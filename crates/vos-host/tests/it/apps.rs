#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Apps end to end over the real adapters, from a paired browser on `/rpc` (what the shell does for an app's page)
//! and an agent on `/mcp`: the handshake, background work that tells the open pages by itself, how installed and
//! available apps are described, and an app of the kind the host no longer runs.

use crate::common;

use common::{
    browser, built_backend, call, claude, connect_agent, next_notification, notification_where,
    publish_app_ref, publish_package, rpc, start, temp, tool_names, Agent, Running,
};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use vos_application::AppStatus;
use vos_domain::AppId;
use vos_host::startup::{prepare_data_dir, start_apps};

/// A host with a data directory, the agent `claude`, and these fixture backends installed (each as the app of its name).
async fn host_with(tmp: &Path, backends: &[&str]) -> Running {
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    })
    .await;
    for app in backends {
        let files = [("app.luau", built_backend(app))];
        let url = publish_package(&tmp.join(format!("pkg-{app}")), app, "app.luau", &files);
        r.host.service.install_from_url(&url).await.unwrap();
    }
    r
}

// ---- the handshake ------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_shell_says_hello_and_the_view_protocol_is_gone() {
    let r = start(|_| {}).await;
    let mut ws = browser(&r).await;
    let hello = json!({"proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC"});
    let reply = rpc(&mut ws, 1, "session.hello", hello.clone()).await;
    let welcome = &reply["result"];
    assert_eq!(
        (&welcome["proto"], &welcome["kit"], &welcome["resumed"]),
        (&json!(1), &json!("1.0"), &json!(false))
    );
    assert!(welcome["sessionId"].as_str().is_some_and(|id| id.starts_with('c')), "{reply}");
    assert_eq!(welcome["hostVersion"], env!("CARGO_PKG_VERSION"));

    // a client coming back says which session it had: the shape is accepted, and the welcome is a fresh one
    let mut again = hello.clone();
    again["features"] = json!([]);
    again["resume"] = json!({"sessionId": welcome["sessionId"], "views": []});
    let reply = rpc(&mut ws, 2, "session.hello", again).await;
    assert_eq!(reply["result"]["resumed"], false, "{reply}");
    // a newer client is answered with the protocol the host speaks; protocol 0 does not exist
    let mut newer = hello.clone();
    newer["proto"] = json!(7);
    assert_eq!(rpc(&mut ws, 3, "session.hello", newer).await["result"]["proto"], 1);
    let mut none = hello.clone();
    none["proto"] = json!(0);
    assert_eq!(rpc(&mut ws, 4, "session.hello", none).await["error"]["code"], -32012);
    let bad = rpc(&mut ws, 5, "session.hello", json!({"proto": 1})).await;
    assert_eq!(bad["error"]["code"], -32602, "{bad}");

    // the host draws no windows: what a view-tree client would ask is not a method any more
    for (id, method) in
        [(10, "view.open"), (11, "view.event"), (12, "row.set"), (13, "source.set"), (14, "state.set")]
    {
        let reply = rpc(&mut ws, id, method, json!({"viewId": "v1"})).await;
        assert_eq!(reply["error"]["code"], -32601, "{method}: {reply}");
    }
    // a notification the host has no use for (the client going away, anything unknown) is not an error
    common::notify(&mut ws, "session.bye", json!({})).await;
    common::notify(&mut ws, "client.state", json!({"viewId": "v1", "set": {}})).await;
    assert_eq!(rpc(&mut ws, 20, "app.list", json!({})).await["result"], json!({"apps": []}));
    r.stop().await;
}

// ---- background work ----------------------------------------------------------------------------------------

/// What the Ticker's `ticks` tool reports now.
async fn ticker_now(agent: &Agent) -> (u64, bool) {
    let out = call(agent, "ticker__ticks", json!({})).await;
    let content = serde_json::to_value(&out.content).unwrap();
    let report: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    (report["ticks"].as_u64().unwrap(), report["running"].as_bool().unwrap())
}

/// Wait for the Ticker's count to be above `floor`; returns it.
async fn ticks_above(agent: &Agent, floor: u64) -> u64 {
    for _ in 0..200 {
        let (ticks, _) = ticker_now(agent).await;
        if ticks > floor {
            return ticks;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the count never rose above {floor}");
}

/// The tick count of the Ticker's `ticked` event, if `event` is one.
fn ticks_in(event: &Value) -> Option<u64> {
    (event["app"] == "ticker" && event["name"] == "ticked").then(|| event["data"]["ticks"].as_u64()).flatten()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one journey, read top to bottom
async fn background_work_tells_the_open_pages_on_its_own_and_outlives_them() {
    let tmp = temp("ticker");
    let r = host_with(&tmp, &["ticker"]).await;
    let mut ws = browser(&r).await;
    let (agent, _) = connect_agent(r.addr, "tok-claude").await;
    let page = |method: &'static str| json!({"app": "ticker", "method": method, "args": {}});

    // --- the count rises and the shell hears each tick, with nothing sent by the client (AGOS-54 #1, #3)
    let first = notification_where(&mut ws, "app.event", |p| ticks_in(p).is_some()).await;
    let first_tick = ticks_in(&first).unwrap();
    let next =
        notification_where(&mut ws, "app.event", |p| ticks_in(p).is_some_and(|n| n > first_tick)).await;
    assert_eq!(ticks_in(&next), Some(first_tick + 1), "every tick is told, in order: {first} then {next}");

    // --- a call from the page reaches its handler, and the pause holds the count while the loop is still alive
    let paused = rpc(&mut ws, 3, "app.call", page("toggle")).await;
    assert_eq!(paused["result"], json!({"running": false}), "{paused}");
    let (held, running) = ticker_now(&agent).await;
    assert!(!running);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(ticker_now(&agent).await.0, held, "paused: the loop runs but counts nothing");
    let state = rpc(&mut ws, 4, "app.call", page("state")).await;
    assert_eq!(
        state["result"],
        json!({"ticks": held, "running": false}),
        "what the page reads is what the tool reads"
    );
    let resumed = rpc(&mut ws, 5, "app.call", page("toggle")).await;
    assert_eq!(resumed["result"], json!({"running": true}));
    ticks_above(&agent, held).await;

    // --- an error in a handler is that call's error answer, and the loop is untouched (AGOS-54 #6)
    let failed = rpc(&mut ws, 6, "app.call", page("fail")).await;
    assert_eq!(failed["error"]["code"], -32010, "{failed}");
    assert!(failed["error"]["message"].as_str().unwrap().contains("always fails"), "{failed}");
    let (before, _) = ticker_now(&agent).await;
    ticks_above(&agent, before).await;

    // --- the page going away (its socket closes) does not stop the service (AGOS-52 #2)
    drop(ws);
    let (at_close, _) = ticker_now(&agent).await;
    let later = ticks_above(&agent, at_close).await;

    // --- a page opened later reads the app as it is now, and hears the ticks from here on
    let mut ws = browser(&r).await;
    let state = rpc(&mut ws, 7, "app.call", page("state")).await;
    assert!(state["result"]["ticks"].as_u64().unwrap() >= later, "{state}");
    notification_where(&mut ws, "app.event", |p| ticks_in(p).is_some_and(|n| n > later)).await;

    // --- uninstalling stops and releases the app: its loop is silent, its handlers and its tools are gone
    let reply = rpc(&mut ws, 8, "app.uninstall", json!({"app": "ticker", "purge": true})).await;
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(next_notification(&mut ws, "app.uninstalled").await["app"], "ticker");
    assert!(!tool_names(&agent).await.iter().any(|n| n.starts_with("ticker__")));
    let gone = rpc(&mut ws, 9, "app.call", page("state")).await;
    assert_eq!(gone["error"]["code"], -32002, "{gone}");
    ws.backlog.clear();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let list = rpc(&mut ws, 10, "app.list", json!({})).await;
    assert_eq!(list["result"], json!({"apps": []}));
    assert!(
        !ws.backlog.iter().any(|m| m["method"] == "app.event"),
        "an uninstalled app says nothing: {:?}",
        ws.backlog
    );

    agent.cancel().await.unwrap();
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

// ---- how apps are described ---------------------------------------------------------------------------------

#[tokio::test]
async fn app_management_describes_installed_and_available_apps() {
    let tmp = temp("app-api");
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| c.data_dir = Some(tmp.join("data"))).await;
    let manifest = json!({
        "id": "ticker", "name": "Ticker", "main": "app.luau", "icon": "clock", "tint": "teal",
        "description": "Counts on its own.",
        "ui": { "root": "static", "permissions": { "clipboardWrite": {} } },
        "window": { "w": 360, "h": 280, "minW": 320, "minH": 240 },
    });
    let url = publish_app_ref(&tmp.join("pkg-ticker"), manifest, &[("app.luau", built_backend("ticker"))]);
    r.host.service.install_from_url(&url).await.unwrap();
    let mut ws = browser(&r).await;

    let list = rpc(&mut ws, 1, "app.list", json!({})).await;
    // the whole row: what the shell reads of an installed app, and nothing else (no `client` flag: every app has a page)
    assert_eq!(
        list["result"]["apps"][0],
        json!({
            "id": "ticker", "name": "Ticker", "version": "1.0.0", "state": "running",
            "icon": "clock", "tint": "teal", "description": "Counts on its own.",
            "window": {"w": 360, "h": 280, "minW": 320, "minH": 240},
            "source": "this computer",
            "permissions": ["clipboard-write"],
        }),
        "{list}"
    );

    let preview = rpc(&mut ws, 2, "app.preview", json!({"url": url})).await;
    assert_eq!(preview["result"]["id"], "ticker", "{preview}");
    assert_eq!(preview["result"]["installed"]["state"], "running");
    let missing = rpc(&mut ws, 3, "app.preview", json!({"url": "file:///nowhere/app-ref.json"})).await;
    assert!(missing.get("error").is_some());
    // `app.install` with a digest installs only the ref that was previewed
    let sha = preview["result"]["sha256"].as_str().expect("the preview carries the ref's digest").to_owned();
    assert_eq!(sha.len(), 64, "{preview}");
    let stale = rpc(&mut ws, 6, "app.install", json!({"url": url, "sha256": "0".repeat(64)})).await;
    let message = stale["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("changed"), "a ref other than the previewed one is refused: {stale}");

    // no registries configured: the catalog is empty, not an error
    let catalog = rpc(&mut ws, 4, "app.available", json!({})).await;
    assert_eq!(catalog["result"], json!({"apps": [], "errors": []}), "{catalog}");

    let devices = rpc(&mut ws, 5, "host.devices.list", json!({})).await;
    assert_eq!(devices["result"]["devices"][0]["current"], true, "{devices}");
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

// ---- an app of the kind the host no longer runs --------------------------------------------------------------

/// An app ref whose manifest has no `ui`: an app that drew its windows as a view tree. It is refused at install, and
/// one that an older host installed is listed as failed with the reason when this host starts, while the others run.
#[tokio::test]
async fn an_app_without_a_page_is_refused_at_install_and_failed_not_fatal_at_start() {
    let tmp = temp("no-page");
    prepare_data_dir(&tmp.join("data")).unwrap();
    let configure = |c: &mut vos_host::HostConfig| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    };
    let old_kind = b"app.manifest { id = 'oldnotes', name = 'Old notes', icon = 'gear' }\napp.window('main', { render = function() end })";
    let no_ui = json!({ "id": "oldnotes", "name": "Old notes", "main": "app.luau" });
    let files = [("app.luau", old_kind.to_vec())];

    // --- at install: refused before anything of it is fetched or run, and the message says what the app is
    let first = start(configure).await;
    let old_url = publish_app_ref(&tmp.join("pkg-old"), no_ui.clone(), &files);
    let mut ws = browser(&first).await;
    let preview = rpc(&mut ws, 1, "app.preview", json!({"url": old_url})).await;
    let message = preview["error"]["message"].as_str().unwrap_or_default().to_owned();
    assert!(message.contains("app `oldnotes` has no `ui` in its manifest"), "{preview}");
    assert!(message.contains("no longer supported"), "{preview}");
    let err = first.host.service.install_from_url(&old_url).await.unwrap_err().to_string();
    assert!(err.contains("app `oldnotes` has no `ui` in its manifest"), "{err}");
    assert!(first.host.service.apps().is_empty());
    assert!(!tmp.join("data/apps/oldnotes.db").exists(), "no database was made for it");

    // --- an older host installed it when a manifest needed no `ui`: here, the same record is made by installing
    //     the app with a page and then serving the ref as the older host saw it
    let with_ui =
        json!({ "id": "oldnotes", "name": "Old notes", "main": "app.luau", "ui": { "root": "static" } });
    let url = publish_app_ref(&tmp.join("pkg-old"), with_ui, &[("app.luau", b"return {}".to_vec())]);
    first.host.service.install_from_url(&url).await.unwrap();
    let good = publish_package(
        &tmp.join("pkg-ticker"),
        "ticker",
        "app.luau",
        &[("app.luau", built_backend("ticker"))],
    );
    first.host.service.install_from_url(&good).await.unwrap();
    first.host.service.shutdown().await;
    first.stop().await;
    publish_app_ref(&tmp.join("pkg-old"), no_ui, &files);

    // --- at start: the host comes up, the app is failed with the reason, the other app runs
    let second = start(configure).await;
    start_apps(&second.host.service, &[]).await;
    let status = |id: &str| second.host.service.status(&AppId::parse(id).unwrap()).unwrap();
    assert_eq!(status("ticker"), AppStatus::Running);
    let AppStatus::Failed(why) = status("oldnotes") else { panic!("{:?}", status("oldnotes")) };
    assert!(why.contains("app `oldnotes` has no `ui` in its manifest"), "{why}");
    let mut ws = browser(&second).await;
    let list = rpc(&mut ws, 1, "app.list", json!({})).await;
    let row = list["result"]["apps"].as_array().unwrap().iter().find(|a| a["id"] == "oldnotes").unwrap();
    assert_eq!(row["state"], "failed", "{list}");
    assert!(row["message"].as_str().unwrap().contains("has no `ui`"), "{list}");
    let (agent, _) = connect_agent(second.addr, "tok-claude").await;
    assert_eq!(tool_names(&agent).await, vec!["ticker__ticks"]);
    // and it can be removed like any app
    let gone = rpc(&mut ws, 2, "app.uninstall", json!({"app": "oldnotes", "purge": true})).await;
    assert!(gone.get("error").is_none(), "{gone}");
    agent.cancel().await.unwrap();
    second.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}
