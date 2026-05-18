//! Cooperative task scheduler — `std:task`.
//!
//! Five primitives matching the Roblox `task` library: `spawn`, `defer`,
//! `delay`, `wait`, `cancel`.
//!
//! `task.spawn` returns a `Task` UserData handle mirroring `tokio::JoinHandle`
//! with `:cancel()`, `:is_finished()`, and `:await()` methods.
//!
//! Per-VM scheduling: each Lua engine gets its own `Scheduler` via `set_app_data`.
//! Spawned coroutines run as separate tokio tasks on the same executor; the Lua
//! VM's internal mutex provides cooperative serialisation without blocking OS
//! threads.

use std::sync::Arc;

use futures::StreamExt as _;
use mlua::{Lua, MultiValue, Table, Thread, ThreadStatus, UserData, UserDataMethods, Value};
use parking_lot::Mutex;
use tokio::sync::Notify;

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
    // AbortHandle set after tokio::spawn; None only transiently at init.
    abort: Mutex<Option<tokio::task::AbortHandle>>,
}

impl TaskInner {
    fn new() -> Self {
        Self {
            outcome: Mutex::new(None),
            notify: Arc::new(Notify::new()),
            abort: Mutex::new(None),
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

        methods.add_async_method("await", |lua, this, ()| {
            let inner = this.inner.clone();
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
                outcome_to_multi(
                    &lua,
                    guard
                        .as_ref()
                        .expect("Notify fired but outcome is None — this is a bug"),
                )
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
    set: Mutex<tokio::task::JoinSet<()>>,
    prev_defer: Mutex<Option<Arc<TaskInner>>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            set: Mutex::new(tokio::task::JoinSet::new()),
            prev_defer: Mutex::new(None),
        }
    }

    fn spawn_task(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> tokio::task::AbortHandle {
        self.set.lock().spawn(future)
    }

    /// Drain all in-flight tasks. Called after the main script exits so
    /// spawned coroutines that haven't been explicitly awaited can complete.
    pub async fn wait_all(&self) {
        let mut set = {
            let mut guard = self.set.lock();
            std::mem::take(&mut *guard)
        };
        while (set.join_next().await).is_some() {}
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Shared helper: validate a thread's status before scheduling
// ---------------------------------------------------------------------------

fn validate_thread(thread: &Thread, kind: &'static str) -> mlua::Result<()> {
    match thread.status() {
        ThreadStatus::Finished | ThreadStatus::Error => Err(mlua::Error::external(format!(
            "{kind}: cannot schedule a dead coroutine"
        ))),
        ThreadStatus::Running => Err(mlua::Error::external(format!(
            "{kind}: cannot schedule a running coroutine"
        ))),
        _ => Ok(()),
    }
}

fn into_thread(lua: &Lua, val: Value, kind: &'static str) -> mlua::Result<Thread> {
    match val {
        Value::Function(f) => lua.create_thread(f),
        Value::Thread(t) => {
            validate_thread(&t, kind)?;
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
    let mut stream = thread.into_async::<MultiValue>(args);
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
    let t = lua.create_table()?;

    // task.spawn(fn | thread, ...) — resume synchronously for the first slice,
    // then return a Task handle. If the function yields (async op), the
    // continuation runs as a separate tokio task.
    t.set(
        "spawn",
        lua.create_function(|lua, (fn_val, args): (Value, MultiValue)| {
            let thread = into_thread(lua, fn_val, "task.spawn")?;
            let inner = Arc::new(TaskInner::new());
            let scheduler = lua
                .app_data_ref::<Scheduler>()
                .ok_or_else(|| mlua::Error::external("task: no Scheduler in app data"))?;

            match thread.resume::<MultiValue>(args) {
                Ok(vals) => {
                    if matches!(thread.status(), ThreadStatus::Resumable) {
                        let inner_clone = inner.clone();
                        let abort = scheduler.spawn_task(async move {
                            let outcome = drive_thread(thread, MultiValue::new()).await;
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

    // task.defer(fn | thread, ...) — queue to run after the current batch.
    // Deferred tasks execute in FIFO order; each waits for the previous
    // defer to finish before starting.
    t.set(
        "defer",
        lua.create_function(|lua, (fn_val, args): (Value, MultiValue)| {
            let thread = into_thread(lua, fn_val, "task.defer")?;
            let inner = Arc::new(TaskInner::new());
            let scheduler = lua
                .app_data_ref::<Scheduler>()
                .ok_or_else(|| mlua::Error::external("task: no Scheduler in app data"))?;

            let prev = scheduler.prev_defer.lock().clone();
            let inner_clone = inner.clone();
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
                let outcome = drive_thread(thread, args).await;
                inner_clone.complete(outcome);
            });

            *inner.abort.lock() = Some(abort);
            *scheduler.prev_defer.lock() = Some(inner.clone());
            Ok(TaskHandle { inner })
        })?,
    )?;

    // task.delay(secs, fn, ...) — schedule fn after secs seconds.
    // delay(0) is semantically equivalent to defer.
    t.set(
        "delay",
        lua.create_function(|lua, (secs, fn_val, args): (f64, Value, MultiValue)| {
            let secs = secs.max(0.0);
            let thread = into_thread(lua, fn_val, "task.delay")?;
            let inner = Arc::new(TaskInner::new());
            let scheduler = lua
                .app_data_ref::<Scheduler>()
                .ok_or_else(|| mlua::Error::external("task: no Scheduler in app data"))?;

            let inner_clone = inner.clone();
            let abort = scheduler.spawn_task(async move {
                tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
                let outcome = drive_thread(thread, args).await;
                inner_clone.complete(outcome);
            });

            *inner.abort.lock() = Some(abort);
            Ok(TaskHandle { inner })
        })?,
    )?;

    // task.wait(secs) — yield current coroutine for secs seconds.
    // Must be called from within a coroutine (spawned task or deferred task).
    // Negative values are clamped to 0. Returns elapsed time as a float.
    t.set(
        "wait",
        lua.create_async_function(|_, secs: f64| async move {
            let secs = secs.max(0.0);
            let start = tokio::time::Instant::now();
            tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
            Ok(start.elapsed().as_secs_f64())
        })?,
    )?;

    // task.cancel(handle) — flat Roblox-compatible function; delegates to handle:cancel().
    t.set(
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
            other => Err(mlua::Error::external(format!(
                "task.cancel: expected Task, got {}",
                other.type_name()
            ))),
        })?,
    )?;

    Ok(t)
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
