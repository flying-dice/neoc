#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! End-to-end: the REAL adapters (lua, sqlite, fetch, mcp, agui, http) wired by the composition root,
//! driven from the outside like the real clients: a paired browser (`/rpc`, `/agui`) and an agent
//! with its own bearer token (`/mcp`).

mod common;

use common::{
    browser, call, claude, connect_agent, connect_rpc, next_notification, pair, publish_package, rpc, start,
    temp, text_of, tool_names, wait_until, Ws,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use vos_application::{
    data::Params,
    live::{SourceId, SourceUpdate},
    testing::CollectingSink,
    AppStatus, BindingKind, Resolution, WriteRequest,
};
use vos_domain::{
    ui::protocol::{AppNotification, HostNotification},
    Actor, AppId, AppRef, FileRef, Manifest,
};
use vos_host::startup::{prepare_data_dir, start_apps};

/// Publish the fixture app under `root` as an app ref with the id `id`; returns the `file://` URL of the ref.
fn publish_app(root: &Path, id: &str) -> String {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes/app.luau");
    let lua = std::fs::read_to_string(src).unwrap().replace("\"notes\"", &format!("\"{id}\""));
    std::fs::create_dir_all(root).unwrap();
    // build the fixture with the app pipeline: the one Luau file an app ref ships
    std::fs::write(root.join("source.luau"), &lua).unwrap();
    let lua = String::from_utf8(common::build_luau(&root.join("source.luau"))).unwrap();
    std::fs::remove_file(root.join("source.luau")).unwrap();
    std::fs::write(root.join("app.luau"), &lua).unwrap();
    let r = AppRef {
        manifest: Manifest {
            id: AppId::parse(id).unwrap(),
            name: id.into(),
            version: "1.0.0".into(),
            main: "app.luau".into(),
            migrations: vec![],
            icon: None,
            tint: None,
            description: None,
            requires: None,
            client: None,
            window: None,
        },
        files: vec![FileRef {
            path: "app.luau".into(),
            url: format!("file://{}/app.luau", root.display()),
            sha256: format!("{:x}", Sha256::digest(lua.as_bytes())),
        }],
    };
    std::fs::write(root.join("app-ref.json"), serde_json::to_vec(&r).unwrap()).unwrap();
    format!("file://{}/app-ref.json", root.display())
}

fn publish_notes_app(root: &Path) -> String {
    publish_app(root, "notes")
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one journey, read top to bottom
async fn install_over_rpc_then_use_from_mcp_and_ui_and_uninstall() {
    let tmp = temp("e2e");
    let app_ref_url = publish_notes_app(&tmp.join("pkg"));
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
    assert_eq!(listed["result"]["apps"][0]["name"], "Notes", "the name the app declared");

    // --- an agent connects on /mcp with its own bearer token and sees the tools
    let (agent, _) = connect_agent(addr, "tok-claude").await;
    assert_eq!(tool_names(&agent).await, vec!["notes__add_note", "notes__append", "notes__search"]);
    let tools = agent.peer().list_tools(None).await.unwrap().tools;
    let search = tools.iter().find(|t| t.name == "notes__search").unwrap();
    assert_eq!(search.annotations.as_ref().and_then(|a| a.read_only_hint), Some(true));
    assert_eq!(search.title.as_deref(), Some("Search Notes"));
    assert_eq!(search.input_schema["required"], json!(["query"]), "the schema the app.t builders compiled");

    // --- an open view subscribes to the app's live sources (the view layer does this per window)
    let notes = AppId::parse("notes").unwrap();
    let live = r.host.service.live(&notes).unwrap();
    let sink = Arc::new(CollectingSink::default());
    let (_, list) = live.subscribe(&SourceId("s1".into()), Params::None, None, sink.clone()).await.unwrap();
    assert!(matches!(&list, SourceUpdate::Rows(d) if d.keys.is_empty() && d.total == Some(0)));
    let (_, count) = live.subscribe(&SourceId("s2".into()), Params::None, None, sink.clone()).await.unwrap();
    assert_eq!(count, SourceUpdate::Scalar(json!(0)));

    // --- the agent calls a tool: Lua runs as that agent, SQLite writes, app.notify reaches the UI, and the
    //     live sources push their deltas with no UI code anywhere
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
    wait_until("the list and count deltas", || sink.peek().len() >= 2).await;
    let updates = sink.take();
    let rows = updates
        .iter()
        .find_map(|(_, u)| if let SourceUpdate::Rows(d) = u { Some(d) } else { None })
        .expect("a rows delta");
    assert_eq!(rows.keys, vec![json!(1)]);
    assert_eq!(rows.changed[0].1["title"], "from the agent");
    assert!(updates.iter().any(|(_, u)| *u == SourceUpdate::Scalar(json!(1))));

    // --- a note is open in a view; the agent appends to it while the human types
    let (_, open) = live
        .subscribe(&SourceId("s3".into()), Params::named([("id", json!(1))]), None, sink.clone())
        .await
        .unwrap();
    let SourceUpdate::Row(open) = open else { panic!("row") };
    assert_eq!(open.rev, Some(json!(0)));
    let out = call(&agent, "notes__append", json!({"id": 1, "text": "- buy oat milk"})).await;
    assert_ne!(out.is_error, Some(true), "{out:?}");
    wait_until("the open note's delta", || {
        sink.peek().iter().any(|(_, u)| matches!(u, SourceUpdate::Row(r) if r.rev == Some(json!(1))))
    })
    .await;
    sink.take();

    // the human edited "from the agent" -> "From the agent" at rev 0 (their last sent value): the host merges
    let ack = r
        .host
        .service
        .write_field(
            &notes,
            WriteRequest {
                source: SourceId("s3".into()),
                params: Params::named([("id", json!(1))]),
                key: json!(1),
                field: "body".into(),
                value: json!("From the agent"),
                base_rev: Some(json!(0)),
                binding: BindingKind::Text { base: Some("from the agent".into()) },
                actor: Actor::user("You"),
            },
        )
        .await
        .unwrap();
    assert_eq!(ack.resolution, Resolution::Merged);
    assert_eq!(ack.value, json!("From the agent\n- buy oat milk"));
    assert_eq!(ack.rev, Some(json!(2)));
    wait_until("the merged note", || {
        sink.peek()
            .iter()
            .any(|(_, u)| matches!(u, SourceUpdate::Row(r) if r.row.as_ref().is_some_and(|row| row["author"] == "You")))
    })
    .await;

    // --- the agent's bad calls are tool errors, not host failures
    let out = call(&agent, "notes__append", json!({"id": 99, "text": "x"})).await;
    assert_eq!(out.is_error, Some(true));
    assert!(text_of(&out).contains("No note with id 99"), "{out:?}");
    let out = call(&agent, "notes__append", json!({"id": "x", "text": "x"})).await;
    assert_eq!(out.is_error, Some(true));
    assert!(text_of(&out).contains("invalid input"), "{out:?}");

    // --- uninstall: the tool disappears for the agent (list_changed), VM + registry cleaned up
    let (agent2, counter) = connect_agent(addr, "tok-claude").await;
    let before = counter.0.load(Ordering::SeqCst);
    let reply = rpc(&mut ws, 5, "app.uninstall", json!({"app":"notes"})).await;
    assert!(reply.get("error").is_none(), "{reply}");
    wait_until("tools/list_changed", || counter.0.load(Ordering::SeqCst) > before).await;
    assert!(agent.peer().list_tools(None).await.unwrap().tools.is_empty());
    assert!(r.host.service.apps().is_empty());

    // --- /agui is reachable for a paired device (the chat behaviour is tested in `agui.rs`)
    let res = reqwest::Client::new()
        .post(format!("http://{addr}/agui"))
        .header("cookie", &cookie)
        .json(&json!({"threadId":"t","runId":"r"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404, "no such thread");
    assert!(res.json::<serde_json::Value>().await.unwrap()["error"].as_str().unwrap().contains("no thread"));

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

    // "rebuild" the bundle: a new tool, a new checksum
    let before = counter.0.load(Ordering::SeqCst);
    let lua = std::fs::read_to_string(pkg.join("app.luau")).unwrap();
    let v2 =
        format!("{lua}\napp.tools.register({{ name = \"extra\", run = function() return \"v2\" end }})\n");
    std::fs::write(pkg.join("app.luau"), &v2).unwrap();
    let mut app_ref: AppRef =
        serde_json::from_slice(&std::fs::read(pkg.join("app-ref.json")).unwrap()).unwrap();
    app_ref.files[0].sha256 = format!("{:x}", Sha256::digest(v2.as_bytes()));
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
