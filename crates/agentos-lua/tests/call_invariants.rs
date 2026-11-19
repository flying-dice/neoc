#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The invariants of a call into the VM, each with the script that broke it. Lua 5.1 cannot
//! yield across a `string.gsub` callback, a sort comparator, a metamethod or an iterator, so a host call made
//! there fails and the script, which may catch the error, goes on; and a script may resume coroutines, yield,
//! and start tasks. None of that may leave a call half-done:
//!
//! 1. a call lends the VM only when it really suspended at an idle wait (not at a stale request, not at a plain
//!    `coroutine.yield()`), and never while it has a `db:tx` open;
//! 2. a transaction is committed when `db:tx` says it succeeded, and belongs to the call that opened it;
//! 3. a host call Lua could not suspend at holds nothing once the script goes on: no database lock;
//! 4. a script resumes only the coroutines it made, and only while nobody else drives them: resuming any other thread
//!    (the top-level thread of a call, a task's thread, the protected coroutine of a `pcall` or `xpcall`) is refused
//!    and changes nothing, and script code never runs in a call that does not hold the turn.
//!
//! `chaos.rs` generates scripts that combine these ingredients and checks the same invariants over all of them.

mod common;

use common::{db, load, timed_tool, try_load_with};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

async fn call(vm: &Arc<agentos_lua::AppVm>, tool: &str) -> Value {
    timed_tool(vm, tool, 10).await.unwrap_or_else(|why| panic!("{tool}: {why}"))
}

fn spawn_call(vm: &Arc<agentos_lua::AppVm>, tool: &'static str) -> tokio::task::JoinHandle<Value> {
    let vm = vm.clone();
    tokio::spawn(async move { call(&vm, tool).await })
}

/// A database call that Lua cannot suspend at must not keep the database's write gate.
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

/// `db:tx` reports success only when its transaction committed, whatever coroutines did inside the body.
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

/// A transaction body continued by another call (a coroutine started in one tool, resumed in the next) never leaves
/// half of it behind, and never reports success for it.
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

/// A coroutine that is handed to `task.spawn` belongs to the scheduler: the script resuming it too (two drivers of one
/// host call) gets Lua's "cannot resume non-suspended coroutine" instead of stealing the call, and no other call is
/// held up.
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

/// A failed wait, then a plain `coroutine.yield()` at the top of a read-only call: the call still holds the VM, so
/// nothing else runs inside it and it stays read-only.
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

/// A wait started outside `db:tx` (Lua could not suspend at it) and resumed inside the body must not lend the VM with
/// the transaction open, and another call's write must not land in the transaction.
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

// ---- a thread the script did not make is not the script's to resume -------------------------------------------------------

/// A call that resumes the top-level thread of another call (a `coroutine.running()` the first stashed) while that
/// call is parked at a wait is refused, and the other call finishes as if nothing had happened.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_top_level_thread_of_a_parked_call_cannot_be_resumed_by_another_call() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local t
            app.tool { name = "a", run = function() t = coroutine.running() task.wait(0.2) return "a done" end }
            app.tool { name = "b", readOnly = true, run = function()
              local ok, e = coroutine.resume(t)
              local ok2, e2 = coroutine.resume(t, "with an argument")
              return { ok = ok, e = tostring(e), ok2 = ok2, e2 = tostring(e2) }
            end }
            app.tool { name = "ping", readOnly = true, run = function() return "pong" end }
            "#,
        )
        .await,
    );
    let a = spawn_call(&vm, "a");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let b = call(&vm, "b").await;
    assert_eq!(b["ok"], false, "{b}");
    assert_eq!(b["ok2"], false, "{b}");
    for e in [&b["e"], &b["e2"]] {
        assert!(e.as_str().unwrap().contains("non-suspended"), "{b}");
    }
    assert_eq!(a.await.unwrap(), json!("a done"), "the parked call was not disturbed");
    assert_eq!(call(&vm, "ping").await, json!("pong"));
}

/// A protected call's coroutine is a host thread like any other: another call that resumes the one a `pcall` runs its
/// body in (stashed by `coroutine.running()`) is refused, the parked call finishes with its own result, and a
/// `readOnly` call never writes or notifies.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_thread_of_a_pcall_cannot_be_resumed_by_another_call() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local state = app.state { v = 0 }
            local t
            local log = {}
            app.tool { name = "a", readOnly = true, run = function()
              local ok, e = pcall(function()
                t = coroutine.running()
                task.wait(0.3)
                local w = pcall(function() state.v = 42 end)
                local n = pcall(function() app.notify { title = "from readOnly" } end)
                log[#log + 1] = { state = w, notify = n }
              end)
              return { "a done", ok, tostring(e) }
            end }
            app.tool { name = "b", run = function()
              local ok, e = coroutine.resume(t)
              local ok2, e2 = coroutine.resume(t, "with an argument")
              return { ok = ok, e = tostring(e), ok2 = ok2, e2 = tostring(e2) }
            end }
            app.tool { name = "report", readOnly = true, run = function() return { v = state.v, log = log } end }
            app.tool { name = "ping", readOnly = true, run = function() return "pong" end }
            "#,
        )
        .await,
    );
    let a = spawn_call(&vm, "a");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let b = call(&vm, "b").await;
    for ok in ["ok", "ok2"] {
        assert_eq!(b[ok], false, "{b}");
    }
    for e in [&b["e"], &b["e2"]] {
        assert_eq!(e, "cannot resume non-suspended coroutine", "{b}");
    }
    assert_eq!(a.await.unwrap(), json!(["a done", true, "nil"]), "the parked call finished its own way");
    let report = call(&vm, "report").await;
    assert_eq!(report["v"], 0, "{report}");
    assert_eq!(report["log"], json!([{ "state": false, "notify": false }]), "{report}");
    assert!(vm.take_effects().is_empty(), "nothing was queued from a readOnly call");
    assert_eq!(call(&vm, "ping").await, json!("pong"));
}

/// The same for the protected coroutine of an `xpcall` inside a task: a task handle never fails to complete because
/// another call resumed the thread under it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_thread_of_an_xpcall_in_a_task_cannot_be_resumed_and_the_task_completes() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local T, h
            local steps = {}
            app.tool { name = "s", readOnly = true, run = function()
              h = task.spawn(function()
                local ok, e = xpcall(function()
                  T = coroutine.running()
                  task.wait(0.3)
                  steps[#steps + 1] = "after wait"
                  steps[#steps + 1] = tostring(pcall(function() app.notify { title = "x" } end))
                end, function(m) return m end)
                return { ok, tostring(e) }
              end)
              return "spawned"
            end }
            app.tool { name = "r", run = function()
              local ok, e = coroutine.resume(T)
              local ok2, e2 = coroutine.resume(T, "with an argument")
              return { ok = ok, e = tostring(e), ok2 = ok2, e2 = tostring(e2) }
            end }
            app.tool { name = "aw", run = function() return { h:await() } end }
            app.tool { name = "report", readOnly = true, run = function() return steps end }
            "#,
        )
        .await,
    );
    assert_eq!(call(&vm, "s").await, json!("spawned"));
    tokio::time::sleep(Duration::from_millis(30)).await;
    let r = call(&vm, "r").await;
    assert_eq!(
        r,
        json!({ "ok": false, "e": "cannot resume non-suspended coroutine", "ok2": false, "e2": "cannot resume non-suspended coroutine" })
    );
    assert_eq!(call(&vm, "aw").await, json!([[true, "nil"]]), "the task completed with its own result");
    assert_eq!(call(&vm, "report").await, json!(["after wait", "false"]), "readOnly came with the task");
    assert!(vm.take_effects().is_empty(), "nothing was queued from a readOnly task");
}

/// Any thread a script can reach with `coroutine.running()` that it did not make is refused, wherever the call was:
/// at the top of a tool, in a `pcall`, an `xpcall`, nested protected calls, a task, a `coroutine.wrap` body's caller.
/// A coroutine of its own (a `create`, or the thread inside a `wrap`) is the script's to resume.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_coroutines_the_script_made_can_be_resumed() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local seen = {}
            local function stash(name) seen[name] = coroutine.running() task.wait(0.2) return name end
            app.tool { name = "park", run = function()
              local out = {}
              out.top = stash("top")
              out.p = { pcall(function() return stash("pcall") end) }
              out.x = { xpcall(function() return stash("xpcall") end, function(m) return m end) }
              out.n = { pcall(function() return { pcall(function() return stash("nested") end) } end) }
              out.t = task.spawn(function() return stash("task") end):await()
              -- a coroutine of the script's own, waiting at a host call: refused while its resumer waits for it
              local co = coroutine.create(function() return stash("mine") end)
              out.mine = { coroutine.resume(co) }
              return out
            end }
            app.tool { name = "meddle", run = function()
              local out = {}
              for _, name in ipairs { "top", "pcall", "xpcall", "nested", "task", "mine" } do
                local t = seen[name]
                if t == nil then
                  out[name] = "never started"
                else
                  local ok, e = coroutine.resume(t)
                  local ok2, e2 = coroutine.resume(t, "with an argument")
                  out[name] = tostring(ok) .. ":" .. tostring(e) .. "|" .. tostring(ok2) .. ":" .. tostring(e2)
                end
              end
              return out
            end }
            "#,
        )
        .await,
    );
    let park = spawn_call(&vm, "park");
    // each thread waits 0.2s at its stash: ask about every one while it is parked (the first answer for a name is
    // given inside its own window, since the stash and the wait are one slice of the call)
    let names = ["top", "pcall", "xpcall", "nested", "task", "mine"];
    let mut said = std::collections::BTreeMap::<String, String>::new();
    let started = std::time::Instant::now();
    while said.len() < names.len() && started.elapsed() < Duration::from_secs(5) {
        let round = call(&vm, "meddle").await;
        for (name, v) in round.as_object().unwrap() {
            if v != "never started" {
                said.entry(name.clone()).or_insert_with(|| v.as_str().unwrap().to_owned());
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let park = park.await.unwrap();
    assert_eq!(park["top"], "top", "{park}");
    assert_eq!(park["p"], json!([true, "pcall"]), "{park}");
    assert_eq!(park["x"], json!([true, "xpcall"]), "{park}");
    assert_eq!(park["n"], json!([true, [true, "nested"]]), "{park}");
    assert_eq!(park["t"], "task", "{park}");
    assert_eq!(park["mine"], json!([true, "mine"]), "{park}");
    let answer = "false:cannot resume non-suspended coroutine|false:cannot resume non-suspended coroutine";
    for name in names {
        assert_eq!(said.get(name).map(String::as_str), Some(answer), "{name}: {said:?}");
    }
}

/// A coroutine the script made keeps working: it resumes, yields (across `pcall` and `xpcall`, nested in either
/// direction), waits at host calls, and its own thread (`coroutine.running()` inside it, inside a `wrap`) is its to
/// resume from anywhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coroutine_the_script_made_still_resumes_yields_and_waits_through_protected_calls() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local wrapped_self
            app.tool { name = "mine", run = function()
              local out = {}
              -- yield across pcall and xpcall inside a coroutine of the script's own
              local co = coroutine.create(function(a)
                local ok, v = pcall(function() local x = coroutine.yield(a + 1) return x * 2 end)
                local ok2, v2 = xpcall(function() return coroutine.yield("second") end, function(m) return m end)
                return ok, v, ok2, v2
              end)
              out.y1 = { coroutine.resume(co, 1) }
              out.y2 = { coroutine.resume(co, 10) }
              out.y3 = { coroutine.resume(co, "last") }
              out.dead = { coroutine.resume(co) }
              -- a coroutine inside pcall inside a coroutine inside pcall, waiting at a host call
              out.deep = { pcall(function()
                local outer = coroutine.create(function()
                  return pcall(function()
                    local inner = coroutine.wrap(function() task.wait(0.01) coroutine.yield("in") return "out" end)
                    return inner() .. inner()
                  end)
                end)
                return coroutine.resume(outer)
              end) }
              -- the thread inside a wrap is the script's own
              local gen = coroutine.wrap(function() wrapped_self = coroutine.running() coroutine.yield(1) return 2 end)
              out.gen = { gen() }
              out.resumed_wrapped = { coroutine.resume(wrapped_self) }
              -- an error inside is the resumer's to see
              out.err = { coroutine.resume(coroutine.create(function() error("boom", 0) end)) }
              return out
            end }
            app.tool { name = "again", run = function() return { coroutine.resume(wrapped_self) } end }
            "#,
        )
        .await,
    );
    let out = call(&vm, "mine").await;
    assert_eq!(out["y1"], json!([true, 2]), "{out}");
    assert_eq!(out["y2"], json!([true, "second"]), "{out}");
    assert_eq!(out["y3"], json!([true, true, 20, true, "last"]), "{out}");
    assert_eq!(out["dead"], json!([false, "cannot resume dead coroutine"]), "{out}");
    assert_eq!(out["deep"], json!([true, true, true, "inout"]), "{out}");
    assert_eq!(out["gen"], json!([1]), "{out}");
    assert_eq!(out["resumed_wrapped"], json!([true, 2]), "{out}");
    assert_eq!(out["err"], json!([false, "boom"]), "{out}");
    // the wrap's coroutine finished with the resume above: the next call finds it dead, as Lua says
    assert_eq!(call(&vm, "again").await, json!([false, "cannot resume dead coroutine"]));
}

/// `task.spawn`, `defer` and `delay` take only coroutines the script made: a thread it reached through
/// `coroutine.running()` is the host's, and handing it to the scheduler would run another call's code under the
/// scheduler. A coroutine of the script's own, once scheduled, is the scheduler's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_scheduler_takes_only_coroutines_the_script_made() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local T, mine
            app.tool { name = "park", run = function() T = coroutine.running() task.wait(0.3) return "parked" end }
            app.tool { name = "steal", run = function()
              local out = {}
              for _, kind in ipairs { "spawn", "defer", "delay" } do
                local ok, e = pcall(function() if kind == "delay" then task.delay(0, T) else task[kind](T) end end)
                out[kind] = { ok, tostring(e):match("not yours") or tostring(e) }
              end
              return out
            end }
            app.tool { name = "own", run = function()
              mine = coroutine.create(function() task.wait(0.1) return "mine done" end)
              local h = task.spawn(mine)
              local ok, e = coroutine.resume(mine)
              return { ok, tostring(e), h:await() }
            end }
            "#,
        )
        .await,
    );
    let parked = spawn_call(&vm, "park");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let steal = call(&vm, "steal").await;
    for kind in ["spawn", "defer", "delay"] {
        assert_eq!(steal[kind], json!([false, "not yours"]), "{kind}: {steal}");
    }
    assert_eq!(parked.await.unwrap(), json!("parked"), "the parked call was not disturbed");
    assert_eq!(
        call(&vm, "own").await,
        json!([false, "cannot resume non-suspended coroutine", "mine done"]),
        "a scheduled coroutine is the scheduler's"
    );
}

/// A `readOnly` call whose thread another call tried to resume stays read-only: it never runs without the turn, and
/// the restriction goes with the turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_only_call_is_still_read_only_after_another_call_tried_to_resume_it() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local state = app.state { v = 0 }
            local t
            local log = {}
            app.tool { name = "a", readOnly = true, run = function()
              t = coroutine.running()
              task.wait(0.02)
              coroutine.yield()
              local ok = pcall(function() state.v = 42 end)
              local notified = pcall(function() app.notify { title = "from readOnly" } end)
              log[#log + 1] = { state = ok, notify = notified }
              return "a done"
            end }
            app.tool { name = "b", run = function()
              local x = 0
              for i = 1, 5000000 do x = x + i end
              local ok, e = coroutine.resume(t)
              task.wait(0.2)
              return { ok = ok, e = tostring(e) }
            end }
            app.tool { name = "report", readOnly = true, run = function() return { v = state.v, log = log } end }
            "#,
        )
        .await,
    );
    let a = spawn_call(&vm, "a");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let b = call(&vm, "b").await;
    assert_eq!(b["ok"], false, "{b}");
    assert_eq!(a.await.unwrap(), json!("a done"));
    let report = call(&vm, "report").await;
    assert_eq!(report["v"], 0, "{report}");
    assert_eq!(report["log"], json!([{ "state": false, "notify": false }]), "{report}");
    assert!(vm.take_effects().is_empty(), "nothing was queued from a readOnly call");
}

/// A thread `task.spawn(fn)` makes is the scheduler's from its first instruction: another call resuming it is
/// refused, and the task still runs to the end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_thread_made_from_a_function_cannot_be_resumed_by_a_script() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local T, h
            local steps = {}
            app.tool { name = "s", run = function()
              h = task.spawn(function() T = coroutine.running() task.wait(0.1) steps[#steps + 1] = "after wait" return "task done" end)
              return "spawned"
            end }
            app.tool { name = "r", run = function()
              local ok, e = coroutine.resume(T)
              return { ok = ok, e = tostring(e) }
            end }
            app.tool { name = "aw", run = function() return { h:await() } end }
            app.tool { name = "report", readOnly = true, run = function() return steps end }
            "#,
        )
        .await,
    );
    assert_eq!(call(&vm, "s").await, json!("spawned"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let r = call(&vm, "r").await;
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["e"].as_str().unwrap().contains("non-suspended"), "{r}");
    assert_eq!(call(&vm, "aw").await, json!(["task done"]), "the task was not wedged");
    assert_eq!(call(&vm, "report").await, json!(["after wait"]));
}

/// A resume value that is not the executor's "terminate" signal changes nothing: `coroutine.resume(t, "x")` of a
/// thread that is waiting at a host call is refused, and the thread finishes normally.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resuming_a_waiting_thread_with_a_value_is_refused_and_does_not_wedge_it() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local S, co, h
            app.tool { name = "start", run = function()
              co = coroutine.create(function() task.wait(0.15) return "finished" end)
              h = task.spawn(function()
                S = coroutine.running()
                local ok, v = coroutine.resume(co)
                return v
              end)
              return "started"
            end }
            app.tool { name = "poke", run = function()
              local a, ae = coroutine.resume(S, "x")
              local b, be = coroutine.resume(co, "y")
              local c, ce = coroutine.resume(S, 1)
              return { a = a, ae = tostring(ae), b = b, be = tostring(be), c = c, ce = tostring(ce) }
            end }
            app.tool { name = "finish", run = function() return { h:await() } end }
            "#,
        )
        .await,
    );
    call(&vm, "start").await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let poke = call(&vm, "poke").await;
    for (ok, why) in [("a", "ae"), ("b", "be"), ("c", "ce")] {
        assert_eq!(poke[ok], false, "{poke}");
        assert!(poke[why].as_str().unwrap().contains("non-suspended"), "{poke}");
    }
    assert_eq!(call(&vm, "finish").await, json!(["finished"]), "the waiting thread ran on to its end");
}

/// Every entry point of the host into script code starts a thread the script cannot resume: a tool, an event
/// handler, the body of `Mutex:with_lock`, a deferred and a delayed function, and a request handler. Each stashes its
/// thread and waits; a probe resumes them all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_entry_point_starts_a_thread_the_script_cannot_resume() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local hyper = require("vnd:hyper")
            local net = require("std:net")
            local collections = require("std:collections")
            local mu = collections.Mutex.new()
            local threads = {}
            local function stash(name) threads[name] = coroutine.running() task.wait(0.3) end
            local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function(req)
              stash("handler")
              return { status = 200, body = "ok" }
            end)
            local url = "http://" .. server:addr() .. "/"
            app.on("event", function() stash("event") return "event done" end)
            app.tool { name = "tool", run = function() stash("tool") return "tool done" end }
            app.tool { name = "lock", run = function() mu:with_lock(function() stash("with_lock") end) return "lock done" end }
            app.tool { name = "later", run = function()
              task.defer(function() stash("defer") end)
              task.delay(0.01, function() stash("delay") end)
              task.spawn(function() hyper.get(url) end)
              return "scheduled"
            end }
            app.tool { name = "probe", run = function()
              local out = {}
              for _, name in ipairs { "tool", "event", "with_lock", "defer", "delay", "handler" } do
                local t = threads[name]
                if t == nil then
                  out[name] = "never started"
                else
                  local ok, e = coroutine.resume(t)
                  out[name] = (ok and "resumed: " or "refused: ") .. tostring(e)
                end
              end
              return out
            end }
            "#,
        )
        .await,
    );
    let mut running = vec![spawn_call(&vm, "tool"), spawn_call(&vm, "lock")];
    let event = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.emit("event", json!(null)).await.unwrap() })
    };
    running.push(spawn_call(&vm, "later"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let probe = call(&vm, "probe").await;
    for name in ["tool", "event", "with_lock", "defer", "delay", "handler"] {
        let said = probe[name].as_str().unwrap();
        assert!(said.starts_with("refused: ") && said.contains("non-suspended"), "{name}: {probe}");
    }
    for call in running {
        call.await.unwrap();
    }
    assert_eq!(event.await.unwrap(), json!("event done"));
}

/// A top-level plain `coroutine.yield()` waits for nothing, so the call does not lend the VM at it, even when a wait
/// that Lua could not suspend at was left pending just before. This leans on mlua (0.12.x) waking the future's waker
/// when a thread run with `call_async` yields something that is not its own "pending" marker (`AsyncThread::poll`):
/// the call sees that wake and keeps its turn (`lifecycle`, rule 4). If an mlua upgrade stops waking there, this test
/// fails instead of the app interleaving calls in the middle of one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plain_yield_at_the_top_of_a_call_does_not_lend_the_vm() {
    let vm = Arc::new(
        load(
            r#"
            local task = require("std:task")
            local counter = 0
            app.tool { name = "bump", run = function() counter = counter + 1 return counter end }
            app.tool { name = "yielder", run = function()
              local before = counter
              -- a wait Lua cannot suspend at: it is left pending, and the plain yields below are not waits
              pcall(table.sort, { 3, 2, 1 }, function(a, b) task.wait(0) return a < b end)
              for _ = 1, 20000 do coroutine.yield() end
              return { interleaved = counter ~= before }
            end }
            "#,
        )
        .await,
    );
    let yielder = spawn_call(&vm, "yielder");
    tokio::time::sleep(Duration::from_millis(2)).await;
    let bumps: Vec<_> = (0..20).map(|_| spawn_call(&vm, "bump")).collect();
    let out = yielder.await.unwrap();
    for bump in bumps {
        bump.await.unwrap();
    }
    assert_eq!(
        out["interleaved"], false,
        "another call ran in the middle of a call that only yielded: {out}"
    );
}
