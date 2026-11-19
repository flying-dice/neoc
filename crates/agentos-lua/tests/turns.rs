#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Whose turn it is in the VM, and what the call that holds it may do. Every test here failed before its fix.
//!
//! - a wait Lua cannot yield from (a `string.gsub` callback, a sort comparator, a metamethod, an iterator) fails
//!   with a Lua error and must not lend the turn: the call keeps it, and a `readOnly` tool stays read-only;
//! - a wait that failed to yield must not make the call give up the turn at the *next* thing it waits for: a host
//!   call Lua could not suspend at is abandoned the moment the script goes on (every async host function is a
//!   leaf, see `lifecycle`), so a coroutine resumed later gets an error from it, not a stale suspension;
//! - that holds for the first slice of a task too: what a detached call could not suspend at is over, and a
//!   different call that resumes the coroutine gets the error instead of running the host call in its turn;
//! - `task.spawn` inside `db:tx` neither deadlocks nor sees the transaction: the task waits for its turn;
//! - the read-only restriction follows a call's origin: a long-lived read-only poller does not make an unrelated
//!   writable tool's delayed write read-only, while a request to the app's own listener while a read-only call
//!   is alive still is;
//! - an emit that is cancelled mid-call does not leave the state tracker recording.

mod common;

use common::{agent_ctx, load, try_load_with, Stalling};
use serde_json::json;
use std::{fmt::Write as _, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpListener};

/// A tiny HTTP server that answers every request, so a script has a network wait to start.
async fn canned_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                    .await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/")
}

// ---- a wait that cannot yield keeps the turn --------------------------------------------------------------------

/// Each way Lua 5.1 refuses a yield (a C function or the VM calling back into Lua), with each kind of wait
/// inside it. `WAIT` is replaced with the wait's source.
const SPELLINGS: &[(&str, &str)] = &[
    ("gsub callback", r#"string.gsub("a", "a", function() WAIT end)"#),
    ("sort comparator", r"table.sort({ 3, 2, 1 }, function(a, b) WAIT return a < b end)"),
    ("__index metamethod", r"local _ = setmetatable({}, { __index = function() WAIT end }).x"),
    ("__add metamethod", r"local _ = setmetatable({}, { __add = function() WAIT return 1 end }) + 1"),
    ("for iterator", r"for _ in function() WAIT end do end"),
];

fn waits(url: &str) -> Vec<(&'static str, String)> {
    vec![
        ("task.wait", "task.wait(0)".to_owned()),
        ("time.sleep", "time.sleep(time.Duration.from_millis(1))".to_owned()),
        ("net wait", format!("hyper.get({url:?})")),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wait_that_cannot_yield_does_not_lend_the_turn_of_a_read_only_tool() {
    let url = canned_server().await;
    let mut attempts = String::new();
    for (spelling, source) in SPELLINGS {
        for (wait, call) in waits(&url) {
            let body = source.replace("WAIT", &call);
            write!(
                attempts,
                r#"
                do
                  local before = counter
                  local ok, why = pcall(function() {body} end)
                  local result = {{ ok = ok, why = tostring(why) }}
                  -- host calls give any other call the chance to run, if the turn had been lent
                  for _ = 1, 3 do app.db:one("select 1 as one") end
                  result.interleaved = counter ~= before
                  local function refused(f) local ok, e = pcall(f) return (not ok) and tostring(e):find("readOnly", 1, true) ~= nil end
                  result.db_write = refused(function() app.db:exec("insert into t values (1)") end)
                  result.state_write = refused(function() state.v = 5 end)
                  result.kv_write = refused(function() app.kv:set("k", "v") end)
                  result.read = app.db:one("select count(*) as n from t").n
                  out[{spelling:?} .. " / " .. {wait:?}] = result
                end
                "#
            )
            .unwrap();
        }
    }
    let src = format!(
        r#"
        local task, time, hyper = require("std:task"), require("std:time"), require("vnd:hyper")
        local state = app.state {{ v = 0 }}
        local counter = 0
        app.db:migrate {{ [1] = "create table t(a)" }}
        app.tool {{ name = "bump", run = function() counter = counter + 1 return counter end }}
        app.tool {{ name = "ro", readOnly = true, run = function()
          local out = {{}}
          {attempts}
          return out
        end }}
        app.tool {{ name = "rows", readOnly = true, run = function() return {{ n = app.db:one("select count(*) as n from t").n, v = state.v }} end }}
        "#
    );
    let vm = Arc::new(load(&src).await);
    // a writable tool is queued behind the read-only one for the whole run: it must not interleave with it
    let ro = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.call_tool("ro", json!({}), &agent_ctx(0), false).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let bump = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.call_tool("bump", json!({}), &agent_ctx(0), false).await })
    };
    let out =
        tokio::time::timeout(Duration::from_secs(60), ro).await.expect("the tool finished").unwrap().unwrap();
    bump.await.unwrap().unwrap();

    let results = out.as_object().unwrap();
    assert_eq!(results.len(), SPELLINGS.len() * 3, "{out}");
    for (name, r) in results {
        // the wait failed where Lua cannot yield, the call went on, and it is still read-only
        assert_eq!(r["ok"], false, "{name}: {r}");
        assert!(r["why"].as_str().unwrap().contains("yield"), "{name}: {r}");
        assert_eq!(r["interleaved"], false, "{name}: another call ran inside this one: {r}");
        assert_eq!(r["db_write"], true, "{name}: {r}");
        assert_eq!(r["state_write"], true, "{name}: {r}");
        assert_eq!(r["kv_write"], true, "{name}: {r}");
        assert_eq!(r["read"], 0, "{name}: {r}");
    }
    let after = vm.call_tool("rows", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(after, json!({ "n": 0, "v": 0 }), "nothing was written");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wait_that_cannot_yield_lets_a_writable_tool_go_on_and_a_later_wait_still_yields() {
    // the failed wait neither wedges the call nor the next one: after it, a real wait still lends the turn
    let src = r#"
        local task = require("std:task")
        local counter = 0
        app.tool { name = "bump", run = function() counter = counter + 1 return counter end }
        app.tool { name = "w", run = function()
          local ok = pcall(string.gsub, "a", "a", function() task.wait(0) end)
          local before = counter
          task.wait(0.2) -- yields for real: the turn is lent, bump runs meanwhile
          return { failed = not ok, bumped = counter - before }
        end }
    "#;
    let vm = Arc::new(load(src).await);
    let w = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.call_tool("w", json!({}), &agent_ctx(0), false).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    vm.call_tool("bump", json!({}), &agent_ctx(0), false).await.unwrap();
    let out = w.await.unwrap().unwrap();
    assert_eq!(out, json!({ "failed": true, "bumped": 1 }));
}

// ---- a failed wait is forgotten when anything else runs ------------------------------------------------------

/// A query that takes a while, so a call that resumes it really suspends at it.
const HEAVY: &str = "with recursive c(x) as (select 1 union all select x + 1 from c where x < 1000000) select count(*) as n from c";

/// Where the script under test runs, as the source that starts `body()`.
const CONTAINERS: &[(&str, &str)] = &[
    ("tool", "return body()"),
    ("task.defer", "task.defer(function() result = body() end) return 'scheduled'"),
    ("task.delay", "task.delay(0, function() result = body() end) return 'scheduled'"),
    (
        "task.spawn after a wait",
        "task.spawn(function() task.wait(0.01) result = body() end) return 'scheduled'",
    ),
];

/// How the stale request to lend is made: a wait that Lua cannot yield from.
const FAILED_WAITS: &[(&str, &str)] = &[
    ("task.wait", "task.wait(5)"),
    ("time.sleep", "time.sleep(time.Duration.from_millis(5000))"),
    ("net wait", "hyper.get(URL)"),
];

/// The host call that is left in flight and resumed after the failed wait.
const LATER_CALLS: &[(&str, &str)] = &[("db:one", "app.db:one(HEAVY)"), ("db:all", "app.db:all(HEAVY)[1]")];

async fn stale_lend_case(container: &str, wait: &str, later: &str, url: &str) -> serde_json::Value {
    let src = format!(
        r#"
        local task, time, hyper = require("std:task"), require("std:time"), require("vnd:hyper")
        local state = app.state {{ v = 0 }}
        local counter = 0
        local result
        local function refused(f) local ok, e = pcall(f) return (not ok) and tostring(e):find("readOnly", 1, true) ~= nil end
        local function body()
          -- `late` is left with its query in flight, `failing` with a wait: both resumes fail at the C boundary
          local late = coroutine.create(function() return {later} end)
          local failing = coroutine.create(function() {wait} end)
          local a = pcall(string.gsub, "x", "x", function() coroutine.resume(late) end)
          local b = pcall(string.gsub, "x", "x", function() coroutine.resume(failing) end)
          local before = counter
          -- the query Lua could not suspend at is over (abandoned), and so is the wait
          local ok, why = coroutine.resume(late)
          -- the call really suspends here, at a query, and not at the failed wait
          local row = {later}
          return {{
            failed = not a and not b, abandoned = (not ok) and tostring(why):find("abandoned", 1, true) ~= nil,
            n = row and row.n,
            interleaved = counter ~= before,
            state_write_refused = refused(function() state.v = 42 end),
            notify_refused = refused(function() app.notify {{ title = "from a readOnly call" }} end),
          }}
        end
        app.tool {{ name = "bump", run = function() counter = counter + 1 return counter end }}
        app.tool {{ name = "ro", readOnly = true, run = function() {container} end }}
        app.tool {{ name = "result", readOnly = true, run = function() return result or "none" end }}
        app.tool {{ name = "v", readOnly = true, run = function() return state.v end }}
        "#,
        later = later.replace("HEAVY", &format!("{HEAVY:?}")),
        wait = wait.replace("URL", &format!("{url:?}")),
    );
    let vm = Arc::new(load(&src).await);
    let ro = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.call_tool("ro", json!({}), &agent_ctx(0), false).await })
    };
    // a writable tool is queued behind the read-only work for as long as it runs
    tokio::time::sleep(Duration::from_millis(30)).await;
    let bump = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.call_tool("bump", json!({}), &agent_ctx(0), false).await })
    };
    let mut out =
        tokio::time::timeout(Duration::from_secs(30), ro).await.expect("ro finished").unwrap().unwrap();
    for _ in 0..300 {
        if out != json!("scheduled") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        out = vm.call_tool("result", json!({}), &agent_ctx(0), false).await.unwrap();
        if out == json!("none") {
            out = json!("scheduled");
        }
    }
    tokio::time::timeout(Duration::from_secs(10), bump).await.expect("bump ran").unwrap().unwrap();
    assert_eq!(
        vm.call_tool("v", json!({}), &agent_ctx(0), false).await.unwrap(),
        json!(0),
        "nothing was written"
    );
    assert!(vm.take_effects().is_empty(), "nothing was queued");
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_wait_does_not_make_a_later_host_call_lend_the_turn_of_a_read_only_call() {
    let url = canned_server().await;
    // one at a time: the cases are CPU-bound and would starve the other tests' timers
    for (container, c) in CONTAINERS {
        for (wait, w) in FAILED_WAITS {
            for (later, l) in LATER_CALLS {
                let name = format!("{container} / {wait} / {later}");
                check_stale_lend_case(&name, &stale_lend_case(c, w, l, &url).await);
            }
        }
    }
}

fn check_stale_lend_case(name: &str, out: &serde_json::Value) {
    assert_eq!(out["failed"], true, "{name}: both waits failed where Lua cannot yield: {out}");
    assert_eq!(out["abandoned"], true, "{name}: a host call Lua could not suspend at is over: {out}");
    assert_eq!(out["n"], 1_000_000, "{name}: {out}");
    assert_eq!(out["interleaved"], false, "{name}: another call ran inside this one: {out}");
    assert_eq!(out["state_write_refused"], true, "{name}: the call is still read-only: {out}");
    assert_eq!(out["notify_refused"], true, "{name}: {out}");
}

// ---- a host call left waiting for a turn by a detached call --------------------------------------------------------

/// Host calls of every kind that take the turn first.
const LEAVES: &[(&str, &str)] = &[
    ("db:one", r#"app.db:one("select 1 as one")"#),
    ("db:all", r#"app.db:all("select 1 as one")"#),
    ("db:exec", r#"app.db:exec("insert into t values (1)")"#),
    ("db:tx", "app.db:tx(function() return 1 end)"),
    ("kv:get", r#"app.kv:get("k")"#),
    ("kv:set", r#"app.kv:set("k", 1)"#),
    ("kv:delete", r#"app.kv:delete("k")"#),
];

/// Who resumes the coroutine that the task's first slice left waiting for its turn.
const RESUMERS: &[(&str, &str)] = &[
    ("the spawning tool", "task.spawn(start) return resume_it()"),
    ("a deferred function", "task.spawn(start) task.defer(resume_it) return 'scheduled'"),
    ("a delayed function", "task.spawn(start) task.delay(0.01, resume_it) return 'scheduled'"),
    (
        "the task's own continuation",
        "task.spawn(function() start() task.wait(0.01) resume_it() end) return 'scheduled'",
    ),
];

async fn detached_hold_case(resumer: &str, leaf: &str) -> serde_json::Value {
    let src = format!(
        r#"
        local task = require("std:task")
        app.db:migrate {{ [1] = "create table t(a)" }}
        local co, outcome
        -- the first slice of a task runs without the turn: the call below waits for it (the spawner holds it),
        -- its resume fails at the C boundary, and the coroutine is left with the call in flight
        local function start()
          co = coroutine.create(function() return {leaf} end)
          pcall(string.gsub, "x", "x", function() coroutine.resume(co) end)
        end
        local function resume_it()
          local ok, r = coroutine.resume(co)
          outcome = {{ ok = ok, r = tostring(r) }}
          return outcome
        end
        app.tool {{ name = "go", run = function() {resumer} end }}
        app.tool {{ name = "outcome", readOnly = true, run = function() return outcome or "none" end }}
        app.tool {{ name = "ping", run = function() return "pong" end }}
        "#
    );
    let vm = Arc::new(load(&src).await);
    let call = |tool: &'static str| {
        let vm = vm.clone();
        async move {
            tokio::time::timeout(Duration::from_secs(5), vm.call_tool(tool, json!({}), &agent_ctx(0), false))
                .await
                .unwrap_or_else(|_| panic!("{tool} never finished: the app is wedged"))
                .unwrap()
        }
    };
    let mut out = call("go").await;
    while out == json!("scheduled") {
        tokio::time::sleep(Duration::from_millis(20)).await;
        out = match call("outcome").await {
            none if none == json!("none") => json!("scheduled"),
            done => done,
        };
    }
    assert_eq!(call("ping").await, json!("pong"), "the app still answers");
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_call_a_detached_call_could_not_suspend_at_is_over_when_someone_resumes_it() {
    for (resumer, r) in RESUMERS {
        for (leaf, l) in LEAVES {
            let out = detached_hold_case(r, l).await;
            assert_eq!(out["ok"], false, "{resumer} / {leaf}: {out}");
            assert!(
                out["r"].as_str().is_some_and(|why| why.contains("abandoned")),
                "{resumer} / {leaf}: the host call Lua could not suspend at is over: {out}"
            );
        }
    }
}

/// A write that a writable call's task could not suspend at is never run: a `readOnly` call that resumes the
/// coroutine gets the abandoned call's error, and the write did not happen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_a_task_could_not_suspend_at_is_never_run_by_a_later_read_only_call() {
    for (write, check) in [
        (r#"app.db:exec("insert into t values (1)")"#, r#"#app.db:all("select a from t") == 0"#),
        (r#"app.kv:set("k", 1)"#, r#"app.kv:get("k") == nil"#),
        (r#"app.kv:delete("k")"#, "true"),
    ] {
        let src = format!(
            r#"
            local task = require("std:task")
            app.db:migrate {{ [1] = "create table t(a)" }}
            local co
            app.tool {{ name = "start", run = function()
              task.spawn(function()
                co = coroutine.create(function() return {write} end)
                pcall(string.gsub, "x", "x", function() coroutine.resume(co) end)
              end)
              return "started"
            end }}
            app.tool {{ name = "finish", readOnly = true, run = function()
              local ok, err = coroutine.resume(co)
              return {{ ok = ok, err = tostring(err), untouched = {check} }}
            end }}
            "#
        );
        let vm = load(&src).await;
        let call = |tool: &'static str| async {
            tokio::time::timeout(Duration::from_secs(5), vm.call_tool(tool, json!({}), &agent_ctx(0), false))
                .await
        };
        call("start").await.expect("start finished").unwrap();
        let out = call("finish").await.expect("finish did not wedge the app").unwrap();
        assert_eq!(out["ok"], false, "{write}: {out}");
        assert!(out["err"].as_str().unwrap().contains("abandoned"), "{write}: {out}");
        assert_eq!(out["untouched"], true, "{write}: {out}");
    }
}

// ---- task.spawn inside db:tx ------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_spawned_in_a_transaction_waits_for_it_instead_of_deadlocking() {
    let src = r#"
        local task = require("std:task")
        local seen = {}
        app.db:migrate { [1] = "create table t(a)" }
        app.tool { name = "with_statement_after", run = function()
          app.db:tx(function()
            app.db:exec("insert into t values (1)")
            task.spawn(function() seen.a = #app.db:all("select a from t") end)
            app.db:exec("insert into t values (2)")
          end)
          return "done"
        end }
        app.tool { name = "nothing_after", run = function()
          app.db:tx(function()
            app.db:exec("insert into t values (3)")
            task.spawn(function() seen.b = #app.db:all("select a from t") end)
          end)
          return "done"
        end }
        app.tool { name = "unrelated", readOnly = true, run = function()
          return { rows = #app.db:all("select a from t"), seen = seen }
        end }
    "#;
    let vm = load(src).await;
    let patience = Duration::from_secs(5);
    for tool in ["with_statement_after", "nothing_after"] {
        let r = tokio::time::timeout(patience, vm.call_tool(tool, json!({}), &agent_ctx(0), false))
            .await
            .unwrap_or_else(|_| panic!("{tool} deadlocked"));
        assert_eq!(r.unwrap(), json!("done"));
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let r = tokio::time::timeout(patience, vm.call_tool("unrelated", json!({}), &agent_ctx(0), false))
        .await
        .expect("a later, unrelated call does not hang")
        .unwrap();
    // the spawned tasks ran after their transactions committed: they saw every row of them
    assert_eq!(r, json!({ "rows": 3, "seen": { "a": 2, "b": 3 } }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_spawned_in_a_transaction_that_writes_waits_for_it_too() {
    let src = r#"
        local task = require("std:task")
        app.db:migrate { [1] = "create table t(a)" }
        app.tool { name = "w", run = function()
          app.db:tx(function()
            app.db:exec("insert into t values (1)")
            task.spawn(function() app.db:exec("insert into t values (100)") end)
            app.db:exec("insert into t values (2)")
          end)
          return "done"
        end }
        app.tool { name = "rows", readOnly = true, run = function() return app.db:all("select a from t order by rowid") end }
    "#;
    let vm = load(src).await;
    tokio::time::timeout(Duration::from_secs(5), vm.call_tool("w", json!({}), &agent_ctx(0), false))
        .await
        .expect("no deadlock")
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let rows = vm.call_tool("rows", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(rows, json!([{ "a": 1 }, { "a": 2 }, { "a": 100 }]), "the task's write came after the commit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_spawned_by_a_read_only_tool_in_a_transaction_stays_read_only() {
    // the spawned slice reads its own turn's flag, not whichever call held the gate when the statement was built
    let src = r#"
        local task = require("std:task")
        local seen = "unset"
        app.db:migrate { [1] = "create table t(a)" }
        app.tool { name = "ro", readOnly = true, run = function()
          app.db:tx(function()
            task.spawn(function() seen = tostring(select(2, pcall(function() app.db:exec("insert into t values (9)") end))) end)
          end)
          return "done"
        end }
        app.tool { name = "report", readOnly = true, run = function() return { seen = seen, n = app.db:one("select count(*) as n from t").n } end }
    "#;
    let vm = load(src).await;
    vm.call_tool("ro", json!({}), &agent_ctx(0), false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let report = vm.call_tool("report", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(report["n"], 0, "{report}");
    assert!(report["seen"].as_str().unwrap().to_lowercase().contains("read"), "{report}");
}

// ---- the read-only restriction follows the call's origin -------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_only_poller_does_not_make_a_writable_tools_delayed_write_read_only() {
    let src = r#"
        local task = require("std:task")
        local state = app.state { v = 0 }
        local delayed, deferred = "unset", "unset"
        app.tool { name = "ro", readOnly = true, run = function()
          task.spawn(function() while true do task.wait(0.02) end end) -- lives for as long as the app does
          return "ok"
        end }
        app.tool { name = "w", run = function()
          task.delay(0.05, function() delayed = tostring(pcall(function() state.v = 7 end)) end)
          task.defer(function() deferred = tostring(pcall(function() state.v = 8 end)) end)
          return "ok"
        end }
        app.tool { name = "report", readOnly = true, run = function() return { delayed = delayed, deferred = deferred, v = state.v } end }
    "#;
    let vm = load(src).await;
    vm.call_tool("ro", json!({}), &agent_ctx(0), false).await.unwrap();
    vm.call_tool("w", json!({}), &agent_ctx(0), false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let report = vm.call_tool("report", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(report, json!({ "delayed": "true", "deferred": "true", "v": 7 }), "{report}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_started_by_a_read_only_tool_stays_read_only_whenever_it_runs() {
    let src = r#"
        local task = require("std:task")
        local state = app.state { v = 0 }
        local results = {}
        app.tool { name = "ro", readOnly = true, run = function()
          task.delay(0.05, function() results.delay = tostring(pcall(function() state.v = 1 end)) end)
          task.defer(function() results.defer = tostring(pcall(function() state.v = 2 end)) end)
          return "ok"
        end }
        app.tool { name = "report", readOnly = true, run = function() return { results = results, v = state.v } end }
    "#;
    let vm = load(src).await;
    vm.call_tool("ro", json!({}), &agent_ctx(0), false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let report = vm.call_tool("report", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(report, json!({ "results": { "delay": "false", "defer": "false" }, "v": 0 }), "{report}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_to_the_apps_own_server_is_read_only_while_a_read_only_call_is_alive() {
    // the handler tries to write; the answer says whether it could
    let src = r#"
        local task, net, hyper = require("std:task"), require("std:net"), require("vnd:hyper")
        local state = app.state { v = 0 }
        local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function(req)
          local ok = pcall(function() state.v = state.v + 1 end)
          return { status = 200, body = tostring(ok) }
        end)
        app.tool { name = "addr", run = function() return server:addr() end }
        -- a readOnly tool that calls the app's own listener (and waits for the answer): the handler is its request
        app.tool { name = "ro_calls_itself", readOnly = true, run = function()
          local res = hyper.get("http://" .. server:addr() .. "/")
          return res.body
        end }
    "#;
    let vm = Arc::new(load(src).await);
    let addr = vm.call_tool("addr", json!({}), &agent_ctx(0), false).await.unwrap();
    let addr = addr.as_str().unwrap().to_owned();
    // nothing read-only is alive: a request from outside may write
    assert_eq!(get(&addr).await, "true");
    // a readOnly tool is alive and made the request: it may not
    let own = vm.call_tool("ro_calls_itself", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(own, json!("false"));
    // and the restriction ended with the tool
    assert_eq!(get(&addr).await, "true");
}

async fn get(addr: &str) -> String {
    use tokio::io::AsyncReadExt;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(format!("GET / HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).await.unwrap();
    out.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default()
}

// ---- a cancelled emit -------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_tracked_emit_leaves_the_vm_usable_and_the_next_one_tracks_afresh() {
    let src = r#"
        local state = app.state { a = 1, b = 2 }
        app.on("stuck", function() local _ = state.a app.db:all("select 'stall'") end)
        app.on("reads_b", function() return state.b end)
    "#;
    let vm = try_load_with("emit", src, Stalling::new()).await.unwrap();
    let cancelled =
        tokio::time::timeout(Duration::from_millis(200), vm.emit_tracked("stuck", json!(null))).await;
    assert!(cancelled.is_err(), "the emit is held at a host call and its caller gives up");
    let (value, deps) = vm.emit_tracked("reads_b", json!(null)).await.unwrap();
    assert_eq!(value, json!(2));
    let keys: Vec<&str> = deps.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys, ["b"], "the cancelled emit's reads are not carried over");
}
