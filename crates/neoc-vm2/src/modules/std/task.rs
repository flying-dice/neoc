//! Cooperative tasks: `std:task`.
//!
//! The scheduler is [`crate::scheduler`]; this is what a script reaches of it.
//!
//! ```lua
//! local task = require("std:task")
//! local slept = task.wait(0.2) -- seconds; other tasks run meanwhile
//! ```
//!
//! Ported from neoc-vm's `std:task`. What changed on purpose:
//! - `spawn`, `defer` and `delay` take a function and nothing else: no arguments to pass on
//!   (`task.spawn(function() f(a, b) end)`), and no coroutine in the function's place.
//! - `Task:await` gives the task's first return value, as it is (a table is that table), or `nil` and a typed
//!   [`TaskError`](task::TaskError) (`cancelled`, or `failed` with the `reason`); it was every return value, or
//!   `nil` and a text.
//! - `task.cancel` refuses what is not a `Task` as a wrong argument, with its place.
//! - `task.wait` and `task.delay` take a number only (a numeric string was accepted).

#[mlua_ir::module("std:task")]
pub mod task {
    use mlua::{Lua, MultiValue, Value};
    use mlua_ir::{Callback, Live, LuaError, Object};
    use serde::Serialize;

    use crate::limits::sleep_duration;
    use crate::scheduler::{self, Outcome, Routine, TaskHandle};

    mlua_ir::unknown_reason!(
        /// Why what a task returns has no declared shape.
        TaskResult = "whatever the task's function returned: the script's own value, handed back as it is"
    );

    /// A task's body: a function of the script's, run as a coroutine of its own. What it returns first is what
    /// `Task:await` gives.
    pub type Body = Callback<fn() -> Live<TaskResult>>;

    /// Why a task has no result.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum TaskError {
        /// The task was cancelled before it ended.
        #[error("the task was cancelled")]
        Cancelled,
        /// The task raised an error, or a host function it called panicked.
        #[error("{reason}")]
        Failed {
            /// What was raised, as text.
            reason: String,
        },
    }

    /// A task: a function the scheduler runs alongside the others.
    #[derive(Clone, Object)]
    pub struct Task {
        handle: TaskHandle,
    }

    impl From<TaskHandle> for Task {
        fn from(handle: TaskHandle) -> Self {
            Self { handle }
        }
    }

    #[mlua_ir::object]
    impl Task {
        /// Cancel the task. `true` when this call cancelled it, `false` when it had already ended.
        pub fn cancel(&self) -> bool {
            self.handle.cancel()
        }

        /// Whether the task has ended: it returned, failed or was cancelled.
        pub fn is_finished(&self) -> bool {
            self.handle.is_finished()
        }

        /// Wait for the task to end: the first value its function returned (as it is: a table is that table), or
        /// `nil` and why there is none. A task that has ended answers at once, every time.
        ///
        /// ```luau
        /// local value, err = task.spawn(function() return 42 end):await()
        /// assert(value == 42 and err == nil)
        /// ```
        pub async fn r#await(&self) -> Result<Live<TaskResult>, TaskError> {
            match self.handle.outcome().await {
                Outcome::Returned(values) => Ok(Live::new(values.into_iter().next().unwrap_or(Value::Nil))),
                Outcome::Failed(reason) => Err(TaskError::Failed { reason }),
                Outcome::Cancelled => Err(TaskError::Cancelled),
            }
        }
    }

    /// Start `body` as a task. It runs at once, inside this call, until it first waits; the rest runs alongside
    /// the caller.
    ///
    /// ```luau
    /// local order = {}
    /// local t = task.spawn(function()
    ///     table.insert(order, "task")
    ///     task.wait(0)
    ///     return "done"
    /// end)
    /// table.insert(order, "caller")
    /// assert(t:await() == "done" and order[1] == "task")
    /// ```
    pub fn spawn(lua: &Lua, body: Body) -> mlua::Result<Task> {
        scheduler::spawn(lua, Routine::Function(body.into_function()), MultiValue::new()).map(Task::from)
    }

    /// Queue `body` to run after the code that is running now reaches a wait or ends. Deferred tasks run in the
    /// order they were queued, each after the one before it has ended.
    pub fn defer(lua: &Lua, body: Body) -> mlua::Result<Task> {
        scheduler::defer(lua, Routine::Function(body.into_function()), MultiValue::new()).map(Task::from)
    }

    /// Run `body` once `secs` seconds have passed. `secs` is read as `wait` reads it: not a number and negative
    /// are no delay, and more than 100 years is 100 years.
    ///
    /// ```luau
    /// local late = task.delay(60, function() end)
    /// assert(late:cancel() and late:is_finished())
    /// ```
    pub fn delay(lua: &Lua, secs: f64, body: Body) -> mlua::Result<Task> {
        let after = sleep_duration(secs);
        scheduler::delay(lua, after, Routine::Function(body.into_function()), MultiValue::new()).map(Task::from)
    }

    /// Wait `secs` seconds without blocking other tasks; the seconds that actually passed. Not a number and
    /// negative are no wait at all (the task still gives the others a turn), and more than 100 years is 100 years.
    ///
    /// ```luau
    /// local slept = task.wait(0.01)
    /// assert(slept >= 0.01)
    /// ```
    #[host("Timer")]
    pub async fn wait(secs: f64) -> f64 {
        let start = tokio::time::Instant::now();
        tokio::time::sleep(sleep_duration(secs)).await;
        start.elapsed().as_secs_f64()
    }

    /// `task:cancel()`, as a function.
    pub fn cancel(task: Task) -> bool {
        task.handle.cancel()
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::task::TaskError;

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let task: mlua::Table = lua.load(r#"return require("std:task")"#).eval().unwrap();
        lua.globals().set("task", task).unwrap();
        lua
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wait_gives_the_time_that_passed() {
        let lua = engine();
        let (slept, zero, negative, nan): (f64, f64, f64, f64) = lua
            .load("return task.wait(0.05), task.wait(0), task.wait(-1), task.wait(0/0)")
            .eval_async()
            .await
            .unwrap();
        assert!((0.05..0.5).contains(&slept), "{slept}");
        for clamped in [zero, negative, nan] {
            assert!((0.0..0.05).contains(&clamped), "{clamped}");
        }
    }

    /// `Duration::from_secs_f64` panics on infinity and on anything past ~1.8e19 s. Every such argument must be a
    /// clamped sleep, not a panic. (A panic inside a task fails the task without a word here, so this test pins the
    /// observable shape; `limits::tests::sleeps_never_panic` is the check on the conversion itself.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hostile_waits_clamp_instead_of_panicking() {
        let lua = engine();
        let (done, pending): (i64, i64) = lua
            .load(
                r#"
                local hostile = { 0/0, -1/0, -1, -1e300, 1/0, 1e300, 1.8e19, 2^63 }
                local handles = {}
                for _, secs in ipairs(hostile) do
                    handles[#handles + 1] = task.spawn(function() task.wait(secs) end)
                end
                task.wait(0.05)
                -- NaN and negatives are zero (done); infinite and huge ones are still sleeping
                local done, pending = 0, 0
                for _, h in ipairs(handles) do
                    if h:is_finished() then done = done + 1 else pending = pending + 1 end
                    h:cancel()
                end
                return done, pending
                "#,
            )
            .eval_async()
            .await
            .expect("hostile durations");
        assert_eq!(
            (done, pending),
            (4, 4),
            "a wait that is not a time ends at once, an endless one is a long one"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_task_is_cancelled_once_by_either_spelling() {
        let lua = engine();
        let seen: Vec<bool> = lua
            .load(
                r#"
                local pending, other = task.delay(60, function() end), task.delay(60, function() end)
                local out = { pending:is_finished(), task.cancel(pending), pending:is_finished(), task.cancel(pending) }
                table.insert(out, other:cancel())
                table.insert(out, other:cancel())
                return out
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(seen, [false, true, true, false, true, false]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancelled_task_never_runs() {
        let lua = engine();
        let (ran, kind, hostile): (bool, String, bool) = lua
            .load(
                r#"
                local ran = false
                local pending = task.delay(0, function() ran = true end)
                task.cancel(pending)
                task.wait(0.05)
                local value, err = pending:await()
                assert(value == nil)
                -- a delay that is not a time is no delay, as a wait is
                local hostile = task.delay(0/0, function() return true end):await()
                return ran, err.kind, hostile
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert!(!ran);
        assert_eq!(kind, "cancelled");
        assert!(hostile);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_is_not_a_task_or_a_number_is_refused_with_its_place() {
        let lua = engine();
        let refused: Vec<String> = lua
            .load(
                r#"
                local out = {}
                for _, call in {
                    function() task.cancel(nil) end,
                    function() task.cancel(42) end,
                    function() task.wait("1") end,
                    function() task.wait() end,
                    function() task.spawn(42) end,
                    function() task.delay("1", function() end) end,
                    function() task.spawn(function() end, 1) end,
                } do
                    local ok, err = pcall(call)
                    assert(not ok)
                    table.insert(out, tostring(err))
                end
                return out
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert!(refused[0].contains("std:task.cancel: argument #1 'task'"), "{refused:#?}");
        assert!(refused[1].contains("std:task.cancel: argument #1 'task'"), "{refused:#?}");
        assert!(refused[2].contains("std:task.wait: argument #1 'secs': expected a number"), "{refused:#?}");
        assert!(refused[3].contains("std:task.wait: argument #1 'secs': expected a number"), "{refused:#?}");
        assert!(refused[4].contains("std:task.spawn: argument #1 'body': expected a function"), "{refused:#?}");
        assert!(refused[5].contains("std:task.delay: argument #1 'secs': expected a number"), "{refused:#?}");
        assert!(refused[6].contains("std:task.spawn"), "{refused:#?}");
    }

    /// The error `await` gives: its kinds and what `tostring` says.
    #[test]
    fn a_task_error_is_a_typed_table() {
        let lua = engine();
        let failed = TaskError::Failed { reason: "boom".to_owned() };
        for (name, error) in [("failed", failed), ("cancelled", TaskError::Cancelled)] {
            let value = mlua_ir::error_value(&error, &lua, "std:task.Task:await").unwrap();
            lua.globals().set(name, value).unwrap();
        }
        let (kind, reason, text, cancelled): (String, String, String, String) =
            lua.load("return failed.kind, failed.reason, tostring(failed), cancelled.kind").eval().unwrap();
        assert_eq!((kind.as_str(), reason.as_str(), cancelled.as_str()), ("failed", "boom", "cancelled"));
        assert_eq!(text, "std:task.Task:await: failed: boom");
    }
}
