#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! What a script starts, and how long it lives: with no embedder (what `neoc run` does, and must keep doing) and
//! under an embedder's `Life` (shut down, or only its background work stopped).
//!
//! The assertions are about what the work *does* (a counter that stops moving, a port that can be bound again, a
//! request that is answered), not about a registry's bookkeeping, which `Life::stop_background` empties as it
//! aborts.

use std::{sync::Arc, time::Duration};

use mlua::Lua;
use neoc_vm::lua::{
    lifecycle::Life,
    modules,
    sandbox::{
        install_runtime_state, make_bare_engine, make_engine, make_engine_in, set_worker_engine,
    },
    std::task::Scheduler,
};

mod common;
use common::{ends, free_addr, http_get, serve_ok, start_static, static_site};

const COUNTERS: &str = r#"
    local task = require("std:task")
    local collections = require("std:collections")
    _G.ticks = collections.Counter.new(0)
    _G.delayed = collections.Counter.new(0)
    _G.worker_ticks = collections.Counter.new(0)
    task.spawn(function() while true do task.wait(0.005) _G.ticks:increment() end end)
    task.delay(0.001, function() while true do task.wait(0.005) _G.delayed:increment() end end)
    _G.worker = require("std:thread").spawn {
      source = "local t = require('std:task') local th = require('std:thread') "
        .. "while true do t.wait(0.005) th.shared.c:increment() end",
      shared = { c = _G.worker_ticks },
    }
"#;

/// The values of the three counters `COUNTERS` starts.
async fn ticks(lua: &Lua) -> [i64; 3] {
    lua.load("return { _G.ticks:get(), _G.delayed:get(), _G.worker_ticks:get() }")
        .eval_async::<Vec<f64>>()
        .await
        .map(|v| [v[0] as i64, v[1] as i64, v[2] as i64])
        .unwrap()
}

/// Did the counters move in the next 150 ms?
async fn moving(lua: &Lua) -> [bool; 3] {
    let before = ticks(lua).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let after = ticks(lua).await;
    [
        after[0] > before[0],
        after[1] > before[1],
        after[2] > before[2],
    ]
}

async fn get(addr: &str) -> Option<String> {
    http_get(addr, "/").await
}

// ---- no embedder: what `neoc run` does --------------------------------------------------------------------------

/// A worker's own tasks end with the worker, whatever the parent's registry is: the worker's engine is dropped
/// when it returns, and the scheduler of that engine aborts its tasks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_workers_own_tasks_end_with_the_worker() {
    for lua in [make_engine().unwrap(), make_engine_in(Life::new()).unwrap()] {
        let out: f64 = lua
            .load(
                r#"local collections = require("std:collections")
                   local c = collections.Counter.new(0)
                   local worker = require("std:thread").spawn {
                     source = "local t = require('std:task') local th = require('std:thread') "
                       .. "t.spawn(function() while true do t.wait(0.01) th.shared.c:increment() end end) return 1",
                     shared = { c = c },
                   }
                   worker:await()
                   require("std:task").wait(0.1)   -- anything the worker's task did before it ended
                   local at_return = c:get()
                   require("std:task").wait(0.3)
                   return c:get() - at_return"#,
            )
            .eval_async()
            .await
            .unwrap();
        assert!(
            out == 0.0,
            "the worker's task kept running after the worker ended ({out} more ticks)"
        );
    }
}

/// The same for a task the worker `defer`red or `delay`ed: the engine is dropped when the worker returns, and these
/// tasks (which hold the engine weakly) end with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_workers_deferred_and_delayed_tasks_end_with_the_worker() {
    for start in ["t.defer(", "t.delay(0, "] {
        for lua in [make_engine().unwrap(), make_engine_in(Life::new()).unwrap()] {
            let source = format!(
                r#"local collections = require("std:collections")
                   local c = collections.Counter.new(0)
                   local worker = require("std:thread").spawn {{
                     source = "local t = require('std:task') local th = require('std:thread') "
                       .. "{start}function() while true do t.wait(0.01) th.shared.c:increment() end end) return 1",
                     shared = {{ c = c }},
                   }}
                   worker:await()
                   require("std:task").wait(0.1)
                   local at_return = c:get()
                   require("std:task").wait(0.3)
                   return c:get() - at_return"#
            );
            let out: f64 = lua.load(source).eval_async().await.unwrap();
            assert!(
                out == 0.0,
                "{start}: the worker's task kept running after the worker ended ({out} more ticks)"
            );
        }
    }
}

/// After a script ends, the CLI aborts the tasks of its scheduler (`wait_all`) and drops the engine: the servers,
/// workers and listeners the script started stay up for the next script, as on `main`. (`neoc run a.lua b.lua`
/// gives each file an engine of its own.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_a_script_leaves_running_outlives_its_tasks_being_drained() {
    let addr = free_addr();
    let lua = make_engine().unwrap();
    lua.load(format!(
        r#"local task = require("std:task")
           local listener = require("std:net").TcpListener.bind({addr:?})
           require("vnd:hyper").serve(listener, function() return {{ status = 200, body = "hi from a" }} end)
           _G.ticks = require("std:collections").Counter.new(0)
           task.spawn(function() while true do task.wait(0.005) _G.ticks:increment() end end)"#
    ))
    .exec_async()
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    // what the CLI does at the end of a script
    {
        let scheduler = lua.app_data_ref::<Scheduler>().unwrap();
        scheduler.wait_all().await;
    }
    // the task it drained is stopped
    let ticks = |lua: &Lua| lua.load("return _G.ticks:get()").eval::<f64>().unwrap();
    let after_drain = ticks(&lua);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(ticks(&lua) == after_drain, "the drained task kept running");
    // and the engine is dropped, as the CLI drops it: the server holds the VM it needs
    drop(lua);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let answer = get(&addr).await.expect("the server of a.lua is still up");
    assert!(answer.contains("hi from a"), "{answer}");
}

/// With no embedder a coroutine a script resumed that is suspended at a host call is not abandoned when the first
/// slice of a task makes another host call: it finishes, as it does in the main chunk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_coroutine_resumed_in_a_first_slice_finishes_with_no_embedder() {
    let lua = make_engine().unwrap();
    let out: String = lua
        .load(
            r#"local task = require("std:task")
               local result
               local h = task.spawn(function()
                 local co = coroutine.create(function() task.wait(0.05) return "done" end)
                 coroutine.resume(co)
                 task.wait(0.01)
                 local ok, v
                 repeat ok, v = coroutine.resume(co) task.wait(0.01) until coroutine.status(co) == "dead"
                 result = tostring(ok) .. ":" .. tostring(v)
               end)
               h:await()
               return result"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(out, "true:done");
}

// ---- under an embedder's Life ------------------------------------------------------------------------------------

/// Shutting a script down stops what it started: its tasks, its delayed tasks and its workers stop doing
/// anything, its server stops answering and reports itself closed, a script waiting in `server:wait()` returns, and
/// a task started afterwards is cancelled without running.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutting_a_script_down_stops_everything_it_started() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).unwrap();
    let addr = free_addr();
    lua.load(COUNTERS).exec_async().await.unwrap();
    lua.load(serve_ok(&addr)).exec_async().await.unwrap();
    assert_eq!(
        moving(&lua).await,
        [true, true, true],
        "control: all running"
    );
    assert!(get(&addr).await.unwrap().contains("ok"), "control: serving");
    // a script sits in `server:wait()`: it must end when the script is shut down
    let waiting = {
        let lua = lua.clone();
        tokio::spawn(async move {
            lua.load("_G.server:wait() return 'ended'")
                .eval_async::<String>()
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!waiting.is_finished());

    life.shutdown();

    let ended = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("wait() ended with the shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(ended, "ended");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        moving(&lua).await,
        [false, false, false],
        "the tasks, the delayed task and the worker stopped"
    );
    assert!(get(&addr).await.is_none(), "the server stopped answering");
    let after: Vec<String> = lua
        .load(
            r#"local ok, e = pcall(function() return _G.server:addr() end)
               local ran = false
               local h = require("std:task").spawn(function() ran = true end)
               local deferred = require("std:task").defer(function() ran = true end)
               local delayed = require("std:task").delay(0, function() ran = true end)
               local _, why = h:await()
               return { tostring(ok), tostring(e), tostring(ran), tostring(why), tostring(select(2, deferred:await())), tostring(select(2, delayed:await())) }"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(after[0], "false", "server:addr() after the shutdown");
    assert!(after[1].contains("server is closed"), "{}", after[1]);
    assert_eq!(after[2], "false", "a task started after the shutdown ran");
    assert_eq!(&after[3..], ["cancelled", "cancelled", "cancelled"]);
}

/// The port is free the moment `shutdown` returns, not when the runtime next gets to the aborted accept loop.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_port_is_free_when_shutdown_returns() {
    for round in 0..20 {
        let life = Life::new();
        let lua = make_engine_in(life.clone()).unwrap();
        let addr: String = lua
            .load(
                r#"local listener = require("std:net").TcpListener.bind("127.0.0.1:0")
                   _G.server = require("vnd:hyper").serve(listener, function() return { status = 200, body = "ok" } end)
                   return _G.server:addr()"#,
            )
            .eval_async()
            .await
            .unwrap();
        // let a connection in, so the accept loop has done real work
        assert!(get(&addr).await.unwrap().contains("ok"));
        life.shutdown();
        assert!(
            tokio::net::TcpListener::bind(&addr).await.is_ok(),
            "round {round}: {addr} was not free when shutdown returned"
        );
    }
}

/// `stop_background` on its own: it aborts what the script started, releases its sockets, ends a `serve_static` it
/// is blocked in, and stays open: the script goes on and may start new work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_background_stops_the_background_and_stays_open() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).unwrap();
    let addr = free_addr();
    let static_addr = free_addr();
    let (dir, pattern) = static_site("lifetimes-static");
    lua.load(COUNTERS).exec_async().await.unwrap();
    lua.load(serve_ok(&addr)).exec_async().await.unwrap();
    // a script blocked in `serve_static` (a call, not a task: the registry cannot abort it)
    let serving = start_static(&lua, &pattern, &static_addr);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        http_get(&static_addr, "/a.txt")
            .await
            .unwrap()
            .contains("static a")
    );
    assert_eq!(moving(&lua).await, [true, true, true], "control");

    life.stop_background();

    let ended = ends(serving, "serve_static (stop_background)").await;
    assert_eq!(ended, "ended");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        moving(&lua).await,
        [false, false, false],
        "tasks and worker stopped"
    );
    assert!(get(&addr).await.is_none(), "the server stopped");
    assert!(
        get(&static_addr).await.is_none(),
        "the static server stopped"
    );
    assert!(tokio::net::TcpListener::bind(&addr).await.is_ok());
    assert!(
        !life.is_closed(),
        "stop_background does not close the registry"
    );
    // new work is accepted and runs
    let out: String = lua
        .load(
            r#"local task = require("std:task")
               local h = task.spawn(function() task.wait(0.01) return "new work" end)
               return h:await()"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(out, "new work");
    // and the port can be served again by the replacement
    let again: String = lua
        .load(format!(
            r#"local listener = require("std:net").TcpListener.bind({addr:?})
               local server = require("vnd:hyper").serve(listener, function() return {{ status = 200, body = "second" }} end)
               return server:addr()"#
        ))
        .eval_async()
        .await
        .unwrap();
    assert_eq!(again, addr);
    assert!(get(&addr).await.unwrap().contains("second"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `serve_static` ends with the script when the registry is already shut down, too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_static_on_a_registry_that_is_already_shut_down_returns_at_once() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).unwrap();
    life.shutdown();
    let (dir, pattern) = static_site("lifetimes-closed");
    let ended = ends(
        start_static(&lua, &pattern, &free_addr()),
        "serve_static on a closed registry",
    )
    .await;
    assert_eq!(ended, "ended");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- workers get the modules their parent was offered --------------------------------------------------------------

const WORKER_PROBE: &str = r#"
    local ok = require("std:thread").spawn {
      source = "local out = {} for _, n in ipairs { 'std:workers', 'lib:compile', 'lib:test', 'lib:json' } do "
        .. "out[#out + 1] = n .. '=' .. tostring((pcall(require, n))) end return table.concat(out, ',')"
    }:await()
    return ok
"#;

/// A worker is offered the modules its parent was offered: refused `std:workers` (which fires the process-wide
/// shutdown signal that every other script's `server:wait()` selects on), the worker is refused it too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_worker_is_offered_only_what_its_parent_was_offered() {
    let allowed: Vec<&str> = modules::names()
        .filter(|n| !["std:workers", "lib:compile", "lib:test"].contains(n))
        .collect();

    // the embedder offers a subset with `install_allowed`: workers are built with the same subset
    let lua = make_bare_engine().unwrap();
    install_runtime_state(&lua, Life::new());
    modules::install_allowed(&lua, &allowed).unwrap();
    let out: String = lua.load(WORKER_PROBE).eval_async().await.unwrap();
    assert_eq!(
        out,
        "std:workers=false,lib:compile=false,lib:test=false,lib:json=true"
    );
    // a worker of that worker too
    let nested: String = lua
        .load(
            r#"return require("std:thread").spawn {
                 source = [[ return require("std:thread").spawn { source = "return tostring((pcall(require, 'std:workers')))" }:await() ]]
               }:await()"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(nested, "false");

    // an embedder with its own `require` says how to build a worker
    let lua = make_bare_engine().unwrap();
    install_runtime_state(&lua, Life::new());
    modules::install_allowed(&lua, &allowed).unwrap();
    let only_json = Arc::new(|life: Life| {
        let lua = make_bare_engine()?;
        install_runtime_state(&lua, life);
        modules::install_allowed(&lua, &["lib:json", "std:thread"])?;
        Ok(lua)
    });
    set_worker_engine(&lua, only_json);
    let out: String = lua.load(WORKER_PROBE).eval_async().await.unwrap();
    assert_eq!(
        out,
        "std:workers=false,lib:compile=false,lib:test=false,lib:json=true"
    );
    let other: String = lua
        .load(
            r#"return require("std:thread").spawn {
                 source = "return tostring((pcall(require, 'lib:hex')))"
               }:await()"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(
        other, "false",
        "the embedder's worker engine is what the worker gets"
    );

    // a bare engine whose embedder offers modules through its own `require` and never said how to build a
    // worker: a worker is refused (it fails closed), and works once the embedder says how
    let lua = make_bare_engine().unwrap();
    install_runtime_state(&lua, Life::new());
    lua.globals()
        .set(
            "THREAD",
            modules::build(&lua, "std:thread").unwrap().unwrap(),
        )
        .unwrap();
    let refused: (Option<String>, Option<String>) = lua
        .load(
            r#"local w, e = THREAD.spawn { source = "return (pcall(require, 'std:workers'))" }
               return tostring(w), e"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(refused.0.as_deref(), Some("nil"), "no worker was started");
    let message = refused.1.unwrap();
    assert!(
        message.starts_with("thread.spawn:") && message.contains("set_worker_engine"),
        "{message}"
    );
    set_worker_engine(
        &lua,
        Arc::new(|life: Life| {
            let lua = make_bare_engine()?;
            install_runtime_state(&lua, life);
            modules::install_allowed(&lua, &["lib:json", "std:thread"])?;
            Ok(lua)
        }),
    );
    let started: String = lua
        .load(
            r#"local w = THREAD.spawn { source = "return tostring((pcall(require, 'std:workers')))" }
               return w:await()"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(
        started, "false",
        "the embedder's worker engine is what the worker gets"
    );

    // with no policy a worker gets everything, as on `main`
    let lua = make_engine().unwrap();
    let out: String = lua.load(WORKER_PROBE).eval_async().await.unwrap();
    assert_eq!(
        out,
        "std:workers=true,lib:compile=true,lib:test=true,lib:json=true"
    );
}
