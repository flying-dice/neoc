#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A host function that panics during a call into an app (a module's Rust code, a port behind the `app` API) fails
//! that call with an error, never the task that polled it, and leaves the app answering with nothing of the call
//! left open: its turn, its transaction and its `readOnly` restriction end with it. The panicking host function
//! here is the agent roster behind `app.agents:list()`, a port the app API calls synchronously, as a module's
//! function is called.

mod common;

use common::{agent_ctx, db, id, package, timed_tool};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    ports::{AgentCard, AgentRoster, AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::{AppVm, LuaRuntime, VmConfig, VmError};

const MANIFEST: &str = r#"app.manifest { id = "probe", name = "Probe", icon = "gear" }"#;

struct PanickingRoster;
impl AgentRoster for PanickingRoster {
    fn cards(&self) -> Vec<AgentCard> {
        panic!("the roster broke")
    }
}

const APP: &str = r#"
local db = app.db
db:migrate { [1] = "create table t(a integer)" }
local function count() return db:one("select count(*) as n from t").n end
app.tool { name = "uncaught", run = function() return app.agents:list() end }
app.tool { name = "caught", run = function()
  local ok, err = pcall(function() return app.agents:list() end)
  return { ok = ok, err = tostring(err) }
end }
app.tool { name = "uncaught_in_tx", run = function()
  db:tx(function() db:exec("insert into t values (1)") app.agents:list() end)
end }
app.tool { name = "caught_in_tx", run = function()
  local ok = pcall(function() db:tx(function() db:exec("insert into t values (2)") app.agents:list() end) end)
  db:exec("insert into t values (3)")
  return { ok = ok, n = count() }
end }
app.tool { name = "read_only", readOnly = true, run = function() return app.agents:list() end }
app.tool { name = "in_task", run = function()
  local task = require("std:task")
  local h = task.spawn(function() task.wait(0) return app.agents:list() end)
  local r, err = h:await()
  return { returned = r ~= nil, err = err }
end }
app.tool { name = "write", run = function() db:exec("insert into t values (4)") return count() end }
app.tool { name = "values", run = function() return db:all("select a from t order by a") end }
"#;

async fn panicking_app() -> Arc<AppVm> {
    let config = VmConfig { roster: Arc::new(PanickingRoster), ..VmConfig::default() };
    let src = format!("{MANIFEST}\n{APP}");
    Arc::new(AppVm::load(&id("probe"), package(&src, &[]), db(), config).await.unwrap())
}

async fn call(vm: &AppVm, tool: &str) -> vos_lua::Result<Value> {
    vm.call_tool(tool, json!({}), &agent_ctx(0), false).await
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
async fn a_read_only_call_that_panics_leaves_the_next_call_writable() {
    let vm = panicking_app().await;
    assert!(matches!(
        vm.call_tool("read_only", json!({}), &agent_ctx(0), true).await,
        Err(VmError::Panicked(_))
    ));
    assert_eq!(call(&vm, "write").await.unwrap(), json!(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_a_task_fails_the_task_and_its_await_returns() {
    let vm = panicking_app().await;
    let out = timed_tool(&vm, "in_task", 10).await.unwrap();
    assert_eq!(out["returned"], json!(false), "{out}");
    let err = out["err"].as_str().unwrap();
    assert!(err.contains("a host function panicked: the roster broke"), "{err}");
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
    runtime.load(&id("probe"), package(&format!("{MANIFEST}\n{APP}"), &[]), data).await.unwrap();
    let app = id("probe");
    let call = |tool: &'static str| runtime.call_tool(&app, tool, json!({}), agent_ctx(0), false);
    match call("uncaught").await {
        Err(PortError::Internal(message)) => {
            assert_eq!(message, "a host function panicked: the roster broke");
        }
        other => panic!("expected an internal error, got {other:?}"),
    }
    assert_eq!(call("write").await.unwrap(), json!(1));
}
