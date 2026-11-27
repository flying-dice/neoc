//! The cooperative task scheduler behind `std:task`.
//!
//! [`spawn`], [`defer`] and [`delay`] start a script function as a task and give
//! back its [`TaskHandle`], which mirrors `tokio::task::JoinHandle`: [`cancel`](TaskHandle::cancel),
//! [`is_finished`](TaskHandle::is_finished) and [`outcome`](TaskHandle::outcome).
//!
//! Per-VM scheduling: each engine gets its own [`Scheduler`] as app data
//! ([`install_runtime_state`](crate::install_runtime_state)). Spawned tasks run as separate tokio tasks on the
//! same executor, registered with the engine's [`Life`] (so shutting the script down aborts them). When the embedder
//! serialises calls into the VM (see [`crate::lifecycle`]), a task's slices are calls like any other: it takes its
//! turn before it runs and hands the turn back while it waits; otherwise the Lua VM's internal mutex serialises the
//! slices.
//!
//! A host function that panics inside a task fails that task like an error does ([`Outcome::Failed`] with
//! "a host function panicked: ..."): nobody else is above a task to hand the panic to (see [`crate::panics`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt as _;
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, Thread, Value};
use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::lifecycle::{Life, Pruned, entrance, first_slice, inherited};
use crate::panics;

/// How a task ended.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// It returned these values.
    Returned(Vec<Value>),
    /// It raised this error, or a host function it called panicked.
    Failed(String),
    /// It was cancelled before it ended.
    Cancelled,
}

/// The state a [`TaskHandle`] shares with the tokio task that runs the script's code.
struct TaskInner {
    outcome: Mutex<Option<Outcome>>,
    notify: Notify,
    abort: Mutex<Option<tokio::task::AbortHandle>>,
    awaited: AtomicBool,
}

impl TaskInner {
    fn new() -> Self {
        Self {
            outcome: Mutex::new(None),
            notify: Notify::new(),
            abort: Mutex::new(None),
            awaited: AtomicBool::new(false),
        }
    }

    /// Record how the task ended, unless it already has an outcome (it was cancelled first).
    fn complete(&self, outcome: Outcome) {
        let mut guard = self.outcome.lock();
        if guard.is_none() {
            *guard = Some(outcome);
            drop(guard);
            self.notify.notify_waiters();
        }
    }

    /// Whether this call was the one that cancelled the task.
    fn cancel(&self) -> bool {
        // the outcome stays locked, so a `complete` cannot slip in between the check and the abort
        let mut guard = self.outcome.lock();
        if guard.is_some() {
            return false;
        }
        if let Some(abort) = &*self.abort.lock() {
            abort.abort();
        }
        *guard = Some(Outcome::Cancelled);
        drop(guard);
        self.notify.notify_waiters();
        true
    }

    fn is_finished(&self) -> bool {
        self.outcome.lock().is_some()
    }

    /// Wait until the task has an outcome.
    async fn finished(&self) {
        // the listener is enabled before the outcome is checked, so a notification between the check and the
        // await is not missed
        let notified = self.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.is_finished() {
            notified.await;
        }
    }

    /// The error of a task that failed and that nobody has awaited.
    fn unobserved_error(&self) -> Option<String> {
        if self.awaited.load(Ordering::Relaxed) {
            return None;
        }
        match &*self.outcome.lock() {
            Some(Outcome::Failed(error)) => Some(error.clone()),
            _ => None,
        }
    }
}

/// A task a script started. Clones are the same task.
#[derive(Clone)]
pub struct TaskHandle {
    inner: Arc<TaskInner>,
}

impl TaskHandle {
    /// Cancel the task. `true` when this call cancelled it, `false` when it had already ended.
    #[must_use = "whether the task was still running"]
    pub fn cancel(&self) -> bool {
        self.inner.cancel()
    }

    /// Whether the task has ended: it returned, failed or was cancelled.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }

    /// Wait for the task to end; how it ended. Asking is what marks a failure as seen
    /// ([`Scheduler::wait_all`] reports the others). A task that has ended answers at once, every time.
    pub async fn outcome(&self) -> Outcome {
        self.inner.awaited.store(true, Ordering::Relaxed);
        loop {
            self.inner.finished().await;
            if let Some(outcome) = self.inner.outcome.lock().clone() {
                return outcome;
            }
        }
    }
}

/// An engine's tasks: what [`Scheduler::wait_all`] aborts and reports.
pub struct Scheduler {
    life: Life,
    /// This engine's own tasks: what [`Scheduler::wait_all`] aborts, and what is aborted when the engine (and so
    /// the scheduler) is dropped, as a worker's tasks end with the worker. The same tasks are registered with the
    /// [`Life`], which aborts them when an embedder shuts the script down.
    handles: Mutex<Pruned<tokio::task::AbortHandle>>,
    prev_defer: Mutex<Option<Arc<TaskInner>>>,
    /// Tasks that are still running, or that failed and nobody has awaited yet (what [`Scheduler::wait_all`]
    /// reports); the others are forgotten as the list grows, so a script that spawns for ever does not keep every
    /// outcome.
    tasks: Mutex<Pruned<Arc<TaskInner>>>,
    had_errors: AtomicBool,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.abort_own();
    }
}

impl Scheduler {
    /// A scheduler with a registry of its own.
    #[must_use]
    pub fn new() -> Self {
        Self::with_life(Life::new())
    }

    /// A scheduler whose tasks also register with `life`.
    #[must_use]
    pub fn with_life(life: Life) -> Self {
        Self {
            life,
            handles: Mutex::new(Pruned::new()),
            prev_defer: Mutex::new(None),
            tasks: Mutex::new(Pruned::new()),
            had_errors: AtomicBool::new(false),
        }
    }

    fn spawn_task(&self, future: impl Future<Output = ()> + Send + 'static) -> tokio::task::AbortHandle {
        // the task carries the span it was started in: what it does later is in the trace of the call that started it
        let abort =
            self.life.spawn(tracing::Instrument::instrument(future, tracing::Span::current())).abort_handle();
        self.handles.lock().push(abort.clone(), |handle| !handle.is_finished());
        abort
    }

    /// Take a task the script is starting into the scheduler, or, when the script was shut down, cancel it
    /// without running its first slice (`false`).
    fn admit(&self, inner: &Arc<TaskInner>) -> bool {
        if self.life.is_closed() {
            inner.complete(Outcome::Cancelled);
            return false;
        }
        self.tasks
            .lock()
            .push(inner.clone(), |task| !task.is_finished() || task.unobserved_error().is_some());
        true
    }

    fn abort_own(&self) {
        for handle in std::mem::take(&mut self.handles.lock().items) {
            handle.abort();
        }
    }

    /// Clean up in-flight tasks after the main script exits.
    ///
    /// Lua async operations (`task.wait`, ...) cannot be driven after the main `exec_async` context returns: their
    /// `AsyncThread` streams block inside the Lua VM lock and never yield. Neither `tokio::time::timeout` nor an
    /// abort can interrupt a task that is synchronously blocked. This aborts this scheduler's tasks and only those:
    /// servers, workers and listeners the script started stay up for whatever runs next, and the engine's [`Life`]
    /// stays open.
    ///
    /// After that, reports un-awaited task errors to stderr. (Synchronous: it never waited for anything, and a
    /// future that held an engine's app data across an `.await` was not `Send`, which the top-level future of
    /// `neoc run` must be.)
    pub fn wait_all(&self) {
        self.abort_own();

        let tasks = std::mem::take(&mut self.tasks.lock().items);
        for error in tasks.iter().filter_map(|task| task.unobserved_error()) {
            self.had_errors.store(true, Ordering::Relaxed);
            eprintln!("error in un-awaited task: {error}");
        }
    }

    /// Whether [`Scheduler::wait_all`] found a task that failed and that nobody awaited.
    #[must_use]
    pub fn had_task_errors(&self) -> bool {
        self.had_errors.load(Ordering::Relaxed)
    }
}

/// A task that is not running yet, and the scheduler it joins; `Err` holds the handle of a task that was cancelled
/// on the spot because the script was shut down.
struct Admitted<'lua> {
    scheduler: mlua::AppDataRef<'lua, Scheduler>,
    thread: Thread,
    inner: Arc<TaskInner>,
}

fn admit(lua: &Lua, body: Function) -> mlua::Result<Result<Admitted<'_>, TaskHandle>> {
    // the task runs in a thread of the host's, which the script holds no handle to
    let thread = lua.create_thread(body)?;
    let inner = Arc::new(TaskInner::new());
    let scheduler = lua
        .app_data_ref::<Scheduler>()
        .ok_or_else(|| mlua::Error::runtime("this engine has no task scheduler (`install_runtime_state`)"))?;
    if !scheduler.admit(&inner) {
        return Ok(Err(TaskHandle { inner }));
    }
    Ok(Ok(Admitted { scheduler, thread, inner }))
}

/// Drive a Lua thread to completion, collecting the final return values.
///
/// `into_async` returns a stream that yields on each explicit `coroutine.yield()` and on the final return; draining
/// it gives the terminal result. Async host functions are transparent: `into_async` drives them through tokio's
/// poll and wake without surfacing them as yields.
async fn drive_thread(thread: Thread, args: MultiValue) -> Outcome {
    let mut stream = match thread.into_async::<MultiValue>(args) {
        Ok(stream) => stream,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let mut last: Option<Vec<Value>> = None;
    loop {
        // a host function that panics fails the task: the outcome is recorded, so awaiting it does not wait for ever
        match panics::caught(stream.next()).await {
            Ok(Some(Ok(values))) => last = Some(values.into_vec()),
            Ok(Some(Err(e))) => return Outcome::Failed(e.to_string()),
            Ok(None) => break,
            Err(message) => return Outcome::Failed(panics::error(&message).to_string()),
        }
    }
    Outcome::Returned(last.unwrap_or_default())
}

/// Run a task that was queued ([`defer`], [`delay`]) now that its time has come, unless it was cancelled meanwhile
/// (`None`: its outcome is the cancellation).
///
/// Aborting the tokio task is not enough to cancel it: an abort lands at an `.await`, and a queued task whose time
/// came while the script was running is not at one. It is blocked inside the first resume, on the VM's lock, and
/// runs the body as soon as the script reaches a wait, although the script cancelled it before that
/// (`local t = task.delay(0, f); task.cancel(t)` ran `f` now and then). So the cancellation is read here, once the
/// script's running batch can no longer be about to cancel:
/// - with a call entry, this runs inside the task's turn ([`Entrance::inbound`]): the call that queued it, and
///   could cancel it, has handed the turn over;
/// - without one, the VM's lock is probed first. `thread.status()` takes it, so it blocks until the running batch
///   reaches a wait; a cancel in that batch ran before that, and `is_finished()` shows it once `status()` returns.
///
/// [`Entrance::inbound`]: crate::lifecycle::Entrance::inbound
async fn started_unless_cancelled(
    entrance: &crate::lifecycle::Entrance,
    task: &TaskInner,
    thread: Thread,
    args: MultiValue,
) -> Option<Outcome> {
    if !entrance.is_set() {
        // on a current-thread runtime nothing runs beside the script's batch: there is no lock to wait for, and
        // `block_in_place` would panic
        let multi_thread = tokio::runtime::Handle::try_current()
            .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
        if multi_thread {
            tokio::task::block_in_place(|| {
                let _ = thread.status();
            });
        }
    }
    if task.is_finished() {
        return None;
    }
    Some(drive_thread(thread, args).await)
}

/// Start `body` as a task, called with `args`. Its first synchronous slice runs inside this call; when it stops
/// at a wait or a yield, the rest runs as a separate task.
///
/// # Errors
/// The engine has no scheduler.
pub fn spawn(lua: &Lua, body: Function, args: MultiValue) -> mlua::Result<TaskHandle> {
    let Admitted { scheduler, thread, inner } = match admit(lua, body)? {
        Ok(admitted) => admitted,
        Err(cancelled) => return Ok(cancelled),
    };

    // the first slice runs inside this call, but a wait it stops at is not this call's: see `lifecycle::resumed`.
    // What the call may do is what its task may do: see `Inherited`.
    let from = inherited();
    // A slice that stopped at a wait (an async function, which yields the poll sentinel) has a continuation that
    // takes its turn when the wait ends, and the leaf it stopped at stays alive for it (`Carried`). One that
    // stopped at a plain `coroutine.yield()` has nothing to wait for.
    let stopped_at_wait = |values: &MultiValue| {
        let mut yielded = values.iter();
        matches!(
            (yielded.next(), yielded.next()),
            (Some(Value::LightUserData(p)), None) if *p == Lua::poll_pending()
        )
    };
    let (first, carried) = first_slice(
        lua,
        from.clone(),
        // a panic in the first slice fails the task, as one in a later slice does (`drive_thread`)
        || {
            panics::caught_sync(|| thread.resume::<MultiValue>(args))
                .unwrap_or_else(|message| Err(panics::error(&message)))
        },
        |slice| matches!(slice, Ok(values) if stopped_at_wait(values) && thread.status() == ThreadStatus::Resumable),
    );
    match first {
        Ok(_) if thread.status() == ThreadStatus::Resumable => {
            let task = inner.clone();
            let entrance = entrance(lua);
            let abort = scheduler.spawn_task(async move {
                // a slice that stopped at a plain yield carries no leaf: the continuation takes its turn before
                // it resumes Lua (lifecycle rule 5)
                let outcome = entrance.resumed(from, carried, drive_thread(thread, MultiValue::new())).await;
                task.complete(outcome);
            });
            *inner.abort.lock() = Some(abort);
        }
        Ok(values) => inner.complete(Outcome::Returned(values.into_vec())),
        Err(e) => inner.complete(Outcome::Failed(e.to_string())),
    }
    Ok(TaskHandle { inner })
}

/// Queue `body` to run, called with `args`, after the current synchronous batch. Deferred tasks run in the
/// order they were queued: each waits for the one before it to end.
///
/// # Errors
/// As [`spawn`].
pub fn defer(lua: &Lua, body: Function, args: MultiValue) -> mlua::Result<TaskHandle> {
    let Admitted { scheduler, thread, inner } = match admit(lua, body)? {
        Ok(admitted) => admitted,
        Err(cancelled) => return Ok(cancelled),
    };

    let previous = scheduler.prev_defer.lock().clone();
    let from = inherited();
    let task = inner.clone();
    let entrance = entrance(lua);
    let abort = scheduler.spawn_task(async move {
        if let Some(previous) = previous {
            previous.finished().await;
        }

        let outcome = entrance.inbound(from, started_unless_cancelled(&entrance, &task, thread, args)).await;
        if let Some(outcome) = outcome {
            task.complete(outcome);
        }
    });

    *inner.abort.lock() = Some(abort);
    *scheduler.prev_defer.lock() = Some(inner.clone());
    Ok(TaskHandle { inner })
}

/// Run `body`, called with `args`, once `after` has passed.
///
/// # Errors
/// As [`spawn`].
pub fn delay(lua: &Lua, after: Duration, body: Function, args: MultiValue) -> mlua::Result<TaskHandle> {
    let Admitted { scheduler, thread, inner } = match admit(lua, body)? {
        Ok(admitted) => admitted,
        Err(cancelled) => return Ok(cancelled),
    };

    let from = inherited();
    let task = inner.clone();
    let entrance = entrance(lua);
    let abort = scheduler.spawn_task(async move {
        tokio::time::sleep(after).await;
        let outcome = entrance.inbound(from, started_unless_cancelled(&entrance, &task, thread, args)).await;
        if let Some(outcome) = outcome {
            task.complete(outcome);
        }
    });

    *inner.abort.lock() = Some(abort);
    Ok(TaskHandle { inner })
}
