//! `std:task` on the scheduler. [`install_task_functions`] spells it as the scripts here do.

use neoc_vm::mlua::{self, Lua, Value};
use neoc_vm::scheduler::Scheduler;

/// Globals `spawn`, `defer`, `delay` and `wait` (those of `std:task`) and `await(task)` (`task:await()`).
fn install_task_functions(lua: &Lua) {
    let globals = lua.globals();
    let task = neoc_vm::modules::build(lua, "std:task").expect("std:task").unwrap();
    for name in ["spawn", "defer", "delay", "wait"] {
        globals.set(name, task.get::<Value>(name).unwrap()).unwrap();
    }
    let wait: Value = lua.load("return function(task) return task:await() end").eval().unwrap();
    globals.set("await", wait).unwrap();
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn engine() -> Lua {
    let lua = neoc_vm::make_engine().expect("engine");
    install_task_functions(&lua);
    lua
}

#[test]
fn wait_returns_elapsed() {
    rt().block_on(async {
        let e = engine();
        let elapsed: f64 =
            e.load("return await(spawn(function() return wait(0.05) end))").eval_async().await.expect("wait");
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
                local got = await(spawn(function(...) return { n = select("#", ...), ... } end, 1, nil, 3))
                assert(got[1] == 1 and got[2] == nil and got[3] == 3 and got.n == 3)
                assert(await(defer(function(x, y) return x .. y end, "x", "y")) == "xy")
                assert(await(delay(0, function(x) return x end, 42)) == 42)

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
        let number: String = e
            .load(
                r#"
                local _, number = pcall(defer, 42)
                -- a coroutine is not a function
                local _, thread = pcall(spawn, coroutine.create(function() end))
                assert(tostring(thread):find("std:task.spawn: argument #1 'body': expected a function", 1, true))
                return tostring(number)
            "#,
            )
            .eval_async()
            .await
            .expect("refusals");
        assert!(number.contains("std:task.defer: argument #1 'body': expected a function"), "{number}");
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
            std::thread::sleep(std::time::Duration::from_millis(30));
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
        let life = neoc_vm::Life::new();
        let e = neoc_vm::make_engine_in(life.clone()).expect("engine");
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
        let life = neoc_vm::Life::new();
        let e = neoc_vm::make_engine_in(life.clone()).expect("engine");
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
