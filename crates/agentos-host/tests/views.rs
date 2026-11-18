#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The view protocol end to end, over the real adapters: a paired browser on `/rpc` opens the Notes and
//! Calculator windows of ui-system.md §4.3 and §4.4 (the spec's own apps, unchanged), types into them, and
//! an agent changes the same data through MCP tools while the windows are open.

mod common;

use agentos_domain::Permission;
use agentos_host::startup::prepare_data_dir;
use common::{
    browser, call, claude, connect_agent, next_notification, notification_where, notify, path_of,
    publish_package, rpc, start, temp, text_of, wait_until, Running, Ws,
};
use serde_json::{json, Value};
use std::path::Path;

fn fixture(app: &str, name: &str) -> (String, Vec<u8>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agentos-codegen/tests/fixtures").join(app);
    (name.to_owned(), std::fs::read(dir.join(name)).unwrap())
}

async fn host_with(tmp: &Path, apps: &[(&str, &[&str])]) -> Running {
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    })
    .await;
    for (app, files) in apps {
        let files: Vec<(String, Vec<u8>)> = files.iter().map(|f| fixture(app, f)).collect();
        let refs: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
        let url =
            publish_package(&tmp.join(format!("pkg-{app}")), app, "app.luau", &refs, vec![Permission::Data]);
        r.host.service.install_from_url(&url).await.unwrap();
    }
    r
}

async fn open(ws: &mut Ws, id: i64, app: &str) -> Value {
    let reply = rpc(
        ws,
        id,
        "view.open",
        json!({"app": app, "window": "main", "windowId": format!("w{id}"), "size": {"w": 960, "h": 640}}),
    )
    .await;
    assert!(reply.get("error").is_none(), "{reply}");
    reply["result"].clone()
}

async fn hello(ws: &mut Ws) {
    let reply =
        rpc(ws, 1, "session.hello", json!({"proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC"}))
            .await;
    assert_eq!(reply["result"]["proto"], 1, "{reply}");
    assert_eq!(reply["result"]["kit"], "1.0");
}

async fn tap(ws: &mut Ws, id: i64, view: &Value, seq: u32, key: &str) -> Value {
    let path = path_of(&view["tree"], key).unwrap_or_else(|| panic!("no node `{key}`"));
    rpc(
        ws,
        id,
        "view.event",
        json!({"viewId": view["viewId"], "rev": view["rev"], "seq": seq, "path": path, "event": "tap", "payload": null}),
    )
    .await
}

fn ops_setting(params: &Value, prop: &str, value: &Value) -> bool {
    params["ops"]
        .as_array()
        .is_some_and(|ops| ops.iter().any(|op| op["op"] == "set" && op["props"][prop] == *value))
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one journey, read top to bottom
async fn notes_the_human_types_an_agent_appends_and_the_open_view_follows() {
    let tmp = temp("notes-view");
    let r = host_with(&tmp, &[("notes", &["app.luau"])]).await;
    let mut ws = browser(&r).await;
    hello(&mut ws).await;

    // --- open: one round trip to a complete window
    let snapshot = open(&mut ws, 2, "notes").await;
    let view = snapshot["viewId"].clone();
    assert_eq!(snapshot["tree"]["key"], "main");
    assert_eq!(snapshot["proto"], 1);
    let sources: Vec<&String> = snapshot["sources"].as_object().unwrap().keys().collect();
    assert_eq!(sources, ["s1", "s2", "s3"]);
    assert_eq!(snapshot["sources"]["s3"]["writable"], json!(["body"]), "derived from the two-way binding");
    assert_eq!(snapshot["sources"]["s2"]["params"]["folder"], json!({"$ref": "client.folder"}));
    assert_eq!(snapshot["data"]["s1"]["kind"], "rows");
    assert_eq!(snapshot["data"]["s1"]["rows"]["1"]["name"], "Notes", "the seeded folder");
    assert_eq!(snapshot["data"]["s3"], json!({"kind": "row", "rev": 0, "row": null}), "no note selected yet");
    assert_eq!(snapshot["clientDecls"]["folder"]["persist"], "app");

    // --- New Note: the handler runs; the answer comes after everything it caused
    let reply = tap(&mut ws, 3, &snapshot, 1, "new-note").await;
    assert_eq!(reply["result"], json!({"ok": true}), "{reply}");
    let set = next_notification(&mut ws, "client.set").await;
    assert_eq!(
        (set["path"].as_str(), &set["value"]),
        (Some("noteId"), &json!(1)),
        "the app drove client state"
    );
    let patch = notification_where(&mut ws, "data.patch", |p| p["sources"]["s2"].is_object()).await;
    assert_eq!(patch["sources"]["s2"]["keys"], json!([1]));
    assert_eq!(patch["sources"]["s2"]["rows"]["1"]["title"], "New Note");

    // --- the client points the note source at it (its param is client.noteId) and mirrors the value
    notify(&mut ws, "client.state", json!({"viewId": view, "set": {"noteId": 1}})).await;
    let open_note =
        rpc(&mut ws, 4, "source.set", json!({"viewId": view, "source": "s3", "params": {"id": 1}})).await;
    assert_eq!(open_note["result"]["kind"], "row", "{open_note}");
    assert_eq!(open_note["result"]["rev"], 0);
    assert_eq!(open_note["result"]["row"]["author"], "You");

    // --- the human types: one debounced write with baseRev
    let ack = rpc(
        &mut ws,
        5,
        "row.set",
        json!({"viewId": view, "source": "s3", "key": 1, "field": "body", "value": "milk", "baseRev": 0}),
    )
    .await;
    assert_eq!(ack["result"]["resolution"], "accepted", "{ack}");
    assert_eq!((&ack["result"]["rev"], &ack["result"]["value"]), (&json!(1), &json!("milk")));
    let patch = notification_where(&mut ws, "data.patch", |p| p["sources"]["s3"]["rev"] == 1).await;
    assert_eq!(patch["sources"]["s3"]["row"]["body"], "milk");

    // --- an agent appends to the note over MCP while the window is open: no UI code anywhere
    let (agent, _) = connect_agent(r.addr, "tok-claude").await;
    let out = call(&agent, "notes__notes.append", json!({"id": 1, "text": "- buy oat milk"})).await;
    assert_ne!(out.is_error, Some(true), "{out:?}");
    let patch = notification_where(&mut ws, "data.patch", |p| p["sources"]["s3"]["rev"] == 2).await;
    assert_eq!(patch["sources"]["s3"]["row"]["body"], "milk\n- buy oat milk");
    assert_eq!(patch["sources"]["s3"]["row"]["author"], "Claude");
    let list = notification_where(&mut ws, "data.patch", |p| {
        p["sources"]["s2"]["rows"]["1"]["preview"] == "milk - buy oat milk"
    })
    .await;
    assert_eq!(list["sources"]["s2"]["keys"], json!([1]), "the list row updated too");

    // --- the human, who still has "milk" (rev 1), capitalises it: the host merges with the agent's line
    let ack = rpc(
        &mut ws,
        6,
        "row.set",
        json!({"viewId": view, "source": "s3", "key": 1, "field": "body", "value": "Milk", "baseRev": 1}),
    )
    .await;
    assert_eq!(ack["result"]["resolution"], "merged", "{ack}");
    assert_eq!(ack["result"]["value"], "Milk\n- buy oat milk");
    assert_eq!(ack["result"]["rev"], 3);

    // --- a write whose base the host no longer has: the human wins and can undo
    let ack = rpc(
        &mut ws,
        7,
        "row.set",
        json!({"viewId": view, "source": "s3", "key": 1, "field": "body", "value": "mine", "baseRev": 99}),
    )
    .await;
    assert_eq!(ack["result"]["resolution"], "replaced", "{ack}");
    assert_eq!(ack["result"]["conflict"], true);
    assert_eq!(ack["result"]["undo"]["lost"], "Milk\n- buy oat milk");
    let revision = ack["result"]["undo"]["revisionId"].clone();
    let undone = rpc(
        &mut ws,
        8,
        "row.restore",
        json!({"viewId": view, "source": "s3", "key": 1, "revisionId": revision}),
    )
    .await;
    assert_eq!(undone["result"]["value"], "Milk\n- buy oat milk", "{undone}");

    // --- a stale event, a replayed event, and bad requests
    let stale_ack = rpc(
        &mut ws,
        9,
        "view.event",
        json!({"viewId": view, "rev": 1, "seq": 9, "path": "main/nope", "event": "tap"}),
    )
    .await;
    assert_eq!(stale_ack["result"], json!({"ok": false, "reason": "stale"}));
    let duplicate = rpc(
        &mut ws,
        10,
        "view.event",
        json!({"viewId": view, "rev": 1, "seq": 1, "path": "main/x", "event": "tap"}),
    )
    .await;
    assert_eq!(duplicate["result"], json!({"ok": true}), "seq 1 was already handled: a duplicate is a no-op");
    let ghost = rpc(
        &mut ws,
        11,
        "view.event",
        json!({"viewId": "v999", "rev": 1, "seq": 1, "path": "main", "event": "tap"}),
    )
    .await;
    assert_eq!(ghost["error"]["code"], -32011, "{ghost}");
    let no_state_binding =
        rpc(&mut ws, 12, "state.set", json!({"viewId": view, "path": "p", "value": 1, "baseRev": 0})).await;
    assert_eq!(no_state_binding["error"]["code"], -32010);
    let bad = rpc(
        &mut ws,
        13,
        "view.open",
        json!({"app": "ghost", "window": "main", "windowId": "w", "size": {"w": 1, "h": 1}}),
    )
    .await;
    assert_eq!(bad["error"]["code"], -32602, "{bad}");
    let bad = rpc(
        &mut ws,
        14,
        "view.open",
        json!({"app": "notes", "window": "nope", "windowId": "w", "size": {"w": 1, "h": 1}}),
    )
    .await;
    assert_eq!(bad["error"]["code"], -32602, "{bad}");
    let old = rpc(
        &mut ws,
        15,
        "session.hello",
        json!({"proto": 0, "kit": "1.0", "locale": "en", "timeZone": "UTC"}),
    )
    .await;
    assert_eq!(old["error"]["code"], -32012);
    let malformed = rpc(&mut ws, 16, "view.event", json!({"viewId": 5})).await;
    assert_eq!(malformed["error"]["code"], -32602);

    // --- persisted client state: close the window, open another, the selection comes back from the host
    let closed = rpc(&mut ws, 17, "view.close", json!({"viewId": view})).await;
    assert!(closed.get("error").is_none(), "{closed}");
    let again = open(&mut ws, 18, "notes").await;
    assert_eq!(again["client"]["noteId"], 1, "`persist = app` came back from the host's copy");
    assert_eq!(again["data"]["s3"]["row"]["id"], 1, "and the note source was filled from it");
    assert_eq!(again["data"]["s3"]["row"]["body"], "Milk\n- buy oat milk");

    // --- disabling the app closes its windows
    let reply = rpc(&mut ws, 19, "app.disable", json!({"app": "notes"})).await;
    assert!(reply.get("error").is_none(), "{reply}");
    let closed = next_notification(&mut ws, "view.closed").await;
    assert_eq!(closed["viewId"], again["viewId"]);
    assert_eq!(closed["reason"], "disabled");
    assert_eq!(r.host.views.open_views(), 0);

    agent.cancel().await.unwrap();
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn the_calculator_answers_a_key_with_ops_before_the_ack_and_follows_the_agent() {
    let tmp = temp("calc-view");
    let r = host_with(&tmp, &[("calculator", &["app.luau", "calc.luau"])]).await;
    let mut ws = browser(&r).await;
    hello(&mut ws).await;
    let first = open(&mut ws, 2, "calculator").await;
    let second = open(&mut ws, 3, "calculator").await; // two windows, one calculator
    assert_eq!(first["sources"], json!({}));

    // press 7: the ops arrive before the answer, as one batch with the revision they are based on
    let reply = tap(&mut ws, 4, &first, 1, "key-7").await;
    assert_eq!(reply["result"], json!({"ok": true}));
    let ops = notification_where(&mut ws, "view.ops", |p| p["viewId"] == first["viewId"]).await;
    assert_eq!((&ops["base"], &ops["rev"]), (&first["rev"], &json!(first["rev"].as_u64().unwrap() + 1)));
    assert!(ops_setting(&ops, "text", &json!("7")), "{ops}");
    assert!(ws.backlog.iter().all(|m| m["method"] != "view.tree"), "a key press never resends the tree");
    // the other window shows the same calculator
    let other = notification_where(&mut ws, "view.ops", |p| p["viewId"] == second["viewId"]).await;
    assert!(ops_setting(&other, "text", &json!("7")));

    // 7 + 8 =
    for (n, key) in ["key-add", "key-8", "key-eq"].into_iter().enumerate() {
        let reply =
            tap(&mut ws, 10 + i64::try_from(n).unwrap(), &first, 2 + u32::try_from(n).unwrap(), key).await;
        assert_eq!(reply["result"], json!({"ok": true}), "{reply}");
    }
    notification_where(&mut ws, "view.ops", |p| {
        p["viewId"] == first["viewId"] && ops_setting(p, "text", &json!("15"))
    })
    .await;

    // an agent shows a result on the calculator: both windows re-render by themselves
    let (agent, _) = connect_agent(r.addr, "tok-claude").await;
    let out = call(&agent, "calculator__show_on_calculator", json!({"expression": "6 * 7"})).await;
    assert_ne!(out.is_error, Some(true), "{out:?}");
    assert!(text_of(&out).contains("42"));
    for window in [&first, &second] {
        notification_where(&mut ws, "view.ops", |p| {
            p["viewId"] == window["viewId"] && ops_setting(p, "text", &json!("42"))
        })
        .await;
    }
    let sum = call(&agent, "calculator__calculate", json!({"expression": "(12.5 + 7) * 3"})).await;
    assert!(text_of(&sum).contains("58.5"), "{sum:?}");
    let bad = call(&agent, "calculator__calculate", json!({"expression": "1 / 0"})).await;
    assert_eq!(bad.is_error, Some(true));

    // closing a window leaves the other open and the app running
    rpc(&mut ws, 30, "view.close", json!({"viewId": first["viewId"]})).await;
    assert_eq!(r.host.views.open_views(), 1);

    // a dropped connection takes its windows with it
    drop(ws);
    wait_until("the windows to be released", || r.host.views.open_views() == 0).await;

    agent.cancel().await.unwrap();
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn a_view_in_another_connection_cannot_be_driven() {
    let tmp = temp("views-owner");
    let r = host_with(&tmp, &[("calculator", &["app.luau", "calc.luau"])]).await;
    let mut a = browser(&r).await;
    let mut b = browser(&r).await;
    let view = open(&mut a, 1, "calculator").await;
    let reply = tap(&mut b, 2, &view, 1, "key-7").await;
    assert_eq!(reply["error"]["code"], -32011, "a view belongs to the socket that opened it: {reply}");
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}
