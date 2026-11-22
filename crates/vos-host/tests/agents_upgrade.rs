#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! Journey AG8 (`apps/agents/JOURNEYS.md`) on the real host: someone who chatted with the view-tree Agents app finds their
//! chats after it is replaced by the Agents app with a page of its own. The app's data stays with the host across the
//! change (uninstalled keeping its data, the new one installed), the new app opens the old database with the same migration,
//! and the conversation, which the host keeps as the AG-UI thread `agents:<chat id>`, is still there.
//!
//! This is a host test and not a Playwright journey because the e2e host starts with the current Agents app installed and
//! cannot start from the old one.

mod common;

use common::{
    agents::{launchable, run_body, sse_events},
    browser, pair, publish_package, rpc, start, temp,
};
use serde_json::{json, Value};
use std::{path::Path, process::Command};
use vos_domain::{AppId, Grants};
use vos_host::startup::prepare_data_dir;

/// The old app's data as it made it: its migration step 1 byte for byte (`apps/agents/app.luau` at f777598a,
/// `crates/vos-lua/tests/agents_app.rs` holds the new app to the same text) and one chat in it.
const OLD_APP: &str = r#"
local app = require("vos:app")
app.manifest { id = "agents", name = "Agents", icon = "sparkles", tint = "orange" }
local db = app.db
db:migrate {
  [1] = [[
    create table threads (
      id text primary key default (lower(hex(randomblob(8)))),
      agent_id text not null,
      agent_name text not null,
      title text not null default 'New chat',
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    create index threads_by_use on threads(updated_at desc, id desc);
  ]],
}
db:exec("insert or ignore into threads(id, agent_id, agent_name, title) values ('0123456789abcdef', 'fake', 'Fake', 'An old chat')")
return {}
"#;

const CHAT: &str = "0123456789abcdef";

/// The Agents app as `bun run build` makes it (`apps/agents/dist/app-ref.json`).
fn built_agents_app() -> String {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/agents");
    let run = Command::new("bun")
        .args(["run", "build"])
        .current_dir(&project)
        .output()
        .expect("bun is needed to build the Agents app (https://bun.sh)");
    assert!(run.status.success(), "bun run build failed:\n{}", String::from_utf8_lossy(&run.stderr));
    format!("file://{}", project.join("dist/app-ref.json").canonicalize().unwrap().display())
}

#[tokio::test]
async fn a_chat_made_with_the_view_tree_agents_app_is_listed_and_keeps_its_transcript_after_the_change() {
    let new_app = built_agents_app();
    let tmp = temp("agents-upgrade");
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![("tok-fake".into(), launchable("fake", "Fake", Grants::default()))];
    })
    .await;
    let agents = AppId::parse("agents").unwrap();

    // before: the old app made the chat, and the person talked in it
    let old = publish_package(&tmp.join("old"), "agents", "app.luau", &[("app.luau", OLD_APP.as_bytes().to_vec())]);
    r.host.service.install_from_url(&old).await.unwrap();
    let cookie = pair(&r, "Safari on iPad").await;
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| http.post(r.url(path)).header("cookie", &cookie).json(&body).send();
    let thread = format!("agents:{CHAT}");
    assert_eq!(post("/agui/threads", json!({"threadId": thread, "agentId": "fake"})).await.unwrap().status(), 200);
    let said = sse_events(&post("/agui", run_body(&thread, "r1", "hello", "fake")).await.unwrap().text().await.unwrap());
    assert_eq!(said.last().unwrap()["type"], "RUN_FINISHED");

    // the change: the old app goes, keeping its data, and the new one is installed
    r.host.service.uninstall(&agents, false).await.unwrap();
    r.host.service.install_from_url(&new_app).await.unwrap();

    // after: the chat is listed with its title, a new message keeps the name, and the transcript is there
    let mut ws = browser(&r).await;
    let list = rpc(&mut ws, 1, "app.call", json!({"app": "agents", "method": "chats"})).await;
    let chats = list["result"]["chats"].as_array().unwrap_or_else(|| panic!("{list}"));
    assert_eq!(chats.len(), 1, "{list}");
    assert_eq!(chats[0]["id"], CHAT);
    assert_eq!(chats[0]["title"], "An old chat");
    assert_eq!(chats[0]["agentName"], "Fake");
    let noted =
        rpc(&mut ws, 2, "app.call", json!({"app": "agents", "method": "noteMessage", "args": {"id": CHAT, "text": "hi"}}))
            .await;
    assert_eq!(noted["result"]["title"], "An old chat", "{noted}");
    let snapshot: Value = http
        .get(r.url(&format!("/agui/threads/{thread}")))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let contents: Vec<&str> =
        snapshot["messages"].as_array().unwrap().iter().filter_map(|m| m["content"].as_str()).collect();
    assert_eq!(contents, ["hello", "Hi there"], "{snapshot}");
    r.stop().await;
}
