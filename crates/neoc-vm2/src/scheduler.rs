//! The cooperative task scheduler behind `std:task`.
//!
//! [`spawn`], [`defer`] and [`delay`] start a script function (or a coroutine the script made) as a task and give
//! back its [`TaskHandle`], which mirrors `tokio::task::JoinHandle`: [`cancel`](TaskHandle::cancel),
//! [`is_finished`](TaskHandle::is_finished) and [`outcome`](TaskHandle::outcome).
//!
//! Per-VM scheduling: each engine gets its own [`Scheduler`] as app data
//! ([`install_runtime_state`](crate::install_runtime_state)). Spawned coroutines run as separate tokio tasks on the
//! same executor, registered with the engine's [`Life`] (so shutting the script down aborts them). When the embedder
//! serialises calls into the VM (see [`crate::lifecycle`]), a task's slices are calls like any other: it takes its
//! turn before it runs and hands the turn back while it waits; otherwise the Lua VM's internal mutex serialises the
//! slices.
//!
//! A host function that panics inside a task fails that task like an error does ([`Outcome::Failed`] with
//! "a host function panicked: ..."): nobody else is above a task to hand the panic to (see [`crate::panics`]).
//!
//! Ported from the scheduler half of neoc-vm's `std/task.rs`: the same rules, as a Rust API that names what it takes
//! ([`Routine`]) and what a task ends with ([`Outcome`]) instead of reading and building Lua values itself.

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
use crate::script_env::mark_driven;

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
        let abort = self.life.spawn(future).abort_handle();
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

/// What a script hands the scheduler to run.
pub enum Routine {
    /// A function: the task runs it in a thread of the host's, which the script holds no handle to.
    Function(Function),
    /// A coroutine the script made: the scheduler drives it from here on, so the script may not resume it too.
    Coroutine(Thread),
}

impl Routine {
    /// The thread the task runs in.
    fn into_thread(self, lua: &Lua) -> mlua::Result<Thread> {
        match self {
            Self::Function(function) => lua.create_thread(function),
            Self::Coroutine(thread) => {
                match thread.status() {
                    ThreadStatus::Finished | ThreadStatus::Error => {
                        return Err(mlua::Error::runtime("cannot schedule a dead coroutine"));
                    }
                    ThreadStatus::Running | ThreadStatus::Normal => {
                        return Err(mlua::Error::runtime("cannot schedule a running coroutine"));
                    }
                    ThreadStatus::Resumable => {}
                }
                mark_driven(lua, &thread)?;
                Ok(thread)
            }
        }
    }
}

/// A task that is not running yet, and the scheduler it joins; `Err` holds the handle of a task that was cancelled
/// on the spot because the script was shut down.
struct Admitted<'lua> {
    scheduler: mlua::AppDataRef<'lua, Scheduler>,
    thread: Thread,
    inner: Arc<TaskInner>,
}

fn admit(lua: &Lua, routine: Routine) -> mlua::Result<Result<Admitted<'_>, TaskHandle>> {
    let thread = routine.into_thread(lua)?;
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

/// Start `routine` as a task, called with `args`. Its first synchronous slice runs inside this call; when it stops
/// at a wait or a yield, the rest runs as a separate task.
///
/// # Errors
/// The coroutine is dead, running, already driven or not the script's own; or the engine has no scheduler.
pub fn spawn(lua: &Lua, routine: Routine, args: MultiValue) -> mlua::Result<TaskHandle> {
    let Admitted { scheduler, thread, inner } = match admit(lua, routine)? {
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

/// Queue `routine` to run, called with `args`, after the current synchronous batch. Deferred tasks run in the
/// order they were queued: each waits for the one before it to end.
///
/// # Errors
/// As [`spawn`].
pub fn defer(lua: &Lua, routine: Routine, args: MultiValue) -> mlua::Result<TaskHandle> {
    let Admitted { scheduler, thread, inner } = match admit(lua, routine)? {
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

/// Run `routine`, called with `args`, once `after` has passed.
///
/// # Errors
/// As [`spawn`].
pub fn delay(lua: &Lua, after: Duration, routine: Routine, args: MultiValue) -> mlua::Result<TaskHandle> {
    let Admitted { scheduler, thread, inner } = match admit(lua, routine)? {
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

#[cfg(test)]
pub(crate) mod tests {
    //! neoc-vm's `std:task` tests, through `std:task` itself. [`install_task_functions`] spells it as the scripts
    //! here do, and adds what the scheduler takes that the module does not offer: a coroutine in a function's
    //! place, and arguments to call it with.

    use mlua::{Lua, MultiValue, Value};

    use std::time::Duration;

    use super::{Routine, Scheduler, TaskHandle};
    use crate::modules::std::task::task::Task;

    fn routine(value: Value) -> mlua::Result<Routine> {
        match value {
            Value::Function(function) => Ok(Routine::Function(function)),
            Value::Thread(thread) => Ok(Routine::Coroutine(thread)),
            _ => Err(mlua::Error::runtime("expected a function or a thread")),
        }
    }

    /// Globals `spawn`, `defer`, `delay` and `wait` (those of `std:task`) and `await(task)` (`task:await()`); and
    /// `spawn_with`, `defer_with` and `delay_with`, the scheduler's own entry points, which also take a coroutine
    /// and the arguments to call it with.
    pub(crate) fn install_task_functions(lua: &Lua) {
        let globals = lua.globals();
        let task = crate::modules::build(lua, "std:task").expect("std:task").unwrap();
        for name in ["spawn", "defer", "delay", "wait"] {
            globals.set(name, task.get::<Value>(name).unwrap()).unwrap();
        }
        let wait: Value = lua.load("return function(task) return task:await() end").eval().unwrap();
        globals.set("await", wait).unwrap();

        let task_of = |handle: TaskHandle| Task::from(handle);
        let spawn = lua.create_function(move |lua, (f, args): (Value, MultiValue)| {
            super::spawn(lua, routine(f)?, args).map(task_of)
        });
        globals.set("spawn_with", spawn.unwrap()).unwrap();
        let defer = lua.create_function(move |lua, (f, args): (Value, MultiValue)| {
            super::defer(lua, routine(f)?, args).map(task_of)
        });
        globals.set("defer_with", defer.unwrap()).unwrap();
        let delay = lua.create_function(move |lua, (secs, f, args): (f64, Value, MultiValue)| {
            super::delay(lua, Duration::from_secs_f64(secs), routine(f)?, args).map(task_of)
        });
        globals.set("delay_with", delay.unwrap()).unwrap();
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        install_task_functions(&lua);
        lua
    }

    #[test]
    fn wait_returns_elapsed() {
        rt().block_on(async {
            let e = engine();
            let elapsed: f64 = e
                .load("return await(spawn(function() return wait(0.05) end))")
                .eval_async()
                .await
                .expect("wait");
            assert!(elapsed >= 0.05, "elapsed={elapsed}");
            assert!(elapsed < 0.5, "elapsed={elapsed}");
        });
    }

    #[test]
    fn spawn_returns_handle_and_runs() {
        rt().block_on(async {
            let e = engine();
            let (ran, kind): (bool, String) = e
                .load(
                    r#"
                    local ran = false
                    local h = spawn(function() ran = true end)
                    await(h)
                    return ran, type(h)
                "#,
                )
                .eval_async()
                .await
                .expect("spawn_runs");
            assert!(ran);
            assert_eq!(kind, "userdata");
        });
    }

    /// From neoc's Luau suite: the first slice runs before `spawn` returns, arguments arrive with their holes, and
    /// the first value a task returns reaches whoever awaits it, each time.
    #[test]
    fn spawn_runs_its_first_slice_at_once_and_passes_arguments_and_results() {
        rt().block_on(async {
            let e = engine();
            let ok: bool = e
                .load(
                    r##"
                    local order = {}
                    local h = spawn(function() table.insert(order, "spawned") end)
                    table.insert(order, "caller")
                    await(h)
                    assert(order[1] == "spawned" and order[2] == "caller")

                    assert(await(spawn(function(...) return select("#", ...) end)) == 0)
                    local got = await(spawn_with(function(...) return { n = select("#", ...), ... } end, 1, nil, 3))
                    assert(got[1] == 1 and got[2] == nil and got[3] == 3 and got.n == 3)
                    assert(await(defer_with(function(x, y) return x .. y end, "x", "y")) == "xy")
                    assert(await(delay_with(0, function(x) return x end, 42)) == 42)

                    local done = spawn(function() return "done" end)
                    assert(await(done) == "done" and await(done) == "done" and done:is_finished())
                    assert(done:cancel() == false)
                    -- the first value only, and no error beside it
                    local one, two, three = await(spawn(function() return 1, 2, 3 end))
                    assert(one == 1 and two == nil and three == nil)
                    local none, err = await(spawn(function() end))
                    assert(none == nil and err == nil)

                    -- a table a task returns is the table itself, not a copy
                    local t = {}
                    assert(await(spawn(function() wait(0) return t end)) == t)
                    return true
                "##,
                )
                .eval_async()
                .await
                .expect("spawn");
            assert!(ok);
        });
    }

    #[test]
    fn deferred_and_delayed_tasks_run_after_the_batch_in_order() {
        rt().block_on(async {
            let e = engine();
            let order: Vec<String> = e
                .load(
                    r#"
                    local order = {}
                    local d = defer(function() table.insert(order, "deferred") end)
                    table.insert(order, "immediate")
                    await(d)
                    local h1 = defer(function() table.insert(order, "1") end)
                    local h2 = defer(function() table.insert(order, "2") end)
                    local h3 = defer(function() table.insert(order, "3") end)
                    await(h1) await(h2) await(h3)
                    local late = delay(0, function() table.insert(order, "delayed") end)
                    table.insert(order, "before")
                    await(late)
                    return order
                "#,
                )
                .eval_async()
                .await
                .expect("defer");
            assert_eq!(order, ["immediate", "deferred", "1", "2", "3", "before", "delayed"]);
        });
    }

    #[test]
    fn many_concurrent_tasks_all_finish() {
        rt().block_on(async {
            let e = engine();
            let count: i64 = e
                .load(
                    r#"
                    local count, handles = 0, {}
                    for i = 1, 100 do
                        handles[i] = spawn(function() wait(i % 2 == 0 and 0 or 0.01) count = count + 1 end)
                    end
                    for _, h in ipairs(handles) do await(h) end
                    return count
                "#,
                )
                .eval_async()
                .await
                .expect("stress");
            assert_eq!(count, 100);
        });
    }

    #[test]
    fn what_cannot_run_is_refused() {
        rt().block_on(async {
            let e = engine();
            let (dead, running, number): (String, String, String) = e
                .load(
                    r#"
                    local co = coroutine.create(function() end)
                    coroutine.resume(co)
                    local _, dead = pcall(spawn_with, co)
                    local _, running = pcall(spawn_with, coroutine.running())
                    local _, number = pcall(defer, 42)
                    -- `std:task` itself takes a function only
                    local _, thread = pcall(spawn, coroutine.create(function() end))
                    assert(tostring(thread):find("std:task.spawn: argument #1 'body': expected a function", 1, true))
                    return tostring(dead), tostring(running), tostring(number)
                "#,
                )
                .eval_async()
                .await
                .expect("refusals");
            assert!(dead.contains("cannot schedule a dead coroutine"), "{dead}");
            assert!(running.contains("cannot schedule a running coroutine"), "{running}");
            assert!(number.contains("std:task.defer: argument #1 'body': expected a function"), "{number}");
        });
    }

    /// A coroutine the script made runs as a task from where it stopped.
    #[test]
    fn a_suspended_coroutine_is_scheduled_from_where_it_stopped() {
        rt().block_on(async {
            let e = engine();
            let out: String = e
                .load(
                    r#"
                    local co = coroutine.create(function(a)
                        local b = coroutine.yield(a .. "1")
                        wait(0)
                        return a .. b
                    end)
                    local _, first = coroutine.resume(co, "x")
                    return first .. await(spawn_with(co, "y"))
                "#,
                )
                .eval_async()
                .await
                .expect("coroutine");
            assert_eq!(out, "x1xy");
        });
    }

    #[test]
    fn cancel_prevents_delayed_execution() {
        rt().block_on(async {
            let e = engine();
            let (ran, first, second): (bool, bool, bool) = e
                .load(
                    r#"
                    local ran = false
                    local dh
                    local h = spawn(function()
                        dh = delay(0.2, function() ran = true end)
                        local first = dh:cancel()
                        wait(0.05)
                        return first
                    end)
                    local first = await(h)
                    return ran, first, dh:cancel()
                "#,
                )
                .eval_async()
                .await
                .expect("cancel_delay");
            assert!(!ran, "delayed task should have been cancelled");
            assert!(first && !second, "only the first cancel is the one that cancelled");
        });
    }

    /// A delayed or deferred task whose time came while the script was still running, and which the script then
    /// cancelled in the same breath, never runs. The pause gives the task's time to come: it is then blocked on the
    /// VM's lock, where an abort does not reach it.
    #[test]
    fn a_task_cancelled_after_its_time_came_but_before_the_script_waited_never_runs() {
        rt().block_on(async {
            let e = engine();
            let pause = e.create_function(|_, ()| {
                std::thread::sleep(Duration::from_millis(30));
                Ok(())
            });
            e.globals().set("pause", pause.unwrap()).unwrap();
            let (ran, kinds): (Vec<String>, Vec<String>) = e
                .load(
                    r#"
                    local ran = {}
                    local delayed = delay(0, function() table.insert(ran, "delayed") end)
                    local later = delay(0.01, function() table.insert(ran, "later") end)
                    local deferred = defer(function() table.insert(ran, "deferred") end)
                    pause()
                    assert(delayed:cancel() and later:cancel() and deferred:cancel())
                    wait(0.05)
                    local kinds = {}
                    for _, t in { delayed, later, deferred } do
                        table.insert(kinds, select(2, await(t)).kind)
                    end
                    return ran, kinds
                "#,
                )
                .eval_async()
                .await
                .expect("cancelled");
            assert!(ran.is_empty(), "cancelled tasks ran: {ran:?}");
            assert_eq!(kinds, ["cancelled", "cancelled", "cancelled"]);
        });
    }

    #[test]
    fn await_cancelled_returns_nil_cancelled() {
        rt().block_on(async {
            let e = engine();
            let (is_nil, err, finished): (bool, String, bool) = e
                .load(
                    r#"
                    local h = delay(1, function() end)
                    local before = h:is_finished()
                    h:cancel()
                    local r, e = await(h)
                    return r == nil, e.kind, not before and h:is_finished()
                "#,
                )
                .eval_async()
                .await
                .expect("await_cancelled");
            assert!(is_nil);
            assert_eq!(err, "cancelled");
            assert!(finished, "a cancelled task is finished, a pending one is not");
        });
    }

    #[test]
    fn wait_all_does_not_deadlock_on_unawaited_async_tasks() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            rt().block_on(async {
                let e = engine();
                e.load("spawn(function() wait(0.01) end)").exec_async().await.expect("exec");

                let scheduler = e.app_data_ref::<Scheduler>().unwrap();
                scheduler.wait_all();
            });
            tx.send(()).ok();
        });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("wait_all deadlocked — did not complete within 10s");
    }

    #[test]
    fn unawaited_error_sets_had_task_errors() {
        rt().block_on(async {
            let e = engine();
            e.load(r#"spawn(function() error("unhandled") end)"#).exec_async().await.expect("exec");

            let scheduler = e.app_data_ref::<Scheduler>().unwrap();
            scheduler.wait_all();
            assert!(scheduler.had_task_errors(), "should flag un-awaited errors");
        });
    }

    #[test]
    fn awaited_error_does_not_set_had_task_errors() {
        rt().block_on(async {
            let e = engine();
            e.load(r#"await(spawn(function() error("handled") end))"#).exec_async().await.expect("exec");

            let scheduler = e.app_data_ref::<Scheduler>().unwrap();
            scheduler.wait_all();
            assert!(!scheduler.had_task_errors(), "awaited errors should not flag");
        });
    }

    #[test]
    fn defer_cancel_before_execution() {
        rt().block_on(async {
            let e = engine();
            let (ran, err): (bool, String) = e
                .load(
                    r#"
                    local ran = false
                    local dh
                    local h = spawn(function()
                        dh = defer(function() ran = true end)
                        dh:cancel()
                        wait(0.05)
                        return ran
                    end)
                    return await(h), select(2, await(dh)).kind
                "#,
                )
                .eval_async()
                .await
                .expect("defer_cancel");
            assert!(!ran, "deferred task should have been cancelled");
            assert_eq!(err, "cancelled");
        });
    }

    /// A script that was shut down starts nothing: a task is cancelled before its first slice.
    #[test]
    fn a_shut_down_script_starts_no_task() {
        rt().block_on(async {
            let life = crate::Life::new();
            let e = crate::make_engine_in(life.clone()).expect("engine");
            install_task_functions(&e);
            life.shutdown();
            let (ran, err): (bool, String) = e
                .load(
                    r#"
                    local ran = false
                    local h = spawn(function() ran = true end)
                    return ran, select(2, await(h)).kind
                "#,
                )
                .eval_async()
                .await
                .expect("the script");
            assert!(!ran);
            assert_eq!(err, "cancelled");
        });
    }

    /// Shutting the script down aborts a task that is waiting.
    #[test]
    fn a_shutdown_aborts_a_waiting_task() {
        rt().block_on(async {
            let life = crate::Life::new();
            let e = crate::make_engine_in(life.clone()).expect("engine");
            install_task_functions(&e);
            e.load("reached = false spawn(function() wait(0.2) reached = true end)")
                .exec_async()
                .await
                .expect("the script");
            life.shutdown();
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            assert!(!e.globals().get::<bool>("reached").unwrap());
        });
    }

    /// An engine with a global `explode()`: a host function that panics.
    fn engine_with_a_panicking_function() -> Lua {
        let e = engine();
        let explode = e.create_function(|_, ()| -> mlua::Result<()> { panic!("the module broke") }).unwrap();
        e.globals().set("explode", explode).unwrap();
        e
    }

    #[test]
    fn a_host_function_that_panics_fails_its_task_in_any_slice() {
        rt().block_on(async {
            let e = engine_with_a_panicking_function();
            let run = e
                .load(
                    r#"
                    local first = spawn(function() explode() end)
                    local later = spawn(function() wait(0) explode() end)
                    local delayed = delay(0, function() explode() end)
                    local r1, e1 = await(first)
                    local r2, e2 = await(later)
                    local r3, e3 = await(delayed)
                    local after = spawn(function() wait(0) return "still runs" end)
                    assert(e1.kind == "failed" and tostring(e1) == "std:task.Task:await: failed: " .. e1.reason)
                    return r1 == nil and r2 == nil and r3 == nil, e1.reason, e2.reason, e3.reason, (await(after))
                "#,
                )
                .eval_async::<(bool, String, String, String, String)>();
            // before the fix a panic after the first slice never completed its handle: `await` waited for ever
            let (nil, e1, e2, e3, after) = tokio::time::timeout(std::time::Duration::from_secs(10), run)
                .await
                .expect("an await of a task that panicked returned")
                .expect("the script ran");
            assert!(nil, "a task that panicked returns no value");
            for err in [e1, e2, e3] {
                assert!(err.contains("a host function panicked: the module broke"), "{err}");
            }
            assert_eq!(after, "still runs");
        });
    }

    #[test]
    fn error_in_spawned_task_isolated() {
        rt().block_on(async {
            let e = engine();
            let (h1_nil, h1_err_contains, h2_ok): (bool, bool, String) = e
                .load(
                    r#"
                    local h1 = spawn(function() error("boom") end)
                    local h2 = spawn(function() return "ok" end)
                    local r1, err1 = await(h1)
                    local r2 = await(h2)
                    return r1 == nil, tostring(err1):find("boom") ~= nil, r2
                "#,
                )
                .eval_async()
                .await
                .expect("isolated");
            assert!(h1_nil);
            assert!(h1_err_contains);
            assert_eq!(h2_ok, "ok");
        });
    }

    /// In a script environment a coroutine handed to the scheduler is the scheduler's: the script may not resume
    /// it too, and one it did not make is refused.
    #[test]
    fn in_a_script_environment_a_scheduled_coroutine_is_no_longer_the_scripts_to_resume() {
        rt().block_on(async {
            let lua = crate::make_bare_engine().expect("engine");
            crate::install_runtime_state(&lua, crate::Life::new());
            install_task_functions(&lua);
            let env = crate::script_env::new_env(&lua).expect("env");
            for name in ["spawn_with", "await", "wait"] {
                env.set(name, lua.globals().get::<Value>(name).unwrap()).unwrap();
            }
            crate::script_env::freeze(&lua, &env).expect("freeze");
            let (resumed, why, foreign, value): (bool, String, String, String) = crate::load_source(
                &lua,
                r#"
                local co = coroutine.create(function() wait(0.05) return "ran" end)
                local h = spawn_with(co)
                local resumed, why = coroutine.resume(co)
                local _, foreign = pcall(spawn_with, coroutine.running())
                return resumed, why, tostring(foreign), (await(h))
                "#,
            )
            .set_environment(env)
            .eval_async()
            .await
            .expect("the script");
            assert!(!resumed, "the scheduler drives it");
            assert!(why.contains("cannot resume non-suspended coroutine"), "{why}");
            assert!(foreign.contains("cannot schedule a running coroutine"), "{foreign}");
            assert_eq!(value, "ran");
        });
    }
}
