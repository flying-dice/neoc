#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The invariants of a call into the VM, each with the script that broke it. Lua 5.1 cannot
//! yield across a `string.gsub` callback, a sort comparator, a metamethod or an iterator, so a host call made
//! there fails and the script, which may catch the error, goes on; and a script may resume coroutines, yield,
//! and start tasks. None of that may leave a call half-done:
//!
//! 1. a call lends the VM only when it really suspended at an idle wait (not at a stale request, not at a plain
//!    `coroutine.yield()`), and never while it has a `db:tx` open;
//! 2. a transaction is committed when `db:tx` says it succeeded, and belongs to the call that opened it;
//! 3. a host call Lua could not suspend at holds nothing once the script goes on: no database lock.
//!
//! `chaos.rs` generates scripts that combine these ingredients and checks the same invariants over all of them.

mod common;

use common::{agent_ctx, db, load, try_load_with};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

async fn call(vm: &Arc<agentos_lua::AppVm>, tool: &str) -> Value {
    match tokio::time::timeout(Duration::from_secs(10), vm.call_tool(tool, json!({}), &agent_ctx(0), false))
        .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => panic!("{tool} failed: {e}"),
        Err(late) => panic!("{tool} never finished ({late}): the app is wedged"),
    }
}

fn spawn_call(vm: &Arc<agentos_lua::AppVm>, tool: &'static str) -> tokio::task::JoinHandle<Value> {
    let vm = vm.clone();
    tokio::spawn(async move { call(&vm, tool).await })
}

/// A database call a database call Lua cannot suspend at must not keep the database's write gate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_database_call_lua_could_not_suspend_at_does_not_hold_the_write_gate() {
    let data = db();
    let vm = Arc::new(
        try_load_with(
            "gate",
            r#"
            app.tool { name = "g", run = function()
              local ok, e = pcall(string.gsub, "a", "a", function() return tostring(app.db:one("select 1 as one").one) end)
              local two = app.db:one("select 2 as two").two
              return { ok = ok, e = tostring(e), two = two }
            end }
            app.tool { name = "g1", run = function()
              local ok = pcall(string.gsub, "a", "a", function() return tostring(app.db:one("select 1 as one").one) end)
              return { ok = ok }
            end }
            app.tool { name = "plain", run = function() return app.db:one("select 3 as three").three end }
            "#,
            data.clone(),
        )
        .await
        .unwrap(),
    );
    let out = call(&vm, "g").await;
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(out["two"], 2, "the next statement of the same call ran: {out}");
    // the call ends right after the failed statement: nothing is left holding the gate
    assert_eq!(call(&vm, "g1").await["ok"], false);
    assert_eq!(call(&vm, "plain").await, 3);
    tokio::time::timeout(Duration::from_secs(3), data.query("select 1".into(), vec![]))
        .await
        .expect("a window query (it does not go through the VM) is not stuck behind the dead statement")
        .unwrap();
}

/// A transaction `db:tx` reports success only when its transaction committed, whatever coroutines did inside the body.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transaction_that_reports_success_committed_what_the_body_started() {
    let vm = Arc::new(
        load(
            r#"
            app.db:migrate { [1] = "create table t(a)" }
            app.tool { name = "txloss", run = function()
              local ok, err = pcall(function()
                return app.db:tx(function()
                  local co = coroutine.create(function() app.db:exec("insert into t values (1)") end)
                  pcall(string.gsub, "a", "a", function() coroutine.resume(co) end)
                  return "body done"
                end)
              end)
              return { ok = ok, ret = tostring(err) }
            end }
            app.tool { name = "txloss2", run = function()
              local ok, err = pcall(function()
                return app.db:tx(function()
                  local c = coroutine.create(function() app.db:exec("insert into t values (2)") end)
                  pcall(string.gsub, "a", "a", function() coroutine.resume(c) end)
                  app.db:exec("insert into t values (3)")
                  return "body done"
                end)
              end)
              return { ok = ok, ret = tostring(err) }
            end }
            app.tool { name = "rows", readOnly = true, run = function() return app.db:all("select a from t order by a") end }
            "#,
        )
        .await,
    );
    let out = call(&vm, "txloss").await;
    let rows = call(&vm, "rows").await;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(rows, json!([{ "a": 1 }]), "success means the statement the body started committed");
    let out = call(&vm, "txloss2").await;
    let rows = call(&vm, "rows").await;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(rows, json!([{ "a": 1 }, { "a": 2 }, { "a": 3 }]));
}

/// A transaction body a transaction body continued by another call (a coroutine started in one tool, resumed in the next) never
/// leaves half of it behind, and never reports success for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transaction_body_continued_by_another_call_is_all_or_nothing() {
    let vm = Arc::new(
        load(
            r#"
            app.db:migrate { [1] = "create table t(a)" }
            local co
            local stage = 0
            app.tool { name = "a", run = function()
              co = coroutine.create(function()
                return app.db:tx(function()
                  app.db:exec("insert into t values (1)")
                  stage = 1
                  local _ = app.db:scalar("select count(*) from t")
                  app.db:exec("insert into t values (2)")
                  return "tx body done"
                end)
              end)
              local tries = 0
              repeat
                tries = tries + 1
                pcall(string.gsub, "a", "a", function() coroutine.resume(co) end)
                if stage == 0 then app.db:one("select 1 as x") end
              until stage == 1 or tries > 50
              return { stage = stage, status = coroutine.status(co) }
            end }
            app.tool { name = "b", run = function()
              local r = { coroutine.resume(co) }
              return { ok = r[1], v = tostring(r[2]), status = coroutine.status(co) }
            end }
            app.tool { name = "rows", readOnly = true, run = function() return app.db:all("select a from t order by a") end }
            "#,
        )
        .await,
    );
    call(&vm, "a").await;
    let b = call(&vm, "b").await;
    let rows = call(&vm, "rows").await;
    let all = rows == json!([{ "a": 1 }, { "a": 2 }]);
    assert!(rows == json!([]) || all, "a transaction is all or nothing, got {rows}");
    assert!(all || b["v"] != json!("tx body done"), "no success without the commit: {b} {rows}");
}

/// A coroutine a coroutine that is handed to `task.spawn` belongs to the scheduler: the script resuming it too (two drivers
/// of one host call) gets Lua's "cannot resume non-suspended coroutine" instead of stealing the call, and no other
/// call is held up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coroutine_with_two_drivers_does_not_hold_up_another_call() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local co
            app.tool { name = "a", run = function()
              co = coroutine.create(function()
                app.db:one("select 1 as x")
                task.wait(0.5)
                return "co done"
              end)
              task.spawn(co)
              local r = { coroutine.resume(co) }
              return { ok = r[1], v = tostring(r[2]) }
            end }
            app.tool { name = "b", run = function() return "b ran" end }
            "#,
        )
        .await,
    );
    let a = spawn_call(&vm, "a");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = std::time::Instant::now();
    assert_eq!(call(&vm, "b").await, json!("b ran"));
    assert!(started.elapsed() < Duration::from_millis(300), "b was held up: {:?}", started.elapsed());
    let a = a.await.unwrap();
    assert_eq!(a["ok"], false, "{a}");
    assert!(a["v"].as_str().unwrap().contains("non-suspended"), "{a}");
}

/// A failed wait a failed wait, then a plain `coroutine.yield()` at the top of a read-only call: the call still holds the
/// VM, so nothing else runs inside it and it stays read-only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plain_yield_after_a_failed_wait_does_not_lend_a_read_only_call() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local state = app.state { v = 0 }
            local counter = 0
            app.tool { name = "bump", run = function() counter = counter + 1 return counter end }
            app.tool { name = "ro", readOnly = true, run = function()
              local failed = not pcall(table.sort, { 3, 2, 1 }, function(a, b) task.wait(0) return a < b end)
              local before = counter
              for _ = 1, 200 do coroutine.yield() end
              local ok, e = pcall(function() state.v = 42 end)
              local nok = pcall(function() app.notify { title = "from readOnly" } end)
              return { failed_wait = failed, state_write_ok = ok, e = tostring(e), notify_ok = nok, interleaved = counter ~= before }
            end }
            app.tool { name = "v", readOnly = true, run = function() return state.v end }
            "#,
        )
        .await,
    );
    let ro = spawn_call(&vm, "ro");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let bump = spawn_call(&vm, "bump");
    let out = ro.await.unwrap();
    bump.await.unwrap();
    assert_eq!(out["failed_wait"], true, "{out}");
    assert_eq!(out["state_write_ok"], false, "{out}");
    assert_eq!(out["notify_ok"], false, "{out}");
    assert_eq!(out["interleaved"], false, "{out}");
    assert_eq!(call(&vm, "v").await, json!(0));
    assert!(vm.take_effects().is_empty(), "nothing was queued");
}

/// A wait a wait started outside `db:tx` (Lua could not suspend at it) and resumed inside the body must not lend the
/// VM with the transaction open, and another call's write must not land in the transaction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wait_resumed_inside_a_transaction_does_not_lend_it() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            app.db:migrate { [1] = "create table t(a)" }
            app.tool { name = "txw", run = function()
              local co = coroutine.create(function() task.wait(0.3) end)
              pcall(string.gsub, "x", "x", function() coroutine.resume(co) end)
              local ok, e = pcall(function()
                app.db:tx(function()
                  app.db:exec("insert into t values (1)")
                  coroutine.resume(co)
                  error("abort the transaction")
                end)
              end)
              return { ok = ok, e = tostring(e) }
            end }
            app.tool { name = "other", run = function() return app.db:exec("insert into t values (99)").changes end }
            app.tool { name = "rows", readOnly = true, run = function() return app.db:all("select a from t order by a") end }
            "#,
        )
        .await,
    );
    let txw = spawn_call(&vm, "txw");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let other = call(&vm, "other").await;
    let txw = txw.await.unwrap();
    assert_eq!(txw["ok"], false, "{txw}");
    assert_eq!(other, 1, "the other call's write was acknowledged");
    assert_eq!(
        call(&vm, "rows").await,
        json!([{ "a": 99 }]),
        "the acknowledged write is there and the aborted transaction's is not"
    );
}

/// A coroutine whose host call is in flight is running as far as the script can tell: resuming it from somewhere
/// else is Lua's own error, instead of stealing the call from the thread waiting for it (which, with the turn
/// held for a call that only the thief can finish, wedged the app).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coroutine_whose_host_call_is_in_flight_cannot_be_resumed_from_elsewhere() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local co, wrapped
            app.tool { name = "steal", run = function()
              task.spawn(function()
                co = coroutine.create(function() app.db:one("select 1 as x") task.wait(0.05) return "finished" end)
                coroutine.resume(co)
              end)
              task.spawn(function()
                wrapped = coroutine.wrap(function() task.wait(0.05) return "finished" end)
                wrapped()
              end)
              local ok, e = coroutine.resume(co)
              local wok, we = pcall(wrapped)
              return { ok = ok, e = tostring(e), wok = wok, we = tostring(we) }
            end }
            "#,
        )
        .await,
    );
    let out = call(&vm, "steal").await;
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["e"].as_str().unwrap().contains("non-suspended"), "{out}");
    assert_eq!(out["wok"], false, "{out}");
    assert!(out["we"].as_str().unwrap().contains("non-suspended"), "{out}");
}

/// Handing a coroutine to the scheduler while another thread is waiting at its host call is refused: it must not
/// take the coroutine over (the thread would then resume into a call the scheduler also polls).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_scheduler_does_not_take_a_coroutine_that_is_waiting_at_a_host_call() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local co
            app.tool { name = "start", run = function()
              task.spawn(function()
                co = coroutine.create(function() task.wait(0.1) return "finished" end)
                coroutine.resume(co)
              end)
              return "started"
            end }
            app.tool { name = "adopt", run = function()
              local ok, e = pcall(task.spawn, co)
              return { ok = ok, e = tostring(e) }
            end }
            "#,
        )
        .await,
    );
    call(&vm, "start").await;
    let out = call(&vm, "adopt").await;
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["e"].as_str().unwrap().contains("already running"), "{out}");
}
