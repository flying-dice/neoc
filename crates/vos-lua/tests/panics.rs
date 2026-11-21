#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A host function that panics during a call into an app (a module's Rust code, a port behind the `app` API) fails
//! that call with an error, never the task that polled it, and leaves the app answering with nothing of the call
//! left open: its turn, its transaction and its `readOnly` restriction end with it. The panicking host function
//! here is the agent roster behind `app.agents:list()`, a port the app API calls synchronously, as a module's
//! function is called.

mod common;

use common::{agent_ctx, db, id, package, timed_tool, user_ctx};
use serde_json::{json, Map, Value};
use std::{collections::HashMap, sync::Arc};
use vos_application::{
    ports::{AgentCard, AgentRoster, AppData, AppRuntime, Notifier, PortResult, ViewEventCall},
    testing::RecordingShell,
    PortError,
};
use vos_domain::{
    ui::{node::join_path, Node},
    Notification,
};
use vos_lua::{AppVm, LuaRuntime, VmConfig, VmError};

const MANIFEST: &str = r#"app.manifest { id = "probe", name = "Probe", icon = "gear" }"#;

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
-- the app's own server: a request to it carries no origin, so it runs read-only while a read-only call is active
local server = assert(hyper.serve(assert(net.TcpListener.bind("127.0.0.1:0")), function()
  db:exec("insert into t values (5)")
  return tostring(count())
end))
app.tool { name = "own_request", run = function()
  local r = assert(hyper.get("http://" .. server:addr() .. "/"))
  return { status = r.status, body = r.body }
end }
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
async fn a_read_only_call_that_panics_leaves_the_next_call_and_the_apps_own_server_writable() {
    let vm = panicking_app().await;
    // control: a request to the app's own server from a writable call may write
    assert_eq!(call(&vm, "own_request").await.unwrap(), json!({ "status": 200, "body": "1" }));
    assert!(matches!(
        vm.call_tool("read_only", json!({}), &agent_ctx(0), true).await,
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

/// A notifier whose first notification panics, then records.
#[derive(Default)]
struct PanicsOnce {
    panicked: std::sync::atomic::AtomicBool,
    titles: parking_lot::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl Notifier for PanicsOnce {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        if !self.panicked.swap(true, std::sync::atomic::Ordering::SeqCst) {
            panic!("the notifier broke");
        }
        self.titles.lock().push(n.title);
        Ok(())
    }
}

/// The pump that forwards what an app's background work asks for survives a panic in a port: the next
/// notification still goes out.
#[tokio::test(flavor = "multi_thread")]
async fn a_panic_forwarding_background_effects_leaves_the_pump_running() {
    let notifier = Arc::new(PanicsOnce::default());
    let runtime = LuaRuntime::new(notifier.clone(), Arc::new(RecordingShell::default()));
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        app.tool {{ name = "later", run = function()
          task.spawn(function()
            task.wait(0)
            app.notify {{ title = "first" }}
            task.wait(0.05)
            app.notify {{ title = "second" }}
          end)
        end }}
        "#
    );
    let data: Arc<dyn AppData> = db();
    runtime.load(&id("probe"), package(&src, &[]), data).await.unwrap();
    runtime.call_tool(&id("probe"), "later", json!({}), agent_ctx(0), false).await.unwrap();
    for _ in 0..200 {
        if !notifier.titles.lock().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(notifier.panicked.load(std::sync::atomic::Ordering::SeqCst), "the first notification panicked");
    assert_eq!(*notifier.titles.lock(), ["second"], "the pump forwarded the next one");
}

// ---- windows ---------------------------------------------------------------------------------------------

const WINDOWS: &str = r#"
local ui = require("vos:ui")
local s = app.state { n = 0 }
app.window("main", {
  onOpen = function(w, p) if p.explode then app.agents:list() end end,
  render = function(w)
    return ui.VStack {
      ui.Text { key = "n", tostring(s.n) },
      ui.Button { key = "boom", title = "Boom", onTap = function() app.agents:list() end },
      ui.Button { key = "add", title = "Add", onTap = function() s.n = s.n + 1 end },
    }
  end,
})
app.window("broken", { render = function(w) return app.agents:list() end })
"#;

async fn windows_app() -> AppVm {
    let config = VmConfig {
        roster: Arc::new(PanickingRoster),
        host_modules: Arc::new(HashMap::from([("ui".to_owned(), vos_lua::UI_MODULE.as_bytes().to_vec())])),
        ..VmConfig::default()
    };
    let src = format!("{MANIFEST}\n{WINDOWS}");
    AppVm::load(&id("probe"), package(&src, &[]), db(), config).await.unwrap()
}

fn path_of(node: &Node, key: &str) -> Option<String> {
    fn walk(node: &Node, path: &str, key: &str) -> Option<String> {
        if node.key == key {
            return Some(path.to_owned());
        }
        node.containers().find_map(|(slot, children)| {
            children.iter().find_map(|child| walk(child, &join_path(path, slot, &child.key), key))
        })
    }
    walk(node, &node.key, key)
}

fn tap(tree: &Node, key: &str) -> ViewEventCall {
    let path = path_of(tree, key).unwrap_or_else(|| panic!("no node `{key}`"));
    ViewEventCall { path, event: "tap".into(), row_key: None, row: None, payload: None }
}

/// Is `view` still an open instance? (`set_client_values` looks it up without running app code.)
async fn is_open(vm: &AppVm, view: &str) -> bool {
    !matches!(vm.set_client_values(view, &Map::new()).await, Err(VmError::NoSuchView(_)))
}

/// A panic in `onOpen` or in the first render fails the open and leaves no half-opened instance behind; a panic
/// in an event handler fails that event, and the window answers the next one.
#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_a_window_fails_that_open_or_event_and_the_window_answers_on() {
    let vm = windows_app().await;
    let explode = Map::from_iter([("explode".to_owned(), json!(true))]);
    assert!(matches!(vm.open_view("v1", "main", explode, Map::new()).await, Err(VmError::Panicked(_))));
    assert!(!is_open(&vm, "v1").await, "a failed onOpen leaves no instance");
    assert!(matches!(vm.open_view("v2", "broken", Map::new(), Map::new()).await, Err(VmError::Panicked(_))));
    assert!(!is_open(&vm, "v2").await, "a failed first render leaves no instance");

    let opened = vm.open_view("v3", "main", Map::new(), Map::new()).await.unwrap();
    assert!(matches!(
        vm.dispatch_view_event("v3", tap(&opened.tree, "boom"), &user_ctx(0)).await,
        Err(VmError::Panicked(_))
    ));
    assert!(vm.dispatch_view_event("v3", tap(&opened.tree, "add"), &user_ctx(0)).await.unwrap());
    let after = vm.render_view("v3").await.unwrap();
    let n = after.tree.containers().flat_map(|(_, c)| c).find(|c| c.key == "n").unwrap();
    assert_eq!(n.props["text"], json!("1"), "the event after the panic ran");
}
