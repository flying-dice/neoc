#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The sandbox: what an app cannot do, and what happens when it tries; unload and hot reload; one call at a time.

mod common;

use common::{agent_ctx, db, load, package, try_load, try_load_with};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use vos_lua::{reload, VmError};

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
        hook("probe", function() counter = counter + 1; return counter end)
        hook("report", function() return out end)
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
async fn errors_carry_the_app_file_and_line_and_never_crash_the_host() {
    let src = "local x = 1\nhook('boom', function()\n  local y = nil\n  return y.field\nend)\nhook('err', function() error('custom failure') end)\nhook('tbl', function() error({ code = 7 }) end)";
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
        "hook('bad', function()\n  local rows = app.db:all('select * from nope')\n  return rows\nend)\n\
               hook('tail', function()\n  return app.db:all('select * from nope')\nend)";
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
        std::fs::write(dir.join("app.lua"), format!("hook('v', function() return {n} end)")).unwrap();
    };
    write(1);
    let mut w = reload::watch_dir(&dir).unwrap();
    // every VM of the app holds the database through its closures: the count says whether one is still alive
    let data = db();
    let load_now = || async {
        let src = std::fs::read_to_string(dir.join("app.lua")).unwrap();
        try_load_with("hot", &src, data.clone()).await.unwrap()
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
