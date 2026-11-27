#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The backend's side of the RPC bus, over the real Lua adapter with no bus bound (the runtime is its own bus):
//! `RPC.send_request` reaches the handler another app serves (`RPC.on_request`), told which app
//! called; `RPC.on_notification` hears an app's notifications, told which app sent them; a `_name` never leaves its app.

use crate::common;

use common::{db, id, package, user_ctx};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    ports::{AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

/// An app with a list, serving `add` on the bus: it announces `added` (public) and `_saved` (private), and hears its own
/// `_saved`.
const LEDGER: &str = r#"
local items = {}
local saved = 0
local function who(ctx) return ctx.actor.kind .. ":" .. ctx.actor.id end
RPC.on_request("add", function(p, ctx)
  table.insert(items, p.text)
  RPC.send_notification("added", { text = p.text, by = who(ctx) })
  RPC.send_notification("_saved", { n = #items })
  return { n = #items, by = who(ctx) }
end)
RPC.on_request("fail", function(_, ctx) return ctx:fail("the ledger says no") end)
RPC.on_request("saved", function() return saved end)
RPC.on_notification("ledger", "_saved", function(p) saved = p.n end)
"#;

/// An app that asks the ledger for things and hears what it adds.
const WATCHER: &str = r#"
local heard = {}
RPC.on_notification("ledger", "added", function(p, ctx)
  table.insert(heard, ctx.actor.kind .. ":" .. ctx.actor.id .. " " .. p.text .. " by " .. p.by)
  RPC.send_notification("heard", { n = #heard })
end)
local function ask(method, params)
  local result, err = RPC.send_request("ledger", method, params)
  if err then return { err = err.kind } end
  return result
end
RPC.on_request("relay", function(p) return ask("add", { text = p.text }) end)
RPC.on_request("missing", function() return ask("nope", {}) end)
RPC.on_request("failing", function() return ask("fail", {}) end)
RPC.on_request("ghost", function()
  local _, err = RPC.send_request("ghost", "add", {})
  return { err = err and err.kind }
end)
RPC.on_request("heard", function() return heard end)
"#;

struct NoNotifier;
#[async_trait::async_trait]
impl Notifier for NoNotifier {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

async fn rig() -> (LuaRuntime, Arc<RecordingShell>) {
    let shell = Arc::new(RecordingShell::default());
    let runtime = LuaRuntime::new(Arc::new(NoNotifier), shell.clone());
    for (name, src) in [("ledger", LEDGER), ("watcher", WATCHER)] {
        let data: Arc<dyn AppData> = db();
        runtime.load(&id(name), package(src, &[]), data).await.unwrap();
    }
    (runtime, shell)
}

async fn call(runtime: &LuaRuntime, app: &str, method: &str, args: Value) -> Value {
    runtime.call_handler(&id(app), method, args, user_ctx(1_000)).await.unwrap()
}

#[tokio::test]
async fn a_handler_registered_on_the_bus_answers_and_is_told_who_asked() {
    let (runtime, _) = rig().await;
    assert!(runtime.has_handler(&id("ledger"), "add") && !runtime.has_handler(&id("ledger"), "nope"));
    assert_eq!(
        call(&runtime, "watcher", "relay", json!({"text": "milk"})).await,
        json!({"n": 1, "by": "app:watcher"})
    );
    assert_eq!(
        call(&runtime, "ledger", "add", json!({"text": "eggs"})).await,
        json!({"n": 2, "by": "user:user"})
    );
}

#[tokio::test]
async fn listeners_hear_notifications_whoever_caused_them_and_a_private_one_stays_home() {
    let (runtime, shell) = rig().await;
    call(&runtime, "ledger", "add", json!({"text": "milk"})).await;
    call(&runtime, "watcher", "relay", json!({"text": "eggs"})).await;
    assert_eq!(
        call(&runtime, "watcher", "heard", json!({})).await,
        json!(["app:ledger milk by user:user", "app:ledger eggs by app:watcher"])
    );
    // the ledger heard its own private notification
    assert_eq!(call(&runtime, "ledger", "saved", json!({})).await, json!(2));
    // every notification reached the pages through the shell: the listener's own, and the ledger's private one
    let names: Vec<(String, String)> =
        shell.events.lock().unwrap().iter().map(|(app, name, _)| (app.clone(), name.clone())).collect();
    assert!(names.contains(&("watcher".into(), "heard".into())), "{names:?}");
    assert!(names.contains(&("ledger".into(), "_saved".into())), "{names:?}");
}

#[tokio::test]
async fn a_notification_from_the_page_reaches_the_backend_listeners() {
    let (runtime, _) = rig().await;
    runtime.deliver_notification(&id("ledger"), "_saved", json!({"n": 41})).await.unwrap();
    assert_eq!(call(&runtime, "ledger", "saved", json!({})).await, json!(41));
}

#[tokio::test]
async fn a_request_that_cannot_be_answered_is_an_error_value_that_says_why() {
    let (runtime, _) = rig().await;
    assert_eq!(call(&runtime, "watcher", "missing", json!({})).await, json!({"err": "not_found"}));
    assert_eq!(call(&runtime, "watcher", "failing", json!({})).await, json!({"err": "failed"}));
    assert_eq!(call(&runtime, "watcher", "ghost", json!({})).await, json!({"err": "not_found"}));
}

#[tokio::test]
async fn another_apps_private_notification_and_a_second_handler_are_refused() {
    let runtime = LuaRuntime::new(Arc::new(NoNotifier), Arc::new(RecordingShell::default()));
    for (src, expected) in [
        (r#"RPC.on_notification("ledger", "_saved", function() end)"#, "never leaves ledger"),
        (r#"RPC.on_notification("Not An Id", "added", function() end)"#, "is not an app id"),
        (
            r#"RPC.on_request("a", function() end) RPC.on_request("a", function() end)"#,
            "already has a handler",
        ),
    ] {
        let data: Arc<dyn AppData> = db();
        let e = runtime.load(&id("spy"), package(src, &[]), data).await.unwrap_err();
        assert!(matches!(&e, PortError::Rejected(m) if m.contains(expected)), "{e:?}");
    }
}
