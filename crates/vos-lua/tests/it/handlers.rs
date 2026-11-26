#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! The backend of an app: the handlers it serves on the bus (`RPC.on_request(method, fn)`) are what a client's
//! `rpc(method, params)` calls, as `fn(params, ctx)`, in that app's VM; `RPC.send_notification` sends an event to
//! the app's open clients.
//! Real Lua adapter, real SQLite adapter.

use crate::common;

use common::{db, id, user_ctx};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use vos_application::{
    ports::{AgentCard, AgentRoster, AppData, AppPackage, AppRuntime, Notifier, PortResult, Shell},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

struct Rig {
    runtime: LuaRuntime,
    shell: Arc<RecordingShell>,
}

fn rig() -> Rig {
    let shell = Arc::new(RecordingShell::default());
    Rig { runtime: LuaRuntime::new(Arc::new(Quiet), shell.clone()), shell }
}

/// A package whose entry is the backend `src`.
fn backend(src: &str) -> AppPackage {
    AppPackage {
        entry: "app.luau".into(),
        files: BTreeMap::from([("app.luau".to_owned(), src.as_bytes().to_vec())]),
    }
}

async fn load(rig: &Rig, name: &str, src: &str) -> PortResult<vos_application::ports::LoadedApp> {
    let data: Arc<dyn AppData> = db();
    rig.runtime.load(&id(name), backend(src), data).await
}

async fn call(rig: &Rig, name: &str, method: &str, args: Value) -> PortResult<Value> {
    rig.runtime.call_handler(&id(name), method, args, user_ctx(1_000)).await
}

const TASKS: &str = r#"
local db = app.db
db:migrate { [1] = "create table tasks(id integer primary key, title text not null)" }
RPC.on_request("add", function(args, ctx)
  db:exec("insert into tasks(title) values (:t)", { t = args.title })
  return db:all("select id, title from tasks order by id")
end)
RPC.on_request("who", function(args, ctx)
  return { kind = ctx.actor.kind, name = ctx.actor.name, now = ctx.now, args = args }
end)
RPC.on_request("nothing", function() end)
RPC.on_request("fail", function(_, ctx) return ctx:fail("A task needs a title.") end)
RPC.on_request("explode", function() error("boom") end)
RPC.on_request("coded", function() error("RPC-32001 not today", 0) end)
RPC.on_request("notcoded", function() error("RPC-32001: not today", 0) end)
RPC.on_request("zero", function() error("RPC0 nothing", 0) end)
RPC.on_request("raise_table", function() error({ code = 7 }) end)
RPC.on_request("raise_function", function() error(function() end) end)
RPC.on_request("loop", function() local t = {} t.t = t return t end)
"#;

#[tokio::test]
async fn a_client_call_runs_the_handler_with_a_context_and_the_arguments() {
    let rig = rig();
    load(&rig, "tasks", TASKS).await.unwrap();
    // the app's handlers are the ones it served on the bus, and a handler that said nothing of itself says nothing
    let handlers = rig.runtime.handlers(&id("tasks"));
    let names: Vec<_> = handlers.iter().map(|h| h.method.as_str()).collect();
    assert!(names.contains(&"add") && names.contains(&"who") && !names.contains(&"nope"), "{names:?}");
    assert!(handlers.iter().all(|h| h.title.is_none() && h.description.is_none() && !h.read_only));

    let added = call(&rig, "tasks", "add", json!({ "title": "Buy milk" })).await.unwrap();
    assert_eq!(
        added,
        json!([{ "id": 1, "title": "Buy milk" }]),
        "a handler may write: it is the person's own client"
    );

    let who = call(&rig, "tasks", "who", json!({ "n": 7 })).await.unwrap();
    assert_eq!(who["kind"], "user");
    assert_eq!(who["name"], "You");
    assert_eq!(who["args"], json!({ "n": 7 }));
    assert!(who["now"].as_str().unwrap().starts_with("1970-01-01T00:00:01"), "{who}");

    // no arguments is nil, and a handler that returns nothing answers null
    assert_eq!(call(&rig, "tasks", "nothing", Value::Null).await.unwrap(), Value::Null);
}

#[tokio::test]
async fn a_handler_that_fails_answers_with_its_message() {
    let rig = rig();
    load(&rig, "tasks", TASKS).await.unwrap();

    let failed = call(&rig, "tasks", "fail", Value::Null).await.unwrap_err();
    assert_eq!(failed, PortError::Rejected("A task needs a title.".into()), "ctx:fail is a plain message");

    let PortError::Rejected(raised) = call(&rig, "tasks", "explode", Value::Null).await.unwrap_err() else {
        panic!("a Luau error is a rejected call");
    };
    assert!(raised.contains("app.luau") && raised.contains("boom"), "{raised}");

    // a result that cannot be JSON is an error, not a hang or a panic
    let PortError::Rejected(unconvertible) = call(&rig, "tasks", "loop", Value::Null).await.unwrap_err()
    else {
        panic!("a cyclic result is a rejected call");
    };
    assert!(unconvertible.contains("contains itself"), "{unconvertible}");

    // and the app goes on
    assert_eq!(call(&rig, "tasks", "nothing", Value::Null).await.unwrap(), Value::Null);
}

#[tokio::test]
async fn an_error_that_starts_with_rpc_and_a_code_reaches_the_caller_as_that_json_rpc_error() {
    let rig = rig();
    load(&rig, "tasks", TASKS).await.unwrap();
    assert_eq!(
        call(&rig, "tasks", "coded", Value::Null).await.unwrap_err(),
        PortError::Rpc { code: -32001, message: "not today".into(), data: None }
    );
    // not a code (no space after the number, or a zero): an ordinary error of the app, with its location
    for handler in ["notcoded", "zero"] {
        let PortError::Rejected(message) = call(&rig, "tasks", handler, Value::Null).await.unwrap_err()
        else {
            panic!("{handler}: rejected");
        };
        assert!(message.contains("RPC"), "{handler}: {message}");
    }
}

#[tokio::test]
async fn a_call_for_a_handler_that_does_not_exist_says_so() {
    let rig = rig();
    load(&rig, "tasks", TASKS).await.unwrap();
    assert_eq!(
        call(&rig, "tasks", "nope", Value::Null).await.unwrap_err(),
        PortError::Rpc { code: -32601, message: "Method not found: nope".into(), data: None }
    );
    // a name no handler can have is the same answer, whatever its length
    assert!(call(&rig, "tasks", &"x".repeat(10_000), Value::Null).await.is_err());
}

#[tokio::test]
async fn a_call_for_an_app_that_is_not_loaded_is_not_found() {
    let rig = rig();
    assert!(matches!(call(&rig, "tasks", "add", Value::Null).await, Err(PortError::NotFound(_))));
}

#[tokio::test]
async fn each_app_answers_from_its_own_vm() {
    let rig = rig();
    load(&rig, "alpha", r#"local mine = "alpha" RPC.on_request("whose", function() return mine end)"#)
        .await
        .unwrap();
    load(&rig, "beta", r#"local mine = "beta" RPC.on_request("whose", function() return mine end)"#)
        .await
        .unwrap();
    assert_eq!(call(&rig, "alpha", "whose", Value::Null).await.unwrap(), json!("alpha"));
    assert_eq!(call(&rig, "beta", "whose", Value::Null).await.unwrap(), json!("beta"));
}

const EMITTER: &str = r#"
local task = require("std:task")
RPC.on_request("poke", function() RPC.send_notification("poked") return "ok" end)
RPC.on_request("peek", function() RPC.send_notification("peeked") return "ok" end, { read_only = true })
RPC.on_request("save", function()
  RPC.send_notification("tasks.changed", { n = 1 })
  RPC.send_notification("plain")
  return "saved"
end)
RPC.on_request("later", function()
  task.spawn(function()
    task.wait(0.05)
    RPC.send_notification("from.task", { late = true })
  end)
  return "started"
end)
RPC.on_request("bad_name", function() RPC.send_notification("not a name") end)
RPC.on_request("host_name", function() RPC.send_notification("host:theme", "dark") end)
RPC.on_request("huge", function() RPC.send_notification("big", { text = string.rep("x", 70000) }) end)
RPC.on_request("flood", function() for i = 1, 300 do RPC.send_notification("n", i) end end)
"#;

fn events(rig: &Rig) -> Vec<(String, String, Value)> {
    rig.shell.events.lock().unwrap().clone()
}

#[tokio::test]
async fn emit_sends_an_event_to_the_apps_clients_after_the_call() {
    let rig = rig();
    load(&rig, "tasks", EMITTER).await.unwrap();
    call(&rig, "tasks", "save", Value::Null).await.unwrap();
    assert_eq!(
        events(&rig),
        vec![
            ("tasks".into(), "tasks.changed".into(), json!({ "n": 1 })),
            ("tasks".into(), "plain".into(), Value::Null)
        ]
    );
}

#[tokio::test]
async fn an_agents_tool_can_emit_and_a_read_only_tool_cannot() {
    let rig = rig();
    load(&rig, "tasks", EMITTER).await.unwrap();
    let agent = common::agent_ctx(1);
    common::serve_rt(&rig.runtime, &id("tasks"), "poke", json!({}), agent.clone()).await.unwrap();
    assert_eq!(events(&rig), vec![("tasks".into(), "poked".into(), Value::Null)]);

    let PortError::Rejected(message) =
        common::serve_rt(&rig.runtime, &id("tasks"), "peek", json!({}), agent).await.unwrap_err()
    else {
        panic!("rejected");
    };
    assert!(message.contains("is not allowed in a tool declared readOnly"), "{message}");
    assert_eq!(events(&rig).len(), 1, "nothing was sent");
}

#[tokio::test]
async fn emit_refuses_a_bad_name_the_hosts_own_names_big_data_and_a_flood() {
    let rig = rig();
    load(&rig, "tasks", EMITTER).await.unwrap();
    for (handler, why) in [
        ("bad_name", "name is 1 to 64"),
        ("host_name", "`host:`"),
        ("huge", "65536 bytes"),
        ("flood", "256 events"),
    ] {
        let PortError::Rejected(message) = call(&rig, "tasks", handler, Value::Null).await.unwrap_err()
        else {
            panic!("{handler}: rejected");
        };
        assert!(message.contains(why), "{handler}: {message}");
    }
}

#[tokio::test]
async fn a_handler_that_raises_a_value_that_is_not_a_message_never_sends_its_address() {
    let rig = rig();
    load(&rig, "tasks", TASKS).await.unwrap();
    for handler in ["raise_table", "raise_function"] {
        let PortError::Rejected(message) = call(&rig, "tasks", handler, Value::Null).await.unwrap_err()
        else {
            panic!("{handler}: rejected");
        };
        assert!(message.ends_with("the script raised a value that is not a message"), "{handler}: {message}");
    }
}

#[tokio::test]
async fn a_background_task_can_emit_after_the_call_that_started_it() {
    let rig = rig();
    load(&rig, "tasks", EMITTER).await.unwrap();
    assert_eq!(call(&rig, "tasks", "later", Value::Null).await.unwrap(), json!("started"));
    assert!(events(&rig).is_empty(), "the task has not run yet");
    for _ in 0..200 {
        if !events(&rig).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(events(&rig), vec![("tasks".into(), "from.task".into(), json!({ "late": true }))]);
}

#[tokio::test]
async fn handlers_stop_with_the_app() {
    let rig = rig();
    load(&rig, "tasks", TASKS).await.unwrap();
    rig.runtime.unload(&id("tasks")).await.unwrap();
    assert!(matches!(call(&rig, "tasks", "nothing", Value::Null).await, Err(PortError::NotFound(_))));
}

/// A handler that waits for ever (a long poll, a LAN service that is down): the host holds the client's request until the
/// handler answers, so what happens to the request when the app goes away decides whether a connection keeps a permit for it.
const WAITER: &str = r#"
local task = require("std:task")
RPC.on_request("hang", function() while true do task.wait(0.05) end end)
RPC.on_request("ping", function() return "pong" end)
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uninstalling_an_app_ends_the_calls_its_handlers_still_hold() {
    let rig = Arc::new(rig());
    load(&rig, "waiter", WAITER).await.unwrap();
    let held: Vec<_> = (0..3)
        .map(|_| {
            let rig = rig.clone();
            tokio::spawn(async move { call(&rig, "waiter", "hang", Value::Null).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(held.iter().all(|h| !h.is_finished()), "the handlers wait");
    // another call is answered meanwhile: a waiting handler lends the app's turn
    assert_eq!(call(&rig, "waiter", "ping", Value::Null).await.unwrap(), json!("pong"));

    rig.runtime.unload(&id("waiter")).await.unwrap();
    for h in held {
        let ended = tokio::time::timeout(Duration::from_secs(5), h).await;
        let outcome = ended.expect("a call the app held ends when the app is unloaded").unwrap();
        assert!(outcome.is_err(), "it ends with an error, not a result: {outcome:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacing_an_app_ends_the_calls_the_old_version_still_holds() {
    let rig = Arc::new(rig());
    load(&rig, "waiter", WAITER).await.unwrap();
    let held = {
        let rig = rig.clone();
        tokio::spawn(async move { call(&rig, "waiter", "hang", Value::Null).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    load(&rig, "waiter", WAITER).await.unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(5), held).await;
    assert!(ended.expect("a call the old version held ends when it is replaced").unwrap().is_err());
    assert_eq!(call(&rig, "waiter", "ping", Value::Null).await.unwrap(), json!("pong"));
}

// ---- a panic in the host ------------------------------------------------------------------------------------

/// The agent roster behind `app.agents:list()`: a port the app API calls, here one that panics.
struct PanickingRoster;
impl AgentRoster for PanickingRoster {
    fn cards(&self) -> Vec<AgentCard> {
        panic!("the roster broke")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_a_handler_fails_that_call_and_the_app_answers_the_next() {
    let shell = Arc::new(RecordingShell::default());
    let runtime = LuaRuntime::new(Arc::new(Quiet), shell).with_roster(Arc::new(PanickingRoster));
    let data: Arc<dyn AppData> = db();
    let src = r#"
        RPC.on_request("boom", function() return app.agents:list() end)
        RPC.on_request("fine", function() return "still here" end)
    "#;
    runtime.load(&id("tasks"), backend(src), data).await.unwrap();
    let app = id("tasks");
    let call = |method: &'static str| runtime.call_handler(&app, method, Value::Null, user_ctx(1_000));

    match call("boom").await {
        Err(PortError::Internal(message)) => {
            assert_eq!(message, "a host function panicked: the roster broke");
        }
        other => panic!("expected an internal error, got {other:?}"),
    }
    assert_eq!(
        call("fine").await.unwrap(),
        json!("still here"),
        "the app stays loaded and answers the next call"
    );
    assert!(matches!(call("boom").await, Err(PortError::Internal(_))), "and it fails the same way again");
}

/// A shell whose first `emit_event` panics, then records what it is given.
#[derive(Default)]
struct PanicsOnFirstEmit {
    panicked: std::sync::atomic::AtomicBool,
    names: parking_lot::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl Shell for PanicsOnFirstEmit {
    async fn open_window(&self, _: &vos_domain::AppId, _: &str, _: serde_json::Map<String, Value>) {}
    async fn emit_event(&self, _: &vos_domain::AppId, name: &str, _: Value) {
        assert!(self.panicked.swap(true, std::sync::atomic::Ordering::SeqCst), "the shell broke");
        self.names.lock().push(name.to_owned());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_on_one_emit_of_a_batch_leaves_the_others_delivered_once_each() {
    let shell = Arc::new(PanicsOnFirstEmit::default());
    let runtime = LuaRuntime::new(Arc::new(Quiet), shell.clone());
    let data: Arc<dyn AppData> = db();
    let src = r#"
        RPC.on_request("batch", function()
          RPC.send_notification("first")
          RPC.send_notification("second")
          RPC.send_notification("third")
          return "sent"
        end)
    "#;
    runtime.load(&id("tasks"), backend(src), data).await.unwrap();
    let answer = runtime.call_handler(&id("tasks"), "batch", Value::Null, user_ctx(1_000)).await.unwrap();
    assert_eq!(answer, json!("sent"), "the call itself is not failed by a shell that broke on an event");
    for _ in 0..200 {
        if shell.names.lock().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(shell.panicked.load(std::sync::atomic::Ordering::SeqCst), "the first event panicked");
    assert_eq!(*shell.names.lock(), ["second", "third"], "the rest of the batch went out, once each");
}

const OPENER: &str = r#"
local app = require("vos:app")
RPC.on_request("open", function() app.open_window() return {} end, { description = "open" })
RPC.on_request("open_main", function() app.open_window({ tab = "all" }) return {} end, { description = "open main" })
RPC.on_request("open_other", function() app.open_window("settings") return {} end, { description = "open another" })
RPC.on_request("peek", function() app.open_window() return {} end, { description = "read only", read_only = true })
RPC.on_request("show", function() app.open_window() return "ok" end)
"#;

fn windows(rig: &Rig) -> Vec<(String, String)> {
    rig.shell.windows.lock().unwrap().iter().map(|(app, window, _)| (app.clone(), window.clone())).collect()
}

#[tokio::test]
async fn an_app_with_a_client_opens_its_one_window_without_naming_it() {
    let rig = rig();
    load(&rig, "calc", OPENER).await.unwrap();
    let agent = common::agent_ctx(1);
    common::serve_rt(&rig.runtime, &id("calc"), "open", json!({}), agent.clone()).await.unwrap();
    common::serve_rt(&rig.runtime, &id("calc"), "open_main", json!({}), agent.clone()).await.unwrap();
    // a handler may ask too (the person's own page calling its backend)
    call(&rig, "calc", "show", Value::Null).await.unwrap();
    // the shell is asked for the app's window, `main`, each time: opening it, or bringing it forward when it is open
    assert_eq!(
        windows(&rig),
        [("calc".into(), "main".into()), ("calc".into(), "main".into()), ("calc".into(), "main".into())]
    );

    // any other name is not a window this app has
    let PortError::Rejected(message) =
        common::serve_rt(&rig.runtime, &id("calc"), "open_other", json!({}), agent.clone())
            .await
            .unwrap_err()
    else {
        panic!("rejected");
    };
    assert!(message.contains("an app has one window: it takes no name"), "{message}");
    // nor may a read-only tool ask
    assert!(common::serve_rt(&rig.runtime, &id("calc"), "peek", json!({}), agent).await.is_err());
    assert_eq!(windows(&rig).len(), 3, "nothing more was asked");
}
