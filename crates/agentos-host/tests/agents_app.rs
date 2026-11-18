#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The Agents app (`apps/agents`, Luau) on the real host: its window lists the roster and the chats, starts a
//! chat, binds `ChatTranscript` to a thread source, and titles the chat from the first message.

mod common;

use agentos_domain::{Grants, Permission};
use common::{
    agents::launchable, browser, next_notification, notification_where, path_of, publish_package, rpc, start,
    temp, Running, Ws,
};
use serde_json::{json, Value};
use std::path::Path;

async fn host_with_agents_app() -> Running {
    let mut fake = launchable("fake", "Fake", Grants::default());
    fake.role = "A scripted agent".into();
    fake.avatar = Some("bolt".into());
    let r = start(|c| c.agents = vec![("tok-fake".into(), fake)]).await;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/agents/app.luau");
    let url = publish_package(
        &temp("agents-app").join("pkg"),
        "agents",
        "app.luau",
        &[("app.luau", std::fs::read(source).unwrap())],
        vec![Permission::Data, Permission::Agents],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    r
}

async fn open(ws: &mut Ws) -> Value {
    let hello =
        rpc(ws, 1, "session.hello", json!({"proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC"}))
            .await;
    assert!(hello.get("error").is_none(), "{hello}");
    let reply = rpc(
        ws,
        2,
        "view.open",
        json!({"app": "agents", "window": "main", "windowId": "w1", "size": {"w": 960, "h": 640}}),
    )
    .await;
    assert!(reply.get("error").is_none(), "{reply}");
    reply["result"].clone()
}

async fn event(
    ws: &mut Ws,
    id: i64,
    view: &Value,
    seq: u32,
    key: &str,
    name: &str,
    payload: &Value,
) -> Value {
    let path = path_of(&view["tree"], key).unwrap_or_else(|| panic!("no node `{key}` in {}", view["tree"]));
    rpc(
        ws,
        id,
        "view.event",
        json!({"viewId": view["viewId"], "rev": view["rev"], "seq": seq, "path": path, "event": name, "payload": payload}),
    )
    .await
}

fn find<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    if node["key"] == key {
        return Some(node);
    }
    let children = node["children"].as_array().into_iter().flatten();
    let slots = node["slots"]
        .as_object()
        .into_iter()
        .flat_map(|s| s.values())
        .flat_map(|v| v.as_array().into_iter().flatten());
    children.chain(slots).find_map(|c| find(c, key))
}

#[tokio::test]
async fn the_window_lists_the_roster_and_binds_the_chat_to_a_thread_source() {
    let r = host_with_agents_app().await;
    let mut ws = browser(&r).await;
    let view = open(&mut ws).await;

    // the roster (not its grants or token) is in the sidebar
    let row = find(&view["tree"], "agent-fake").expect("the agent is listed");
    assert_eq!(row["props"]["title"], "Fake");
    assert_eq!(row["props"]["subtitle"], "A scripted agent");
    assert_eq!(row["props"]["icon"], "bolt");
    assert!(find(&view["tree"], "new-chat").is_some(), "with one agent, New chat starts a chat with it");

    // the chat is a ChatTranscript on a `thread` source whose params follow the selection
    let chat = find(&view["tree"], "chat").expect("the transcript");
    assert_eq!(chat["type"], "ChatTranscript");
    let source = chat["props"]["thread"].as_str().unwrap();
    let decl = &view["sources"][source];
    assert_eq!(decl["kind"], "thread");
    assert_eq!(decl["params"]["thread"], json!({"$ref": "client.threadId"}));
    assert_eq!(decl["params"]["agent"]["$ref"].as_str().unwrap().rsplit('.').next(), Some("agent_id"));
    assert!(view["data"].get(source).is_none(), "a thread's data is /agui, not a data.patch");
    // the thread source is declared once; the chat list and the open chat's row are live queries
    let kinds: Vec<&str> =
        view["sources"].as_object().unwrap().values().map(|d| d["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "thread").count(), 1);
    assert!(kinds.contains(&"rows") && kinds.contains(&"row"));

    // the client re-asks when the selection changes: the host answers quietly, with no data
    let answer = rpc(
        &mut ws,
        3,
        "source.set",
        json!({"viewId": view["viewId"], "source": source, "params": {"thread": "abc", "agent": "fake"}}),
    )
    .await;
    assert_eq!(answer["result"], json!({"kind": "value", "value": null}), "{answer}");
    r.stop().await;
}

#[tokio::test]
async fn starting_a_chat_adds_it_to_the_list_and_the_first_message_titles_it() {
    let r = host_with_agents_app().await;
    let mut ws = browser(&r).await;
    let view = open(&mut ws).await;

    let started = event(&mut ws, 3, &view, 1, "new-chat", "tap", &Value::Null).await;
    assert_eq!(started["result"], json!({"ok": true}), "{started}");
    let set = next_notification(&mut ws, "client.set").await;
    assert_eq!(set["path"], "threadId");
    let thread = set["value"].as_str().unwrap().to_owned();
    assert_eq!(thread.len(), 16, "a random id from the database");
    let list = notification_where(&mut ws, "data.patch", |p| {
        p["sources"]
            .as_object()
            .is_some_and(|s| s.values().any(|v| v["kind"] == "rows" && v["keys"] == json!([thread.clone()])))
    })
    .await;
    let rows = list["sources"].as_object().unwrap().values().find(|v| v["kind"] == "rows").unwrap();
    assert_eq!(rows["rows"][&thread]["title"], "New chat");
    assert_eq!(rows["rows"][&thread]["agent_name"], "Fake");

    // the person's first message names the chat; the handler reads the open chat from the mirrored client value
    common::notify(&mut ws, "client.state", json!({"viewId": view["viewId"], "set": {"threadId": thread}}))
        .await;
    let sent =
        event(&mut ws, 5, &view, 2, "chat", "send", &json!({"text": "  Plan my week please\nthanks"})).await;
    assert_eq!(sent["result"], json!({"ok": true}), "{sent}");
    let renamed = notification_where(&mut ws, "data.patch", |p| {
        p["sources"]
            .as_object()
            .is_some_and(|s| s.values().any(|v| v["rows"][&thread]["title"] == "Plan my week please"))
    })
    .await;
    assert!(renamed["sources"].is_object());

    // a later message moves the chat but keeps its name
    event(&mut ws, 6, &view, 3, "chat", "send", &json!({"text": "something else"})).await;
    let data = r.host.service.data_of(&agentos_domain::AppId::parse("agents").unwrap()).unwrap();
    let rows = data
        .query("select title from threads".into(), agentos_application::data::Params::None)
        .await
        .unwrap();
    assert_eq!(rows[0]["title"], "Plan my week please");
    r.stop().await;
}

#[tokio::test]
async fn an_app_without_the_agents_permission_cannot_list_the_roster() {
    let r = start(|c| {
        c.agents = vec![common::claude(), ("tok-fake".into(), launchable("fake", "Fake", Grants::default()))]
    })
    .await;
    let source = br#"app.manifest { id = "nosy", name = "Nosy", icon = "gear", permissions = {} }
app.tool { name = "peek", run = function() return app.agents:list() end }"#;
    let url = publish_package(
        &temp("nosy").join("pkg"),
        "nosy",
        "app.luau",
        &[("app.luau", source.to_vec())],
        vec![],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let (agent, _) = common::connect_agent(r.addr, "tok-claude").await;
    let out = common::call(&agent, "nosy__peek", json!({})).await;
    assert_eq!(out.is_error, Some(true));
    assert!(common::text_of(&out).contains("agents"), "{}", common::text_of(&out));
    r.stop().await;
}
