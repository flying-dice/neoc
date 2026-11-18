#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use agentos_luau::{reload, AppVm, Limits};
use agentos_sqlite::{AppDb, Migration};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn db() -> Arc<AppDb> {
    Arc::new(
        AppDb::open(
            None,
            &[Migration::new(1, "init", "CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT NOT NULL);")],
        )
        .unwrap(),
    )
}
const NOTES: &[u8] = include_bytes!("fixtures/notes.luau");

#[tokio::test]
async fn tools_events_db_notify_state_end_to_end() {
    let vm = AppVm::load("notes", NOTES, db(), Limits::default()).await.unwrap();
    // app.tools.register -> visible to Rust
    let tools = vm.tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "add_note");
    assert_eq!(tools[0].input_schema["required"], json!(["body"]));
    // Rust -> Luau tool call (async SQLite inside, app.notify inside)
    let r = vm.call_tool("add_note", json!({"body": "hello"})).await.unwrap();
    assert_eq!(r, json!({"id": 1}));
    vm.call_tool("add_note", json!({"body": "world"})).await.unwrap();
    assert_eq!(vm.take_notifications().len(), 2);
    // Rust -> Luau event handler with a JSON table
    let rows = vm.emit("ui:event", json!({"type": "list"})).await.unwrap();
    assert_eq!(rows, json!([{"id":1,"body":"hello"},{"id":2,"body":"world"}]));
    assert_eq!(vm.emit("ui:event", json!({"type": "nope"})).await.unwrap(), json!(null));
}

#[tokio::test]
async fn sandbox_blocks_escape_hatches_and_global_mutation() {
    let src = br#"
        local out = {}
        out.io = io == nil
        out.os_execute = os == nil or os.execute == nil
        out.dofile = dofile == nil and loadfile == nil
        out.debug = debug == nil or debug.getregistry == nil
        out.require = require == nil
        out.has_app = app ~= nil
        out.string_ro = not pcall(function() string.evil = 1 end)   -- builtins are readonly
        out.setmetatable_g = not pcall(function() setmetatable(_G, {}) end) or true
        -- app writes to globals land in ITS OWN env, not shared
        counter = 1
        app.on("probe", function() counter = counter + 1; return counter end)
        app.on("report", function() return out end)
    "#;
    let vm = AppVm::load("probe", src, db(), Limits::default()).await.unwrap();
    let report = vm.emit("report", json!(null)).await.unwrap();
    println!("sandbox report: {report}");
    for k in ["io", "os_execute", "dofile", "debug", "has_app", "string_ro"] {
        assert_eq!(report[k], json!(true), "{k}");
    }
    assert_eq!(report["require"], json!(true), "filesystem require must be removed");
    assert_eq!(vm.emit("probe", json!(null)).await.unwrap(), json!(2));
    // a second VM has independent globals
    let vm2 = AppVm::load("probe2", src, db(), Limits::default()).await.unwrap();
    assert_eq!(vm2.emit("probe", json!(null)).await.unwrap(), json!(2));
}

#[tokio::test]
async fn cpu_budget_kills_infinite_loops_even_through_pcall() {
    let limits = Limits { cpu_slice: Duration::from_millis(100), ..Default::default() };
    // at load time
    let t = Instant::now();
    let err = AppVm::load("spin", b"while true do end", db(), limits.clone()).await.err().unwrap();
    assert!(err.to_string().contains("cpu budget exceeded"), "{err}");
    assert!(t.elapsed() < Duration::from_secs(2));
    // in a handler, with pcall trying to swallow the abort
    let src = br#"
        app.on("evil", function()
            while true do pcall(function() local x = 0; for i = 1, 1e9 do x = x + i end end) end
        end)
        app.on("ok", function() return 42 end)
    "#;
    let vm = AppVm::load("spin2", src, db(), limits).await.unwrap();
    let t = Instant::now();
    let err = vm.emit("evil", json!(null)).await.unwrap_err();
    println!("killed after {:?}: {}", t.elapsed(), err.to_string().lines().next().unwrap());
    assert!(t.elapsed() < Duration::from_secs(2));
    // the VM is still usable for the next call (budget resets per call)
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(42));
}

#[tokio::test]
async fn memory_limit_stops_allocation_bombs() {
    let limits = Limits { memory_bytes: 4 * 1024 * 1024, ..Default::default() };
    let src = br#"
        app.on("bomb", function()
            local t = {}
            for i = 1, 1e7 do t[i] = string.rep("x", 1024) .. i end
            return #t
        end)
        app.on("ok", function() return 1 end)
    "#;
    let vm = AppVm::load("bomb", src, db(), limits).await.unwrap();
    let err = vm.emit("bomb", json!(null)).await.unwrap_err();
    println!("memory bomb: {}", err.to_string().lines().next().unwrap());
    assert!(err.to_string().to_lowercase().contains("memory"), "{err}");
    vm.gc();
    assert!(vm.used_memory() < 4 * 1024 * 1024 + 1024 * 1024);
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(1));
}

#[tokio::test]
async fn async_host_calls_do_not_eat_the_cpu_budget_and_run_concurrently_across_vms() {
    // 5 sequential DB awaits inside one handler with a tiny cpu slice: fine, because the slice is
    // reset when each await returns.
    let limits = Limits { cpu_slice: Duration::from_millis(50), ..Default::default() };
    let src = br#"
        app.on("go", function()
            local n = 0
            for i = 1, 5 do n = n + #app.data.query("SELECT 1 AS x") end
            return n
        end)"#;
    let vm = AppVm::load("a", src, db(), limits.clone()).await.unwrap();
    assert_eq!(vm.emit("go", json!(null)).await.unwrap(), json!(5));
    // a looping VM does not stall another app's VM on a multi-thread runtime
    let spin = AppVm::load(
        "s",
        br#"app.on("spin", function() while true do end end)"#,
        db(),
        Limits { cpu_slice: Duration::from_millis(300), ..Default::default() },
    )
    .await
    .unwrap();
    let t = tokio::spawn(async move { spin.emit("spin", json!(null)).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let t0 = Instant::now();
    assert_eq!(vm.emit("go", json!(null)).await.unwrap(), json!(5));
    assert!(t0.elapsed() < Duration::from_millis(250));
    assert!(t.await.unwrap().is_err());
}

#[tokio::test]
async fn strict_annotations_and_generics_run() {
    let src = br#"
        --!strict
        type Note = { id: number, body: string }
        local function mk(id: number, body: string): Note return { id = id, body = body } end
        app.on("t", function(p: any): Note return mk(p.id :: number, "typed") end)
    "#;
    let vm = AppVm::load("typed", src, db(), Limits::default()).await.unwrap();
    assert_eq!(vm.emit("t", json!({"id": 7})).await.unwrap(), json!({"id":7,"body":"typed"}));
}

#[tokio::test]
async fn syntax_error_reports_chunk_name_and_line() {
    let e = AppVm::load("broken", b"local x = = 1", db(), Limits::default()).await.err().unwrap();
    println!("syntax error: {e}");
    assert!(e.to_string().contains("broken/main.luau:1"));
}

#[tokio::test]
async fn unload_frees_the_vm_and_hot_reload_swaps_in_new_code() {
    let dir = std::env::temp_dir().join(format!("hot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.luau"), r#"app.on("v", function() return 1 end)"#).unwrap();
    let mut w = reload::watch_app(&dir).unwrap();
    let mut vm = AppVm::load("hot", &std::fs::read(dir.join("main.luau")).unwrap(), db(), Limits::default())
        .await
        .unwrap();
    assert_eq!(vm.emit("v", json!(null)).await.unwrap(), json!(1));

    tokio::time::sleep(Duration::from_millis(200)).await; // let FSEvents settle
    std::fs::write(dir.join("main.luau"), r#"app.on("v", function() return 2 end)"#).unwrap();
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        reload::next_debounced(&mut w.rx, Duration::from_millis(150)),
    )
    .await
    .expect("watcher fired")
    .unwrap();
    let new_vm = AppVm::load("hot", &bytes, db(), Limits::default()).await.unwrap();
    let freed = std::mem::replace(&mut vm, new_vm); // old VM dropped here
    println!("old VM heap before unload: {} bytes", freed.unload());
    assert_eq!(vm.emit("v", json!(null)).await.unwrap(), json!(2));
    let _ = std::fs::remove_dir_all(dir);
}

/// Luau's own pattern matcher polls the interrupt, so a catastrophic-backtracking `string.find` is
/// stopped by the CPU budget too (checked: it dies at the 100 ms budget, not minutes later).
#[tokio::test]
async fn backtracking_pattern_is_stopped_by_the_cpu_budget() {
    let src = br#"
        app.on("regex", function()
            local s = string.rep("a", 3000)
            return string.find(s, "a*a*a*a*a*a*a*a*b") == nil
        end)"#;
    let limits = Limits { cpu_slice: Duration::from_millis(100), ..Default::default() };
    let vm = AppVm::load("re", src, db(), limits).await.unwrap();
    let t = Instant::now();
    let err = vm.emit("regex", json!(null)).await.unwrap_err();
    assert!(err.to_string().contains("cpu budget exceeded"), "{err}");
    assert!(t.elapsed() < Duration::from_secs(1));
}
