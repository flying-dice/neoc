#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A host function that panics during a call into an app (a module's Rust code, a port behind the `app` API) fails
//! that call with an error, never the task that polled it, and leaves the app answering with nothing of the call
//! left open: its turn, its transaction and its `readOnly` restriction end with it. The panicking host function
//! here is the agent roster behind `app.agents:list()`, a port the app API calls synchronously, as a module's
//! function is called.

use crate::common;

use common::{agent_ctx, db, id, package, timed_tool, user_ctx};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    ports::{AgentCard, AgentRoster, AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::{AppVm, LuaRuntime, VmConfig, VmError};

/// The first line of every source here: the line numbers the tests name are the body's plus one.
const MANIFEST: &str = "-- probe";

struct PanickingRoster;
impl AgentRoster for PanickingRoster {
    fn cards(&self) -> Vec<AgentCard> {
        panic!("the roster broke")
    }
}

const APP: &str = r#"
local hyper = require("vnd:hyper")
local net = require("std:net")
local db = app.db
db:migrate { [1] = "create table t(a integer)" }
local function count() return db:one("select count(*) as n from t").n end
RPC.on_request("uncaught", function() return app.agents:list() end)
RPC.on_request("caught", function()
  local ok, err = pcall(function() return app.agents:list() end)
  return { ok = ok, err = tostring(err) }
end)
RPC.on_request("uncaught_in_tx", function()
  db:tx(function() db:exec("insert into t values (1)") app.agents:list() end)
end)
RPC.on_request("caught_in_tx", function()
  local ok = pcall(function() db:tx(function() db:exec("insert into t values (2)") app.agents:list() end) end)
  db:exec("insert into t values (3)")
  return { ok = ok, n = count() }
end)
RPC.on_request("read_only", function() return app.agents:list() end, { read_only = true })
RPC.on_request("in_task", function()
  local task = require("std:task")
  local h = task.spawn(function() task.wait(0) return app.agents:list() end)
  local r, err = h:await()
  return { returned = r ~= nil, kind = err and err.kind, reason = err and err.reason }
end)
RPC.on_request("write", function() db:exec("insert into t values (4)") return count() end)
-- the app's own server: a request to it carries no origin, so it runs read-only while a read-only call is active
local server = assert(hyper.serve(assert(net.TcpListener.bind("127.0.0.1:0")), function()
  db:exec("insert into t values (5)")
  return tostring(count())
end))
RPC.on_request("own_request", function()
  local r = assert(hyper.get("http://" .. server:addr() .. "/"))
  return { status = r.status, body = r.body }
end)
RPC.on_request("values", function() return db:all("select a from t order by a") end)
"#;

async fn panicking_app() -> Arc<AppVm> {
    let config = VmConfig { roster: Arc::new(PanickingRoster), ..VmConfig::default() };
    let src = format!("{MANIFEST}\n{APP}");
    Arc::new(AppVm::load(&id("probe"), package(&common::backend(&src), &[]), db(), config).await.unwrap())
}

async fn call(vm: &AppVm, tool: &str) -> vos_lua::Result<Value> {
    common::serve(vm, tool, json!({}), &agent_ctx(0)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_the_script_does_not_catch_fails_that_call_and_the_app_answers_on() {
    let vm = panicking_app().await;
    // on a task of its own, so a panic that escaped the call would fail the join, not this test's own task
    let err = timed_tool(&vm, "uncaught", 10).await.expect_err("the call failed");
    assert!(err.contains("the call failed: a host function panicked: the roster broke"), "{err}");
    match call(&vm, "uncaught").await {
        Err(VmError::Panicked(message)) => assert_eq!(message, "the roster broke"),
        other => panic!("expected a panicked call, got {other:?}"),
    }
    assert_eq!(call(&vm, "write").await.unwrap(), json!(1), "the next call runs, and writes");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_is_an_error_the_script_can_catch() {
    let vm = panicking_app().await;
    let out = call(&vm, "caught").await.unwrap();
    assert_eq!(out, json!({ "ok": false, "err": "the roster broke" }));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_inside_db_tx_rolls_the_transaction_back_whether_or_not_it_is_caught() {
    let vm = panicking_app().await;
    assert!(matches!(call(&vm, "uncaught_in_tx").await, Err(VmError::Panicked(_))));
    // caught: the statement after the pcall is outside the failed transaction, so it commits on its own
    assert_eq!(call(&vm, "caught_in_tx").await.unwrap(), json!({ "ok": false, "n": 1 }));
    // the database is free (no transaction left holding it) and only the writes outside a transaction are there
    assert_eq!(call(&vm, "write").await.unwrap(), json!(2));
    assert_eq!(call(&vm, "values").await.unwrap(), json!([{ "a": 3 }, { "a": 4 }]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_call_that_panics_leaves_the_next_call_and_the_apps_own_server_writable() {
    let vm = panicking_app().await;
    // control: a request to the app's own server from a writable call may write
    assert_eq!(call(&vm, "own_request").await.unwrap(), json!({ "status": 200, "body": "1" }));
    assert!(matches!(
        common::serve_with(&vm, "read_only", json!({}), &agent_ctx(0), true).await,
        Err(VmError::Panicked(_))
    ));
    assert_eq!(call(&vm, "write").await.unwrap(), json!(2));
    // the panicked call no longer counts as an active read-only call: had it leaked, this request would run
    // read-only, its write would raise and the server would answer 500
    assert_eq!(call(&vm, "own_request").await.unwrap(), json!({ "status": 200, "body": "3" }));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_a_task_fails_the_task_and_its_await_returns() {
    let vm = panicking_app().await;
    let out = timed_tool(&vm, "in_task", 10).await.unwrap();
    assert_eq!(out["returned"], json!(false), "{out}");
    assert_eq!(out["kind"], json!("failed"), "{out}");
    let reason = out["reason"].as_str().unwrap();
    assert!(reason.contains("a host function panicked: the roster broke"), "{reason}");
    assert_eq!(timed_tool(&vm, "write", 10).await.unwrap(), json!(1), "the gate is free");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_while_the_app_loads_fails_the_load() {
    let config = VmConfig { roster: Arc::new(PanickingRoster), ..VmConfig::default() };
    let src = format!("{MANIFEST}\napp.agents:list()");
    match AppVm::load(&id("probe"), package(&src, &[]), db(), config).await {
        Err(VmError::Panicked(message)) => assert_eq!(message, "the roster broke"),
        Err(other) => panic!("expected a panicked load, got {other}"),
        Ok(_) => panic!("the load succeeded"),
    }
}

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

/// Through the `AppRuntime` port, as the host calls it: the caller gets an internal error that says what
/// happened, and the app stays loaded.
#[tokio::test(flavor = "multi_thread")]
async fn the_port_reports_a_panic_as_an_internal_error_and_keeps_the_app_loaded() {
    let runtime = LuaRuntime::new(Arc::new(Quiet), Arc::new(RecordingShell::default()))
        .with_roster(Arc::new(PanickingRoster));
    let data: Arc<dyn AppData> = db();
    runtime
        .load(&id("probe"), package(&common::backend(&format!("{MANIFEST}\n{APP}")), &[]), data)
        .await
        .unwrap();
    let app = id("probe");
    let call = |tool: &'static str| common::serve_rt(&runtime, &app, tool, json!({}), agent_ctx(0));
    match call("uncaught").await {
        Err(PortError::Internal(message)) => {
            assert_eq!(message, "a host function panicked: the roster broke");
        }
        other => panic!("expected an internal error, got {other:?}"),
    }
    assert_eq!(call("write").await.unwrap(), json!(1));
}

/// A notifier whose first notification panics, then records.
#[derive(Default)]
struct PanicsOnce {
    panicked: std::sync::atomic::AtomicBool,
    titles: parking_lot::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl Notifier for PanicsOnce {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        assert!(self.panicked.swap(true, std::sync::atomic::Ordering::SeqCst), "the notifier broke");
        self.titles.lock().push(n.title);
        Ok(())
    }
}

/// The pump that forwards what an app's background work asks for survives a panic in a port, and one effect that
/// panics costs only itself: the rest of its batch and the next batch still go out.
#[tokio::test(flavor = "multi_thread")]
async fn a_panic_forwarding_background_effects_leaves_the_pump_running() {
    let notifier = Arc::new(PanicsOnce::default());
    let runtime = LuaRuntime::new(notifier.clone(), Arc::new(RecordingShell::default()));
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        RPC.on_request("later", function()
          task.spawn(function()
            task.wait(0)
            app.notify {{ title = "first" }}
            app.notify {{ title = "second" }}
            task.wait(0.05)
            app.notify {{ title = "third" }}
          end)
        end)
        "#
    );
    let data: Arc<dyn AppData> = db();
    runtime.load(&id("probe"), package(&common::backend(&src), &[]), data).await.unwrap();
    common::serve_rt(&runtime, &id("probe"), "later", json!({}), agent_ctx(0)).await.unwrap();
    for _ in 0..200 {
        if notifier.titles.lock().len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(notifier.panicked.load(std::sync::atomic::Ordering::SeqCst), "the first notification panicked");
    assert_eq!(
        *notifier.titles.lock(),
        ["second", "third"],
        "the rest of the batch, and the next batch, were forwarded"
    );
}

// ---- handlers --------------------------------------------------------------------------------------------

const HANDLERS: &str = r#"
local n = 0
RPC.on_request("boom", function() return app.agents:list() end)
RPC.on_request("add", function() n = n + 1 return n end)
"#;

/// A panic in a handler (the page's call into the backend) fails that call, and the app answers the next one.
#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_a_handler_fails_that_call_and_the_app_answers_the_next() {
    let config = VmConfig { roster: Arc::new(PanickingRoster), ..VmConfig::default() };
    let src = format!("{MANIFEST}\n{HANDLERS}");
    let vm = AppVm::load(&id("probe"), package(&src, &[]), db(), config).await.unwrap();
    assert_eq!(vm.call_handler("add", json!(null), &user_ctx(0)).await.unwrap(), json!(1));
    match vm.call_handler("boom", json!(null), &user_ctx(0)).await {
        Err(VmError::Panicked(message)) => assert_eq!(message, "the roster broke"),
        other => panic!("expected a panicked call, got {other:?}"),
    }
    assert_eq!(
        vm.call_handler("add", json!(null), &user_ctx(0)).await.unwrap(),
        json!(2),
        "the call after the panic ran, on the state the app had"
    );
}
