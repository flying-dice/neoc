#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! The backend of an app: the table its entry returns is the set of handlers a client's `rpc(name,
//! args)` calls, as `handler(ctx, args)`, in that app's VM; `app.emit` sends an event to the app's open clients.
//! Real Lua adapter, real SQLite adapter.

mod common;

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
return {
  add = function(ctx, args)
    db:exec("insert into tasks(title) values (:t)", { t = args.title })
    return db:all("select id, title from tasks order by id")
  end,
  who = function(ctx, args) return { kind = ctx.actor.kind, name = ctx.actor.name, now = ctx.now, args = args } end,
  nothing = function() end,
  fail = function(ctx) return ctx:fail("A task needs a title.") end,
  explode = function() error("boom") end,
  coded = function() error("RPC-32001 not today", 0) end,
  notcoded = function() error("RPC-32001: not today", 0) end,
  zero = function() error("RPC0 nothing", 0) end,
  raise_table = function() error({ code = 7 }) end,
  raise_function = function() error(function() end) end,
  loop = function() local t = {} t.t = t return t end,
}
"#;

#[tokio::test]
async fn a_client_call_runs_the_handler_with_a_context_and_the_arguments() {
    let rig = rig();
    let loaded = load(&rig, "tasks", TASKS).await.unwrap();
    assert!(loaded.tools.is_empty(), "handlers are not tools: an agent does not see them");

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
    // a tool is not a handler
    let tool = r#"app.tool { name = "t", run = function() return 1 end } return {}"#;
    load(&rig, "tools", tool).await.unwrap();
    assert!(call(&rig, "tools", "t", Value::Null).await.is_err());
}

#[tokio::test]
async fn a_call_for_an_app_that_is_not_loaded_is_not_found() {
    let rig = rig();
    assert!(matches!(call(&rig, "tasks", "add", Value::Null).await, Err(PortError::NotFound(_))));
}

#[tokio::test]
async fn each_app_answers_from_its_own_vm() {
    let rig = rig();
    load(&rig, "alpha", r#"local mine = "alpha" return { whose = function() return mine end }"#)
        .await
        .unwrap();
    load(&rig, "beta", r#"local mine = "beta" return { whose = function() return mine end }"#).await.unwrap();
    assert_eq!(call(&rig, "alpha", "whose", Value::Null).await.unwrap(), json!("alpha"));
    assert_eq!(call(&rig, "beta", "whose", Value::Null).await.unwrap(), json!("beta"));
}

#[tokio::test]
async fn an_entry_that_returns_the_wrong_thing_does_not_load() {
    let rig = rig();
    for (src, why) in [
        ("return 5", "it returned a number"),
        ("return { 1, 2 }", "a key is a number, not a name"),
        ("return { list = 5 }", "`list` is a number, not a function"),
        ("return { [''] = function() end }", "a handler name is 1 to 128 characters"),
    ] {
        let err = load(&rig, "bad", src).await.unwrap_err().to_string();
        assert!(err.contains(why), "{src}: {err}");
    }
    // nothing at all is not a backend either: an app with nothing for its page to call returns `{}`
    let err = load(&rig, "bad", "-- no backend").await.unwrap_err().to_string();
    assert!(
        err.contains("the entry must return the app's handlers") && err.contains("it returned nothing"),
        "{err}"
    );
    load(&rig, "bad", "return {}").await.unwrap();
}

const EMITTER: &str = r#"
local task = require("std:task")
app.tool { name = "poke", run = function() app.emit("poked") return "ok" end }
app.tool { name = "peek", readOnly = true, run = function() app.emit("peeked") return "ok" end }
return {
  save = function(ctx)
    app.emit("tasks.changed", { n = 1 })
    app.emit("plain")
    return "saved"
  end,
  later = function(ctx)
    task.spawn(function()
      task.wait(0.05)
      app.emit("from.task", { late = true })
    end)
    return "started"
  end,
  bad_name = function(ctx) app.emit("not a name") end,
  host_name = function(ctx) app.emit("host:theme", "dark") end,
  huge = function(ctx) app.emit("big", { text = string.rep("x", 70000) }) end,
  flood = function(ctx) for i = 1, 300 do app.emit("n", i) end end,
}
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
    rig.runtime.call_tool(&id("tasks"), "poke", json!({}), agent.clone(), false).await.unwrap();
    assert_eq!(events(&rig), vec![("tasks".into(), "poked".into(), Value::Null)]);

    let PortError::Rejected(message) =
        rig.runtime.call_tool(&id("tasks"), "peek", json!({}), agent, false).await.unwrap_err()
    else {
        panic!("rejected");
    };
    assert!(message.contains("app.emit is not allowed in a tool declared readOnly"), "{message}");
    assert_eq!(events(&rig).len(), 1, "nothing was sent");
}

#[tokio::test]
async fn emit_refuses_a_bad_name_the_hosts_own_names_big_data_and_a_flood() {
    let rig = rig();
    load(&rig, "tasks", EMITTER).await.unwrap();
    for (handler, why) in [
        ("bad_name", "an event name is 1 to 64"),
        ("host_name", "names starting with `host:` are the host's own"),
        ("huge", "at most 65536 bytes"),
        ("flood", "256 events are already waiting"),
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
return {
  hang = function() while true do task.wait(0.05) end end,
  ping = function() return "pong" end,
}
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
        return {
          boom = function() return app.agents:list() end,
          fine = function() return "still here" end,
        }
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
        return {
          batch = function()
            app.emit("first")
            app.emit("second")
            app.emit("third")
            return "sent"
          end,
        }
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
app.tool { name = "open", description = "open", input = {}, run = function() app:openWindow() return {} end }
app.tool { name = "open_main", description = "open main", input = {}, run = function() app:openWindow("main") return {} end }
app.tool { name = "open_other", description = "open another", input = {}, run = function() app:openWindow("settings") return {} end }
app.tool { name = "peek", description = "read only", readOnly = true, input = {}, run = function() app:openWindow() return {} end }
return {
  show = function() app:openWindow() return "ok" end,
}
"#;

fn windows(rig: &Rig) -> Vec<(String, String)> {
    rig.shell.windows.lock().unwrap().iter().map(|(app, window, _)| (app.clone(), window.clone())).collect()
}

#[tokio::test]
async fn an_app_with_a_client_opens_its_one_window_without_naming_it() {
    let rig = rig();
    load(&rig, "calc", OPENER).await.unwrap();
    let agent = common::agent_ctx(1);
    rig.runtime.call_tool(&id("calc"), "open", json!({}), agent.clone(), false).await.unwrap();
    rig.runtime.call_tool(&id("calc"), "open_main", json!({}), agent.clone(), false).await.unwrap();
    // a handler may ask too (the person's own page calling its backend)
    call(&rig, "calc", "show", Value::Null).await.unwrap();
    // the shell is asked for the app's window, `main`, each time: opening it, or bringing it forward when it is open
    assert_eq!(
        windows(&rig),
        [("calc".into(), "main".into()), ("calc".into(), "main".into()), ("calc".into(), "main".into())]
    );

    // any other name is not a window this app has
    let PortError::Rejected(message) =
        rig.runtime.call_tool(&id("calc"), "open_other", json!({}), agent.clone(), false).await.unwrap_err()
    else {
        panic!("rejected");
    };
    assert!(message.contains("an app has one window, `main`"), "{message}");
    // nor may a read-only tool ask
    assert!(rig.runtime.call_tool(&id("calc"), "peek", json!({}), agent, false).await.is_err());
    assert_eq!(windows(&rig).len(), 3, "nothing more was asked");
}
