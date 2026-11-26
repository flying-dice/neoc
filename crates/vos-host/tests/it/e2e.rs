#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! End-to-end: the REAL adapters (lua, sqlite, fetch, mcp, http) wired by the composition root,
//! driven from the outside like the real clients: a paired browser (`/rpc`) and an agent
//! with its own bearer token (`/mcp`).

use crate::common;

use common::{
    browser, built_backend, call, claude, connect_agent, connect_rpc, next_notification, notification_where,
    pair, publish_app_ref, publish_package, rpc, start, temp, text_of, tool_names, wait_until, Ws,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::atomic::Ordering, time::Duration};
use vos_application::AppStatus;
use vos_domain::{
    protocol::{AppNotification, HostNotification},
    AppId,
};
use vos_host::startup::{prepare_data_dir, start_apps};

/// Publish the Notes fixture backend (`tests/fixtures/backends/notes.luau`, built with the app pipeline: the one
/// Luau file an app ref ships) under `root` as the app `id`; returns the `file://` URL of the ref.
fn publish_app(root: &Path, id: &str) -> String {
    let name = if id == "notes" { "Notes" } else { id };
    let manifest = json!({ "id": id, "name": name, "main": "app.luau", "ui": { "root": "static" } });
    publish_app_ref(root, manifest, &[("app.luau", built_backend("notes"))])
}

fn publish_notes_app(root: &Path) -> String {
    publish_app(root, "notes")
}

/// The Notes app with a contract (`tests/fixtures/backends/notes.openrpc.json`): the bus holds its calls to it and an
/// agent is shown its public methods as it describes them.
fn publish_notes_app_with_contract(root: &Path) -> String {
    let manifest = json!({
        "id": "notes", "name": "Notes", "main": "app.luau", "ui": { "root": "static" }, "contract": "openrpc.json"
    });
    let contract = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/backends/notes.openrpc.json"),
    )
    .unwrap();
    publish_app_ref(root, manifest, &[("app.luau", built_backend("notes")), ("openrpc.json", contract)])
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one journey, read top to bottom
async fn install_over_rpc_then_use_from_mcp_and_ui_and_uninstall() {
    let tmp = temp("e2e");
    let app_ref_url = publish_notes_app_with_contract(&tmp.join("pkg"));
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    })
    .await;
    let addr = r.addr;

    // --- the browser pairs, then connects on /rpc and installs the app from a file:// app ref
    let cookie = pair(&r, "Safari on iPad").await;
    let raw =
        connect_rpc(addr, Some(&cookie), Some(&format!("http://127.0.0.1:{}", addr.port()))).await.unwrap();
    let mut ws = Ws { raw, backlog: vec![] };
    let preview = rpc(&mut ws, 50, "app.preview", json!({"url": app_ref_url})).await;
    let sha256 = preview["result"]["sha256"].as_str().expect("the preview carries the digest").to_owned();
    let installed = rpc(&mut ws, 1, "app.install", json!({"url": app_ref_url, "sha256": sha256})).await;
    assert_eq!(installed["result"]["app"], "notes", "{installed}");
    assert_eq!(next_notification(&mut ws, "app.installed").await["app"], "notes");
    let listed = rpc(&mut ws, 2, "app.list", json!({})).await;
    assert_eq!(listed["result"]["apps"][0]["state"], "running", "{listed}");
    assert_eq!(listed["result"]["apps"][0]["name"], "Notes", "the name its manifest gives it");

    // --- an agent connects on /mcp with its own bearer token and sees the tools
    let (agent, _) = connect_agent(addr, "tok-claude").await;
    assert_eq!(tool_names(&agent).await, vec!["notes__add_note", "notes__append", "notes__search"]);
    let tools = agent.peer().list_tools(None).await.unwrap().tools;
    let search = tools.iter().find(|t| t.name == "notes__search").unwrap();
    assert_eq!(search.annotations.as_ref().and_then(|a| a.read_only_hint), Some(true));
    assert_eq!(search.title.as_deref(), Some("Search Notes"));
    assert_eq!(search.input_schema["required"], json!(["query"]), "the schema the contract says");

    // --- the app's page (the shell forwards its calls as `app.call`) reads through a handler
    let notes = AppId::parse("notes").unwrap();
    let page = |method: &'static str, args: Value| json!({"app": "notes", "method": method, "args": args});
    let list = rpc(&mut ws, 3, "app.call", page("_list", json!({}))).await;
    assert_eq!(list["result"], json!([]), "{list}");

    // --- the agent calls a tool: Lua runs as that agent, SQLite writes, app.notify reaches the UI, and the app
    //     tells its open pages what changed (a notification) with no window code in the host
    let out = call(&agent, "notes__add_note", json!({"body": "from the agent"})).await;
    assert_ne!(out.is_error, Some(true), "{out:?}");
    // the typed `notification.deliver` message: the sender is the host's id for the app, not anything the app wrote
    let n = next_notification(&mut ws, "notification.deliver").await;
    assert_eq!(
        HostNotification::from_rpc("notification.deliver", n).unwrap(),
        HostNotification::Notify(AppNotification {
            app: "notes".into(),
            title: "Note added".into(),
            body: Some("from the agent".into()),
        })
    );
    let event = next_notification(&mut ws, "app.event").await;
    assert_eq!(event, json!({"app": "notes", "name": "notes.changed", "data": {"id": 1}}));
    // the page reads again, as it does when it hears the event
    let list = rpc(&mut ws, 4, "app.call", page("_list", json!({}))).await;
    assert_eq!(list["result"][0]["title"], "from the agent", "{list}");
    assert_eq!(list["result"].as_array().unwrap().len(), 1);

    // --- the note is open in the page; the agent appends to it and the page hears it
    let out = call(&agent, "notes__append", json!({"id": 1, "text": "- buy oat milk"})).await;
    assert_ne!(out.is_error, Some(true), "{out:?}");
    notification_where(&mut ws, "app.event", |p| p["name"] == "notes.changed" && p["data"]["id"] == 1).await;
    let open = rpc(&mut ws, 6, "app.call", page("_get", json!({"id": 1}))).await;
    assert_eq!(open["result"]["body"], "from the agent\n- buy oat milk", "{open}");
    assert_eq!((&open["result"]["rev"], &open["result"]["author"]), (&json!(1), &json!("Claude")));

    // --- the person edits the note in the page: the handler runs as them, and may write
    let saved = rpc(
        &mut ws,
        7,
        "app.call",
        page("_save", json!({"id": 1, "body": "From the agent\n- buy oat milk"})),
    )
    .await;
    assert_eq!(
        saved["result"],
        json!({"body": "From the agent\n- buy oat milk", "rev": 2, "author": "You"}),
        "{saved}"
    );
    // a handler's own failure is the app's error (-32010), with the message it gave
    let missing = rpc(&mut ws, 8, "app.call", page("_save", json!({"id": 99, "body": "x"}))).await;
    assert_eq!(missing["error"]["code"], -32010, "{missing}");
    assert_eq!(missing["error"]["message"], "No note with id 99");
    // and a method the backend does not have is JSON-RPC's own
    let unknown = rpc(&mut ws, 9, "app.call", page("nope", json!({}))).await;
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");

    // --- the agent's bad calls are tool errors, not host failures
    let out = call(&agent, "notes__append", json!({"id": 99, "text": "x"})).await;
    assert_eq!(out.is_error, Some(true));
    assert!(text_of(&out).contains("No note with id 99"), "{out:?}");
    let out = call(&agent, "notes__append", json!({"id": "x", "text": "x"})).await;
    assert_eq!(out.is_error, Some(true));
    assert!(text_of(&out).contains("Invalid params"), "{out:?}");

    // --- uninstall: the tool disappears for the agent (list_changed), VM + registry cleaned up
    let (agent2, counter) = connect_agent(addr, "tok-claude").await;
    let before = counter.0.load(Ordering::SeqCst);
    let reply = rpc(&mut ws, 10, "app.uninstall", json!({"app":"notes"})).await;
    assert!(reply.get("error").is_none(), "{reply}");
    wait_until("tools/list_changed", || counter.0.load(Ordering::SeqCst) > before).await;
    assert!(agent.peer().list_tools(None).await.unwrap().tools.is_empty());
    assert!(r.host.service.apps().is_empty());

    agent.cancel().await.unwrap();
    agent2.cancel().await.unwrap();
    r.host.service.uninstall(&notes, false).await.unwrap_err(); // already gone
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn installed_apps_come_back_after_a_restart_with_their_data() {
    let tmp = temp("restart");
    let url = publish_notes_app(&tmp.join("pkg"));
    prepare_data_dir(&tmp.join("data")).unwrap();
    let configure = |c: &mut vos_host::HostConfig| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    };

    let first = start(configure).await;
    first.host.service.install_from_url(&url).await.unwrap();
    let (agent, _) = connect_agent(first.addr, "tok-claude").await;
    call(&agent, "notes__add_note", json!({"body": "survives"})).await;
    agent.cancel().await.unwrap();
    first.host.service.shutdown().await; // what the binary does on SIGTERM
    first.stop().await;

    // a new process: nothing installed in memory, the system database remembers
    let second = start(configure).await;
    assert!(second.host.service.apps().is_empty());
    start_apps(&second.host.service, &[]).await;
    let apps = second.host.service.apps();
    assert_eq!((apps.len(), &apps[0].status), (1, &AppStatus::Running));
    let (agent, _) = connect_agent(second.addr, "tok-claude").await;
    assert_eq!(tool_names(&agent).await.len(), 3);
    let found = call(&agent, "notes__search", json!({"query": "surv"})).await;
    assert!(text_of(&found).contains("survives"), "the data was kept: {found:?}");

    // registries are applied on top: the same URL is not installed twice
    start_apps(&second.host.service, &[url]).await;
    assert_eq!(second.host.service.apps().len(), 1);
    agent.cancel().await.unwrap();
    second.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn disable_blocks_agents_keeps_data_and_enable_and_delete_data_work_over_rpc() {
    let tmp = temp("disable");
    let url = publish_notes_app(&tmp.join("pkg"));
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    })
    .await;
    r.host.service.install_from_url(&url).await.unwrap();
    let mut ws = browser(&r).await;
    let (agent, counter) = connect_agent(r.addr, "tok-claude").await;
    call(&agent, "notes__add_note", json!({"body": "kept"})).await;

    let before = counter.0.load(Ordering::SeqCst);
    assert!(rpc(&mut ws, 1, "app.disable", json!({"app": "notes"})).await.get("error").is_none());
    assert_eq!(next_notification(&mut ws, "app.disabled").await["app"], "notes");
    wait_until("list_changed", || counter.0.load(Ordering::SeqCst) > before).await;
    assert!(tool_names(&agent).await.is_empty(), "agent access is blocked");
    let out = call(&agent, "notes__add_note", json!({"body": "nope"})).await;
    assert_eq!(out.is_error, Some(true), "a stale tool name cannot reach a disabled app");
    let listed = rpc(&mut ws, 2, "app.list", json!({})).await;
    assert_eq!(listed["result"]["apps"][0]["state"], "disabled");

    assert!(rpc(&mut ws, 3, "app.enable", json!({"app": "notes"})).await.get("error").is_none());
    assert_eq!(tool_names(&agent).await.len(), 3);
    let found = call(&agent, "notes__search", json!({"query": "kept"})).await;
    assert!(text_of(&found).contains("kept"), "data survived the disable: {found:?}");

    assert!(rpc(&mut ws, 4, "app.deleteData", json!({"app": "notes"})).await.get("error").is_none());
    let found = call(&agent, "notes__search", json!({"query": "kept"})).await;
    assert!(!text_of(&found).contains("kept"), "the data is gone: {found:?}");
    assert_eq!(tool_names(&agent).await.len(), 3, "and the app is back, on an empty database");
    let err = rpc(&mut ws, 5, "app.enable", json!({"app": "ghost"})).await;
    assert_eq!(err["error"]["code"], -32002);

    agent.cancel().await.unwrap();
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn an_app_whose_bundle_broke_is_marked_failed_and_the_others_start() {
    let tmp = temp("failed");
    let good = publish_app(&tmp.join("pkg-notes"), "notes");
    let bad = publish_app(&tmp.join("pkg-todo"), "todo");
    prepare_data_dir(&tmp.join("data")).unwrap();
    let configure = |c: &mut vos_host::HostConfig| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    };
    let first = start(configure).await;
    first.host.service.install_from_url(&good).await.unwrap();
    first.host.service.install_from_url(&bad).await.unwrap();
    first.host.service.shutdown().await;
    first.stop().await;

    // the bundle is edited on disk without rebuilding the app ref: its checksum no longer matches
    std::fs::write(tmp.join("pkg-todo/app.luau"), "error('edited by hand')").unwrap();

    let second = start(configure).await;
    start_apps(&second.host.service, &[]).await;
    let status = |id: &str| second.host.service.status(&AppId::parse(id).unwrap()).unwrap();
    assert_eq!(status("notes"), AppStatus::Running);
    assert!(matches!(status("todo"), AppStatus::Failed(m) if m.contains("checksum")), "{:?}", status("todo"));
    let (agent, _) = connect_agent(second.addr, "tok-claude").await;
    assert_eq!(tool_names(&agent).await, vec!["notes__add_note", "notes__append", "notes__search"]);
    agent.cancel().await.unwrap();
    second.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn dev_reload_rebuilds_the_vm_when_the_bundle_changes_and_keeps_the_data() {
    let tmp = temp("reload");
    let pkg = tmp.join("pkg");
    let url = publish_notes_app(&pkg);
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    })
    .await;
    r.host.service.install_from_url(&url).await.unwrap();
    let stop = r.host.shutdown.clone();
    let watching = vos_host::startup::watch_for_reload(&r.host.service, &stop);
    assert_eq!(watching.len(), 1);
    let (agent, counter) = connect_agent(r.addr, "tok-claude").await;
    call(&agent, "notes__add_note", json!({"body": "before"})).await;
    tokio::time::sleep(Duration::from_millis(300)).await; // let the watcher settle

    // "rebuild" the bundle: a new tool (a public handler served last), a new checksum
    let before = counter.0.load(Ordering::SeqCst);
    let lua = std::fs::read_to_string(pkg.join("app.luau")).unwrap();
    let v2 = format!("{lua}\nRPC.on_request(\"extra\", function() return \"v2\" end)\n");
    std::fs::write(pkg.join("app.luau"), &v2).unwrap();
    let mut app_ref: Value =
        serde_json::from_slice(&std::fs::read(pkg.join("app-ref.json")).unwrap()).unwrap();
    let entry =
        app_ref["files"].as_array_mut().unwrap().iter_mut().find(|f| f["path"] == "app.luau").unwrap();
    entry["sha256"] = json!(format!("{:x}", Sha256::digest(v2.as_bytes())));
    std::fs::write(pkg.join("app-ref.json"), serde_json::to_vec(&app_ref).unwrap()).unwrap();

    wait_until("the reloaded tool list", || counter.0.load(Ordering::SeqCst) > before).await;
    assert!(tool_names(&agent).await.contains(&"notes__extra".to_owned()));
    let found = call(&agent, "notes__search", json!({"query": "before"})).await;
    assert!(text_of(&found).contains("before"), "the database was kept: {found:?}");

    agent.cancel().await.unwrap();
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn shutdown_unloads_installed_apps_and_retracts_their_tools() {
    let tmp = temp("shutdown");
    let app_ref_url = publish_notes_app(&tmp.join("pkg"));
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| c.data_dir = Some(tmp.join("data"))).await;

    r.host.service.install_from_url(&app_ref_url).await.unwrap();
    assert_eq!(r.host.service.installed_apps().len(), 1);
    r.host.service.shutdown().await;
    assert!(r.host.service.installed_apps().is_empty());
    assert!(tmp.join("data/apps/notes.db").exists(), "app data survives a shutdown");
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn the_fetcher_caps_one_package_file_at_the_domain_limit() {
    let tmp = temp("filecap");
    let max = vos_domain::limits::MAX_FILE_BYTES;
    let url = publish_package(
        &tmp.join("pkg"),
        "big",
        "app.luau",
        // an app is one bundled file, so the entry is the file the fetcher is handed
        &[("app.luau", vec![b'-'; max + 1])],
    );
    let r = start(|_| {}).await;
    let mut ws = browser(&r).await;
    let preview = rpc(&mut ws, 1, "app.preview", json!({"url": url})).await;
    let sha256 = preview["result"]["sha256"].as_str().unwrap().to_owned();
    let installed = rpc(&mut ws, 2, "app.install", json!({"url": url, "sha256": sha256})).await;
    let message = installed["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&format!("larger than {max} bytes")),
        "the fetcher refuses past the one file limit, not a bigger private one: {installed}"
    );
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}
