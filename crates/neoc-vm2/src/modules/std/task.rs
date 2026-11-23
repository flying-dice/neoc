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
//! - `task.cancel` refuses what is not a `Task` as a wrong argument, with its place.
//! - `task.wait` takes a number only (a numeric string was accepted).
//!
//! **Not bound yet: `spawn`, `defer`, `delay` and `Task:await`.** The scheduler has all four
//! ([`spawn`](crate::scheduler::spawn), [`defer`](crate::scheduler::defer), [`delay`](crate::scheduler::delay),
//! [`TaskHandle::outcome`](crate::scheduler::TaskHandle::outcome)) and its tests run them, but mlua-ir cannot
//! declare them:
//! - a task's body is a script function the scheduler runs as a coroutine of its own (the first slice inside the
//!   call, the rest as a tokio task). `mlua_ir::Callback` only calls its function to the end, and keeps it private;
//! - what a task returns reaches whoever awaits it as the value itself (a table is that table, a userdata that
//!   userdata). `mlua_ir::Unknown` is a copy of plain data.

#[mlua_ir::module("std:task")]
pub mod task {
    use mlua_ir::{LuaError, Object};
    use serde::Serialize;

    use crate::limits::sleep_duration;
    use crate::scheduler::TaskHandle;

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

        // TODO(mlua-ir): `await`, which gives the values the task returned as they are, or a `TaskError`.
    }

    // TODO(mlua-ir): `spawn(body)`, `defer(body)` and `delay(secs, body)`, each giving a `Task`.

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
    use mlua::{Lua, MultiValue};

    use super::task::{Task, TaskError};
    use crate::scheduler::{self, Routine};

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let task: mlua::Table = lua.load(r#"return require("std:task")"#).eval().unwrap();
        lua.globals().set("task", task).unwrap();
        lua
    }

    /// A `Task` whose body is the Lua function `source` evaluates to, started after `secs`.
    fn delayed(lua: &Lua, secs: u64, source: &str) -> Task {
        let body: mlua::Function = lua.load(source).eval().unwrap();
        let after = std::time::Duration::from_secs(secs);
        scheduler::delay(lua, after, Routine::Function(body), MultiValue::new()).unwrap().into()
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
        crate::scheduler::tests::install_task_functions(&lua);
        let (done, pending): (i64, i64) = lua
            .load(
                r#"
                local hostile = { 0/0, -1/0, -1, -1e300, 1/0, 1e300, 1.8e19, 2^63 }
                local handles = {}
                for _, secs in ipairs(hostile) do
                    handles[#handles + 1] = spawn(function() task.wait(secs) end)
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
        lua.globals().set("pending", delayed(&lua, 60, "function() end")).unwrap();
        lua.globals().set("other", delayed(&lua, 60, "function() end")).unwrap();
        let seen: Vec<bool> = lua
            .load(
                r#"
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
        lua.globals().set("pending", delayed(&lua, 0, "function() ran = true end")).unwrap();
        let ran: bool =
            lua.load("task.cancel(pending) task.wait(0.05) return ran == true").eval_async().await.unwrap();
        assert!(!ran);
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
    }

    /// The error `await` will give: its kinds and what `tostring` says.
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
