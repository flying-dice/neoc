#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The sandbox and its limits: what an app cannot do, and what happens when it tries.

mod common;

use agentos_luau::{reload, VmError};
use common::{agent_ctx, db, load, package, quick, try_load, try_load_with};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// A small CPU budget: 200 000 ticks of 100 VM instructions each, a fraction of a second of an idle machine.
const QUICK_SLICE: u64 = 200_000;
/// "Killed at the budget, not by the backstop": far below the 10 s wall-clock backstop even on a busy machine.
const PROMPTLY: Duration = Duration::from_secs(5);

#[tokio::test]
async fn sandbox_blocks_escape_hatches_and_global_mutation() {
    let src = r#"
        local out = {}
        out.io = io == nil
        out.os_execute = os == nil or os.execute == nil
        out.dofile = dofile == nil and loadfile == nil
        out.debug = debug == nil or debug.getregistry == nil
        out.fs_require = not pcall(require, "/etc/passwd")
        out.has_app = app ~= nil
        out.string_ro = not pcall(function() string.evil = 1 end)   -- builtins are readonly
        -- app writes to globals land in ITS OWN env, not shared
        counter = 1
        app.on("probe", function() counter = counter + 1; return counter end)
        app.on("report", function() return out end)
    "#;
    let vm = load(src).await;
    let report = vm.emit("report", json!(null)).await.unwrap();
    for k in ["io", "os_execute", "dofile", "debug", "fs_require", "has_app", "string_ro"] {
        assert_eq!(report[k], json!(true), "{k}");
    }
    assert_eq!(vm.emit("probe", json!(null)).await.unwrap(), json!(2));
    // a second VM has independent globals
    let vm2 = try_load("probe2", src).await.unwrap();
    assert_eq!(vm2.emit("probe", json!(null)).await.unwrap(), json!(2));
}

#[tokio::test]
async fn cpu_budget_kills_infinite_loops_even_through_pcall() {
    let t = Instant::now();
    let err = try_load_with("spin", "while true do end", db(), quick(QUICK_SLICE)).await.err().unwrap();
    assert!(err.to_string().contains("cpu budget exceeded"), "{err}");
    assert!(t.elapsed() < PROMPTLY);
    // in a handler, with pcall trying to swallow the abort
    let src = r#"
        app.on("evil", function()
            while true do pcall(function() local x = 0; for i = 1, 1e9 do x = x + i end end) end
        end)
        app.on("ok", function() return 42 end)
    "#;
    let vm = try_load_with("spin2", src, db(), quick(QUICK_SLICE)).await.unwrap();
    let t = Instant::now();
    let err = vm.emit("evil", json!(null)).await.unwrap_err();
    assert!(matches!(err, VmError::Script(_)));
    assert!(t.elapsed() < PROMPTLY);
    // the VM is still usable for the next call (budget resets per call)
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(42));
}

#[tokio::test]
async fn memory_limit_stops_allocation_bombs() {
    let limits = agentos_luau::Limits { memory_bytes: 4 * 1024 * 1024, ..Default::default() };
    let src = r#"
        app.on("bomb", function()
            local t = {}
            for i = 1, 1e7 do t[i] = string.rep("x", 1024) .. i end
            return #t
        end)
        app.on("ok", function() return 1 end)
    "#;
    let vm = try_load_with("bomb", src, db(), limits).await.unwrap();
    let err = vm.emit("bomb", json!(null)).await.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("memory"), "{err}");
    vm.gc();
    assert!(vm.used_memory() < 4 * 1024 * 1024 + 1024 * 1024);
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_awaits_do_not_eat_the_cpu_budget_and_a_spinning_vm_does_not_stall_another() {
    // 5 sequential DB awaits inside one handler with a tiny cpu slice: fine, the slice resets per await
    let src = r#"
        app.on("go", function()
            local n = 0
            for i = 1, 5 do n = n + #app.db:all("SELECT 1 AS x") end
            return n
        end)"#;
    let vm = try_load_with("aaa", src, db(), quick(5_000)).await.unwrap();
    assert_eq!(vm.emit("go", json!(null)).await.unwrap(), json!(5));
    // a second VM spins for a good while (a million ticks) on another worker thread
    let spin =
        try_load_with("spin", r#"app.on("spin", function() while true do end end)"#, db(), quick(1_000_000))
            .await
            .unwrap();
    let spinner = tokio::spawn(async move { spin.emit("spin", json!(null)).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(vm.emit("go", json!(null)).await.unwrap(), json!(5));
    assert!(!spinner.is_finished(), "the other VM was still spinning while this one answered");
    assert!(spinner.await.unwrap().is_err());
}

/// Busy OS threads, twice as many as cores: the machine is as loaded as a parallel test run makes it.
struct Load {
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Load {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cores = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        let threads = (0..cores * 2)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::hint::spin_loop();
                    }
                })
            })
            .collect();
        Self { stop, threads }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// The budget counts ticks, not wall-clock time: an honest call stays inside it however loaded the
/// machine is, and a runaway one is still killed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cpu_budget_is_not_tripped_by_a_loaded_host() {
    let src = r#"
        app.on("honest", function() local n = 0 for i = 1, 1e6 do n = n + 1 end return n end)
        app.on("runaway", function() local n = 0 for i = 1, 1e9 do n = n + 1 end return n end)"#;
    let vm = try_load_with("loaded", src, db(), quick(300_000)).await.unwrap();
    let _load = Load::start();
    for _ in 0..5 {
        assert_eq!(vm.emit("honest", json!(null)).await.unwrap(), json!(1_000_000));
        tokio::time::sleep(Duration::from_millis(40)).await; // idle time between calls costs nothing either
    }
    let err = vm.emit("runaway", json!(null)).await.unwrap_err();
    assert!(err.to_string().contains("cpu budget exceeded"), "{err}");
}

#[tokio::test]
async fn the_wall_clock_backstop_still_ends_a_call_that_runs_too_long() {
    let limits = agentos_luau::Limits {
        cpu_ticks: u64::MAX,
        cpu_wall: Duration::from_millis(200),
        ..Default::default()
    };
    let err = try_load_with("slow", "while true do end", db(), limits).await.err().unwrap();
    assert!(err.to_string().contains("cpu budget exceeded"), "{err}");
}

/// The C pattern matcher never polls the VM hook; the host's own matcher charges the CPU budget, so a
/// catastrophic-backtracking `string.find` is stopped by it too.
#[tokio::test]
async fn backtracking_pattern_is_stopped_by_the_cpu_budget() {
    let src = r#"
        app.on("regex", function()
            local s = string.rep("a", 3000)
            return string.find(s, "a*a*a*a*a*a*a*a*b") == nil
        end)"#;
    let vm = try_load_with("rex", src, db(), quick(QUICK_SLICE)).await.unwrap();
    let t = Instant::now();
    let err = vm.emit("regex", json!(null)).await.unwrap_err();
    assert!(err.to_string().contains("cpu budget exceeded"), "{err}");
    assert!(t.elapsed() < PROMPTLY);
}

#[tokio::test]
async fn a_blocking_host_call_has_its_own_timeout() {
    // the app asks the database for something that never ends: SQLite's own interrupt (adapter) or the
    // host-call backstop (here) ends it, and the VM stays usable
    let limits = agentos_luau::Limits { host_call_timeout: Duration::from_millis(300), ..Default::default() };
    let src = r#"
        app.on("spin", function()
            return app.db:all("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c")
        end)
        app.on("ok", function() return 1 end)"#;
    let vm = try_load_with("slow", src, db(), limits).await.unwrap();
    let t = Instant::now();
    let err = vm.emit("spin", json!(null)).await.unwrap_err();
    assert!(err.to_string().contains("abandoned") || err.to_string().contains("interrupted"), "{err}");
    assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(1));
}

#[tokio::test]
async fn errors_carry_the_app_file_and_line_and_never_crash_the_host() {
    let src = "local x = 1\napp.on('boom', function()\n  local y = nil\n  return y.field\nend)\napp.on('err', function() error('custom failure') end)\napp.on('tbl', function() error({ code = 7 }) end)";
    let vm = try_load("broken", src).await.unwrap();
    let e = vm.emit("boom", json!(null)).await.unwrap_err().to_string();
    assert!(e.starts_with("broken/app.lua:4:"), "{e}");
    let e = vm.emit("err", json!(null)).await.unwrap_err().to_string();
    assert_eq!(e, "broken/app.lua:6: custom failure");
    assert!(vm.emit("tbl", json!(null)).await.is_err(), "a table error is an error, not a panic");

    // a syntax error names its line, as a load failure
    let e = try_load("broken", "local x = = 1").await.err().unwrap();
    assert!(matches!(e, VmError::Load { .. }));
    assert!(e.to_string().contains("broken/app.lua:1"), "{e}");
    // and a runtime error while loading
    let e = try_load("broken", "local t = nil\nlocal v = t.x").await.err().unwrap();
    assert!(e.to_string().contains("broken/app.lua:2"), "{e}");
}

#[tokio::test]
async fn host_function_errors_get_the_calling_line() {
    let src =
        "app.on('bad', function()\n  local rows = app.db:all('select * from nope')\n  return rows\nend)\n\
               app.on('tail', function()\n  return app.db:all('select * from nope')\nend)";
    let vm = try_load("err", src).await.unwrap();
    let e = vm.emit("bad", json!(null)).await.unwrap_err().to_string();
    assert!(e.starts_with("err/app.lua:2:"), "{e}");
    assert!(e.contains("no such table"), "{e}");
    // Lua 5.1 drops the frame of a function that tail-calls (`return f()`): the line is lost, the cause is not
    let e = vm.emit("tail", json!(null)).await.unwrap_err().to_string();
    assert!(e.contains("no such table"), "{e}");
}

#[tokio::test]
async fn unload_frees_the_vm_and_hot_reload_swaps_in_new_code() {
    let dir = std::env::temp_dir().join(format!("hot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write = |n: u32| {
        std::fs::write(dir.join("app.lua"), format!("app.on('v', function() return {n} end)")).unwrap();
    };
    write(1);
    let mut w = reload::watch_dir(&dir).unwrap();
    // every VM of the app holds the database through its closures: the count says whether one is still alive
    let data = db();
    let load_now = || async {
        let src = std::fs::read_to_string(dir.join("app.lua")).unwrap();
        try_load_with("hot", &src, data.clone(), agentos_luau::Limits::default()).await.unwrap()
    };
    let mut vm = load_now().await;
    assert_eq!(vm.emit("v", json!(null)).await.unwrap(), json!(1));

    tokio::time::sleep(Duration::from_millis(200)).await; // let FSEvents settle
    write(2);
    tokio::time::timeout(Duration::from_secs(5), reload::next_change(&mut w.rx, Duration::from_millis(150)))
        .await
        .expect("watcher fired");
    let old = std::mem::replace(&mut vm, load_now().await);
    assert_eq!(Arc::strong_count(&data), 3, "the test, the old VM and the new one hold the database");
    assert!(old.unload() > 0);
    assert_eq!(
        Arc::strong_count(&data),
        2,
        "the unloaded VM is freed: nothing of it still holds the database"
    );
    assert_eq!(vm.emit("v", json!(null)).await.unwrap(), json!(2));
    drop(vm);
    assert_eq!(Arc::strong_count(&data), 1, "and so is a VM that is only dropped");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn one_call_runs_at_a_time_in_a_vm() {
    let src = r#"
        local order = {}
        app.tools.register { name = "slow", run = function()
            table.insert(order, "a-start")
            app.db:all("select 1")      -- yields to the host
            table.insert(order, "a-end")
            return order
        end }
        app.tools.register { name = "fast", run = function()
            table.insert(order, "b")
            return order
        end }
    "#;
    let vm = std::sync::Arc::new(load(src).await);
    let (a, b) = (vm.clone(), vm.clone());
    let first = tokio::spawn(async move { a.call_tool("slow", json!({}), &agent_ctx(0), false).await });
    let second = tokio::spawn(async move { b.call_tool("fast", json!({}), &agent_ctx(0), false).await });
    let (first, second) = (first.await.unwrap().unwrap(), second.await.unwrap().unwrap());
    // whichever ran first ran to completion before the other started
    let a_done = first == json!(["a-start", "a-end"]) || second == json!(["a-start", "a-end", "b"]);
    let b_done = second == json!(["b"])
        || first == json!(["b", "a-start", "a-end"])
        || second == json!(["b", "a-start", "a-end"]);
    assert!(a_done || b_done, "{first} / {second}");
    let _ = package("", &[]);
}
