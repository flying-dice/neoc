//! Cooperative task scheduler — `std:task`.
//!
//! Five primitives matching the Roblox `task` library: `spawn`, `defer`,
//! `delay`, `wait`, `cancel`.
//!
//! `task.spawn` returns a `Task` `UserData` handle mirroring `tokio::JoinHandle`
//! with `:cancel()`, `:is_finished()`, and `:await()` methods.
//!
//! Per-VM scheduling: each Lua engine gets its own `Scheduler` via `set_app_data`.
//! Spawned coroutines run as separate tokio tasks on the same executor, registered
//! with the engine's [`Life`] (so unloading the app aborts them). When the embedder
//! serialises calls into the VM (see `lifecycle`), a task's slices are calls like any
//! other: it takes its turn before it runs and hands the turn back while it waits in
//! `task.wait`; otherwise the Lua VM's internal mutex serialises the slices.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::StreamExt as _;
use mlua::{thread::ThreadStatus, Lua, MultiValue, Table, Thread, UserData, UserDataMethods, Value};
use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::lua::facade::Module;
use crate::lua::lifecycle::{
    async_function, async_method, first_slice, has_entry, inbound, inherited, mark_driven, resumed, Life,
    Role,
};

// ---------------------------------------------------------------------------
// Task outcome — stored once the task reaches a terminal state
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum TaskOutcome {
    Ok(Vec<Value>),
    Err(String),
    Cancelled,
}

// ---------------------------------------------------------------------------
// TaskInner — state shared between TaskHandle (Lua side) and the tokio task
// ---------------------------------------------------------------------------

struct TaskInner {
    outcome: Mutex<Option<TaskOutcome>>,
    notify: Arc<Notify>,
    abort: Mutex<Option<tokio::task::AbortHandle>>,
    awaited: AtomicBool,
}

impl TaskInner {
    fn new() -> Self {
        Self {
            outcome: Mutex::new(None),
            notify: Arc::new(Notify::new()),
            abort: Mutex::new(None),
            awaited: AtomicBool::new(false),
        }
    }

    fn complete(&self, outcome: TaskOutcome) {
        let mut guard = self.outcome.lock();
        if guard.is_none() {
            *guard = Some(outcome);
            drop(guard);
            self.notify.notify_waiters();
        }
    }

    // Returns true if this call was the one that cancelled (first cancel).
    fn cancel(&self) -> bool {
        // Hold outcome lock to prevent race with complete().
        let mut guard = self.outcome.lock();
        if guard.is_some() {
            return false;
        }
        if let Some(ref abort) = *self.abort.lock() {
            abort.abort();
        }
        *guard = Some(TaskOutcome::Cancelled);
        drop(guard);
        self.notify.notify_waiters();
        true
    }

    fn is_finished(&self) -> bool {
        self.outcome.lock().is_some()
    }
}

// ---------------------------------------------------------------------------
// TaskHandle (UserData exposed to Lua)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct TaskHandle {
    inner: Arc<TaskInner>,
}

impl UserData for TaskHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancel", |_, this, ()| Ok(this.inner.cancel()));

        methods.add_method("is_finished", |_, this, ()| Ok(this.inner.is_finished()));

        async_method(methods, "await", Role::Wait, |lua, this, ()| {
            let inner = this.inner.clone();
            inner.awaited.store(true, Ordering::Relaxed);
            async move {
                // Enable the notification listener BEFORE checking outcome to
                // guarantee we don't miss a notification that fires between
                // the check and the await.
                let notify = inner.notify.clone();
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();

                {
                    let guard = inner.outcome.lock();
                    if let Some(ref outcome) = *guard {
                        return outcome_to_multi(&lua, outcome);
                    }
                }

                notified.await;

                let guard = inner.outcome.lock();
                match guard.as_ref() {
                    Some(outcome) => outcome_to_multi(&lua, outcome),
                    None => Err(mlua::Error::external("task.await: notified but no outcome was recorded")),
                }
            }
        });
    }
}

fn outcome_to_multi(lua: &Lua, outcome: &TaskOutcome) -> mlua::Result<MultiValue> {
    match outcome {
        TaskOutcome::Ok(vals) => Ok(MultiValue::from_vec(vals.clone())),
        TaskOutcome::Err(msg) => {
            let s = lua.create_string(msg.as_bytes())?;
            Ok(MultiValue::from_vec(vec![Value::Nil, Value::String(s)]))
        }
        TaskOutcome::Cancelled => {
            let s = lua.create_string(b"cancelled")?;
            Ok(MultiValue::from_vec(vec![Value::Nil, Value::String(s)]))
        }
    }
}

// ---------------------------------------------------------------------------
// Scheduler — per-VM app data; tracks in-flight task handles for cleanup
// ---------------------------------------------------------------------------

pub struct Scheduler {
    life: Life,
    prev_defer: Mutex<Option<Arc<TaskInner>>>,
    tasks: Mutex<Vec<Arc<TaskInner>>>,
    had_errors: AtomicBool,
}

impl Scheduler {
    /// A scheduler whose tasks register with `life`.
    pub fn with_life(life: Life) -> Self {
        Self {
            life,
            prev_defer: Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
            had_errors: AtomicBool::new(false),
        }
    }

    fn spawn_task(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> tokio::task::AbortHandle {
        self.life.spawn(future).abort_handle()
    }

    fn track(&self, inner: &Arc<TaskInner>) {
        self.tasks.lock().push(inner.clone());
    }

    /// Clean up in-flight tasks after the main script exits: abort everything registered with the
    /// engine's [`Life`] and report un-awaited task errors through tracing.
    pub fn wait_all(&self) {
        self.life.shutdown();

        let tasks = std::mem::take(&mut *self.tasks.lock());
        for inner in &tasks {
            if inner.awaited.load(Ordering::Relaxed) {
                continue;
            }
            let guard = inner.outcome.lock();
            if let Some(TaskOutcome::Err(msg)) = guard.as_ref() {
                self.had_errors.store(true, Ordering::Relaxed);
                tracing::error!(error = %msg, "error in un-awaited task");
            }
        }
    }

    pub fn had_task_errors(&self) -> bool {
        self.had_errors.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Shared helper: validate a thread's status before scheduling
// ---------------------------------------------------------------------------

fn validate_thread(thread: &Thread, kind: &'static str) -> mlua::Result<()> {
    match thread.status() {
        ThreadStatus::Finished | ThreadStatus::Error => {
            Err(mlua::Error::external(format!("{kind}: cannot schedule a dead coroutine")))
        }
        ThreadStatus::Running => {
            Err(mlua::Error::external(format!("{kind}: cannot schedule a running coroutine")))
        }
        _ => Ok(()),
    }
}

fn into_thread(lua: &Lua, val: Value, kind: &'static str) -> mlua::Result<Thread> {
    match val {
        Value::Function(f) => {
            let thread = lua.create_thread(f)?;
            // the scheduler's thread, from its first instruction to the end of its life
            mark_driven(lua, &thread)?;
            Ok(thread)
        }
        Value::Thread(t) => {
            validate_thread(&t, kind)?;
            // the scheduler drives it from here on: the script may not resume it too
            mark_driven(lua, &t)?;
            Ok(t)
        }
        other => Err(mlua::Error::external(format!(
            "{kind}: expected function or thread, got {}",
            other.type_name()
        ))),
    }
}

// ---------------------------------------------------------------------------
// Drive a Lua thread to completion, collecting the final return values.
//
// `into_async` returns a Stream that yields on each explicit coroutine.yield()
// and on the final return. We drain the stream to get the terminal result.
// Async Rust functions (task.wait, etc.) are transparent — into_async drives
// them through tokio's poll/wake mechanism without surfacing them as yields.
// ---------------------------------------------------------------------------

async fn drive_thread(thread: Thread, args: MultiValue) -> TaskOutcome {
    let mut stream = match thread.into_async::<MultiValue>(args) {
        Ok(stream) => stream,
        Err(e) => return TaskOutcome::Err(e.to_string()),
    };
    let mut last: Option<Vec<Value>> = None;
    loop {
        match stream.next().await {
            Some(Ok(vals)) => last = Some(vals.into_vec()),
            Some(Err(e)) => return TaskOutcome::Err(e.to_string()),
            None => break,
        }
    }
    TaskOutcome::Ok(last.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Module constructor
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:task` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]:
/// the hand-written registration carried **no** `#[instrument]` span, and each
/// function raises `mlua::Error` directly on bad input (a dead coroutine, a
/// non-`Task` handle) rather than returning the `(value, error)` channel — so
/// neither `func` nor `try_func` fits. The `Task` userdata keeps its own
/// `impl UserData` and is recorded here for the header via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:task")?;

    // task.spawn(fn | thread, ...) — resume synchronously for the first slice,
    // then return a Task handle. If the function yields (async op), the
    // continuation runs as a separate tokio task.
    m.set(
        "spawn",
        lua.create_function(|lua, (fn_val, args): (Value, MultiValue)| {
            let thread = into_thread(lua, fn_val, "task.spawn")?;
            let inner = Arc::new(TaskInner::new());
            let scheduler = lua
                .app_data_ref::<Scheduler>()
                .ok_or_else(|| mlua::Error::external("task.spawn: no Scheduler in app data"))?;

            scheduler.track(&inner);

            // the first slice runs inside this call, but a wait it stops at is not this call's: see
            // `lifecycle::resumed`. What the call may do is what its task may do: see `Inherited`.
            let from = inherited();
            // A slice that stopped at a wait (an async function, which yields the poll sentinel) has a
            // continuation that takes its turn when the wait ends, and the leaf it stopped at stays alive for it
            // (`Carried`). One that stopped at a plain `coroutine.yield()` has nothing to wait for.
            let stopped_at_wait = |vals: &MultiValue| {
                let mut yielded = vals.iter();
                matches!(
                    (yielded.next(), yielded.next()),
                    (Some(Value::LightUserData(p)), None) if *p == Lua::poll_pending()
                )
            };
            let (first, carried) = first_slice(
                lua,
                from,
                || thread.resume::<MultiValue>(args),
                |slice| matches!(slice, Ok(vals) if stopped_at_wait(vals) && thread.status() == ThreadStatus::Resumable),
            );
            match first {
                Ok(vals) => {
                    if matches!(thread.status(), ThreadStatus::Resumable) {
                        let inner_clone = inner.clone();
                        let lua = lua.clone();
                        let abort = scheduler.spawn_task(async move {
                            // a slice that stopped at a plain yield carries no leaf: the continuation takes its turn
                            // before it resumes Lua (lifecycle rule 5)
                            let outcome =
                                resumed(&lua, from, carried, drive_thread(thread, MultiValue::new())).await;
                            inner_clone.complete(outcome);
                        });
                        *inner.abort.lock() = Some(abort);
                    } else {
                        inner.complete(TaskOutcome::Ok(vals.into_vec()));
                    }
                }
                Err(e) => {
                    inner.complete(TaskOutcome::Err(e.to_string()));
                }
            }

            Ok(TaskHandle { inner })
        })?,
    )?;
    m.record_fn(
        "spawn",
        &[("fn_or_thread", "LuaValue"), ("args", "LuaValue")],
        "Task",
        "Spawn a function or coroutine. The first synchronous slice runs immediately; a yield runs the continuation as a separate task.",
    );

    // task.defer(fn | thread, ...) — queue to run after the current batch.
    // Deferred tasks execute in FIFO order; each waits for the previous
    // defer to finish before starting.
    m.set(
        "defer",
        lua.create_function(|lua, (fn_val, args): (Value, MultiValue)| {
            let thread = into_thread(lua, fn_val, "task.defer")?;
            let inner = Arc::new(TaskInner::new());
            let scheduler = lua
                .app_data_ref::<Scheduler>()
                .ok_or_else(|| mlua::Error::external("task.defer: no Scheduler in app data"))?;

            scheduler.track(&inner);

            let prev = scheduler.prev_defer.lock().clone();
            let from = inherited();
            let inner_clone = inner.clone();
            let lua = lua.clone();
            let abort = scheduler.spawn_task(async move {
                if let Some(prev_inner) = prev {
                    let notify = prev_inner.notify.clone();
                    let notified = notify.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    if !prev_inner.is_finished() {
                        notified.await;
                    }
                }

                // Without a call entry: probe the Lua VM lock before checking cancellation.
                // thread.status() acquires the ReentrantMutex — it blocks until the current
                // synchronous Lua batch yields. Since cancel() runs synchronously before the
                // yield, is_finished() is guaranteed to reflect it once status() returns.
                if !has_entry(&lua) {
                    tokio::task::block_in_place(|| {
                        let _ = thread.status();
                    });
                }

                // With one, the turn is taken first (the spawner's call has ended by then) and
                // cancellation is checked inside it.
                let outcome = inbound(&lua, from, async {
                    if inner_clone.is_finished() {
                        return None;
                    }
                    Some(drive_thread(thread, args).await)
                })
                .await;
                if let Some(outcome) = outcome {
                    inner_clone.complete(outcome);
                }
            });

            *inner.abort.lock() = Some(abort);
            *scheduler.prev_defer.lock() = Some(inner.clone());
            Ok(TaskHandle { inner })
        })?,
    )?;
    m.record_fn(
        "defer",
        &[("fn_or_thread", "LuaValue"), ("args", "LuaValue")],
        "Task",
        "Queue a function to run after the current synchronous batch. Deferred tasks execute in FIFO order.",
    );

    // task.delay(secs, fn, ...) — schedule fn after secs seconds.
    // delay(0) is semantically equivalent to defer.
    m.set(
        "delay",
        lua.create_function(|lua, (secs, fn_val, args): (f64, Value, MultiValue)| {
            let delay = crate::lua::limits::sleep_duration(secs);
            let thread = into_thread(lua, fn_val, "task.delay")?;
            let inner = Arc::new(TaskInner::new());
            let scheduler = lua
                .app_data_ref::<Scheduler>()
                .ok_or_else(|| mlua::Error::external("task.delay: no Scheduler in app data"))?;

            scheduler.track(&inner);

            let from = inherited();
            let inner_clone = inner.clone();
            let lua = lua.clone();
            let abort = scheduler.spawn_task(async move {
                tokio::time::sleep(delay).await;
                let outcome = inbound(&lua, from, drive_thread(thread, args)).await;
                inner_clone.complete(outcome);
            });

            *inner.abort.lock() = Some(abort);
            Ok(TaskHandle { inner })
        })?,
    )?;
    m.record_fn(
        "delay",
        &[("secs", "number"), ("fn_or_thread", "LuaValue"), ("args", "LuaValue")],
        "Task",
        "Schedule a function to run after `secs` seconds. `delay(0, ...)` is equivalent to `defer(...)`.",
    );

    // task.wait(secs) — yield current coroutine for secs seconds.
    // Must be called from within a coroutine (spawned task or deferred task).
    // Negative and NaN values are clamped to 0, and a delay past 100 years (infinity included) to 100 years. Returns elapsed time as a float.
    m.set(
        "wait",
        async_function(lua, Role::Wait, |_, secs: f64| async move {
            let delay = crate::lua::limits::sleep_duration(secs);
            let start = tokio::time::Instant::now();
            tokio::time::sleep(delay).await;
            Ok(start.elapsed().as_secs_f64())
        })?,
    )?;
    m.record_fn(
        "wait",
        &[("secs", "number")],
        "number",
        "Yield the current coroutine for `secs` seconds (NaN and negative clamp to 0, over 100 years clamps to 100 years). Returns the actual elapsed time.",
    );

    // task.cancel(handle) — flat Roblox-compatible function; delegates to handle:cancel().
    m.set(
        "cancel",
        lua.create_function(|_, val: Value| match val {
            Value::UserData(ud) => {
                let h = ud.borrow::<TaskHandle>().map_err(|_| {
                    mlua::Error::external(
                        "task.cancel: expected Task, got a different userdata type. \
                             Pass a handle returned by task.spawn / task.defer / task.delay.",
                    )
                })?;
                Ok(h.inner.cancel())
            }
            other => {
                Err(mlua::Error::external(format!("task.cancel: expected Task, got {}", other.type_name())))
            }
        })?,
    )?;
    m.record_fn(
        "cancel",
        &[("handle", "Task")],
        "bool",
        "Cancel a task by handle. Returns `true` if this call was the one that cancelled it.",
    );

    m.userdata("Task", |t| {
        t.method(
            "cancel",
            &[],
            "bool",
            "Cancel the task. Returns `true` if this call cancelled it; `false` if already finished.",
        )
        .method(
            "is_finished",
            &[],
            "bool",
            "Return `true` if the task has completed (success, error, or cancel).",
        )
        .method(
            "await",
            &[],
            "Result<LuaValue, string>",
            "Wait for the task to finish. Returns its values on success, `(nil, error)` on error, or `(nil, \"cancelled\")` if cancelled.",
        );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::lua;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    fn engine() -> mlua::Lua {
        lua::sandbox::make_engine().expect("engine")
    }

    #[test]
    fn wait_returns_elapsed() {
        rt().block_on(async {
            let e = engine();
            let elapsed: f64 = e
                .load(
                    r#"
                    local task = require("std:task")
                    local h = task.spawn(function()
                        return task.wait(0.05)
                    end)
                    return h:await()
                "#,
                )
                .eval_async()
                .await
                .expect("wait");
            assert!(elapsed >= 0.05, "elapsed={elapsed}");
            assert!(elapsed < 0.5, "elapsed={elapsed}");
        });
    }

    /// Standard Lua 5.1 cannot yield across a C call boundary such as `pcall`; an async wait inside a
    /// protected call must still work (this is the behaviour a LuaJIT-to-5.1 move puts at risk).
    #[test]
    fn wait_inside_pcall_yields_across_the_protected_call() {
        rt().block_on(async {
            let e = engine();
            let (ok, elapsed): (bool, mlua::Value) = e
                .load(
                    r#"
                    local task = require("std:task")
                    local h = task.spawn(function()
                        return pcall(task.wait, 0.05)
                    end)
                    return h:await()
                "#,
                )
                .eval_async()
                .await
                .expect("pcall(wait)");
            assert!(ok, "pcall around an async wait must succeed: {elapsed:?}");
            assert!(matches!(elapsed, mlua::Value::Number(n) if n >= 0.05), "elapsed={elapsed:?}");
        });
    }

    /// `Duration::from_secs_f64` panics on infinity and on anything past ~1.8e19 s; with `panic = "abort"`
    /// that would take the host down. Every such argument must be a clamped sleep, not a panic.
    #[test]
    fn hostile_wait_and_delay_durations_clamp_instead_of_panicking() {
        rt().block_on(async {
            let e = engine();
            let (waits, pending): (i64, bool) = e
                .load(
                    r#"
                    local task = require("std:task")
                    local hostile = { 0/0, -1/0, -1, -1e300, 1/0, 1e300, 1.8e19, 2^63 }
                    local handles = {}
                    for _, secs in ipairs(hostile) do
                        handles[#handles + 1] = task.delay(secs, function() end)
                        handles[#handles + 1] = task.spawn(function() task.wait(secs) end)
                    end
                    task.wait(0.05)
                    -- NaN and negatives are zero (done); infinite and huge ones are still sleeping
                    local done, still = 0, false
                    for i, h in ipairs(handles) do
                        if h:is_finished() then done = done + 1 else still = true end
                        h:cancel()
                    end
                    return done, still
                "#,
                )
                .eval_async()
                .await
                .expect("hostile durations");
            assert!(waits >= 6, "NaN and negative sleeps finish at once, got {waits} finished");
            assert!(pending, "an infinite sleep is clamped to a long one, not to zero");
        });
    }

    #[test]
    fn spawn_returns_handle_and_runs() {
        rt().block_on(async {
            let e = engine();
            let result: bool = e
                .load(
                    r#"
                    local task = require("std:task")
                    local ran = false
                    local h = task.spawn(function() ran = true end)
                    h:await()
                    return ran
                "#,
                )
                .eval_async()
                .await
                .expect("spawn_runs");
            assert!(result);
        });
    }

    #[test]
    fn cancel_prevents_delayed_execution() {
        rt().block_on(async {
            let e = engine();
            let ran: bool = e
                .load(
                    r#"
                    local task = require("std:task")
                    local ran = false
                    local h = task.spawn(function()
                        local dh = task.delay(0.2, function() ran = true end)
                        task.cancel(dh)
                        task.wait(0.05)
                        return ran
                    end)
                    return h:await()
                "#,
                )
                .eval_async()
                .await
                .expect("cancel_delay");
            assert!(!ran, "delayed task should have been cancelled");
        });
    }

    #[test]
    fn await_cancelled_returns_nil_cancelled() {
        rt().block_on(async {
            let e = engine();
            let (is_nil, err): (bool, String) = e
                .load(
                    r#"
                    local task = require("std:task")
                    local h = task.delay(1, function() end)
                    h:cancel()
                    local r, e = h:await()
                    return r == nil, e
                "#,
                )
                .eval_async()
                .await
                .expect("await_cancelled");
            assert!(is_nil);
            assert_eq!(err, "cancelled");
        });
    }

    #[test]
    fn wait_all_does_not_deadlock_on_unawaited_async_tasks() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            rt().block_on(async {
                let e = engine();
                e.load(
                    r#"
                    local task = require("std:task")
                    task.spawn(function()
                        task.wait(0.01)
                    end)
                "#,
                )
                .exec_async()
                .await
                .expect("exec");

                let scheduler = e.app_data_ref::<super::Scheduler>().unwrap();
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
            e.load(
                r#"
                local task = require("std:task")
                task.spawn(function() error("unhandled") end)
            "#,
            )
            .exec_async()
            .await
            .expect("exec");

            let scheduler = e.app_data_ref::<super::Scheduler>().unwrap();
            scheduler.wait_all();
            assert!(scheduler.had_task_errors(), "should flag un-awaited errors");
        });
    }

    #[test]
    fn awaited_error_does_not_set_had_task_errors() {
        rt().block_on(async {
            let e = engine();
            e.load(
                r#"
                local task = require("std:task")
                local h = task.spawn(function() error("handled") end)
                h:await()
            "#,
            )
            .exec_async()
            .await
            .expect("exec");

            let scheduler = e.app_data_ref::<super::Scheduler>().unwrap();
            scheduler.wait_all();
            assert!(!scheduler.had_task_errors(), "awaited errors should not flag");
        });
    }

    #[test]
    fn defer_cancel_before_execution() {
        rt().block_on(async {
            let e = engine();
            let ran: bool = e
                .load(
                    r#"
                    local task = require("std:task")
                    local ran = false
                    local h = task.spawn(function()
                        local dh = task.defer(function() ran = true end)
                        dh:cancel()
                        task.wait(0.05)
                        return ran
                    end)
                    return h:await()
                "#,
                )
                .eval_async()
                .await
                .expect("defer_cancel");
            assert!(!ran, "deferred task should have been cancelled");
        });
    }

    #[test]
    fn error_in_spawned_task_isolated() {
        rt().block_on(async {
            let e = engine();
            let (h1_nil, h1_err_contains, h2_ok): (bool, bool, String) = e
                .load(
                    r#"
                    local task = require("std:task")
                    local h1 = task.spawn(function() error("boom") end)
                    local h2 = task.spawn(function() return "ok" end)
                    local r1, err1 = h1:await()
                    local r2 = h2:await()
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
}
