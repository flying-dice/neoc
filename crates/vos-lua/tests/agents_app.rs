#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! The Agents app (`apps/agents`, a Vite project): its contract's methods through the generated router, under the real
//! runtime, with a roster of two agents. The backend keeps the list of chats (the `threads` table of the view-tree Agents
//! app it replaced, with the same migration) and which one is open; the conversations are the host's and never reach it.

mod common;

use common::{built_package, db, id, user_ctx};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Once},
};
use vos_application::{
    data::{Params, SqlMigration},
    ports::{AgentCard, AgentRoster, AppData, AppPackage, AppRuntime, Notifier, PortResult},
    PortError,
};
use vos_domain::{AgentId, Notification};
use vos_lua::LuaRuntime;

const APP: &str = "agents";

/// The migration of the view-tree Agents app (`apps/agents/app.luau` at f777598a, version 0.1.2), byte for byte: a
/// database it made must still open under the new app, so the new app's step 1 must be this text exactly.
const OLD_STEP_1: &str = "
    create table threads (
      id text primary key default (lower(hex(randomblob(8)))),
      agent_id text not null,
      agent_name text not null,
      title text not null default 'New chat',
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    create index threads_by_use on threads(updated_at desc, id desc);
  ";

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

struct Roster;
impl AgentRoster for Roster {
    fn cards(&self) -> Vec<AgentCard> {
        vec![
            AgentCard {
                id: AgentId("claude".into()),
                name: "Claude".into(),
                avatar: Some("sparkles".into()),
                role: "Pair programmer".into(),
            },
            AgentCard { id: AgentId("other".into()), name: "Other".into(), avatar: None, role: String::new() },
        ]
    }
}

fn project() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps").join(APP)
}

/// The app's generated files are not committed: generate them once, as the project's own scripts do.
fn generate() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let run = Command::new("bun")
            .args(["run", "generate"])
            .current_dir(project())
            .output()
            .expect("bun is needed to generate the app's router (https://bun.sh)");
        assert!(run.status.success(), "bun run generate failed:\n{}", String::from_utf8_lossy(&run.stderr));
    });
}

async fn agents_on(data: Arc<dyn AppData>) -> LuaRuntime {
    generate();
    let package = AppPackage { has_client: true, ..built_package(&project().join(".app/entry.luau")) };
    let runtime = LuaRuntime::new(Arc::new(Quiet), Arc::new(vos_application::testing::RecordingShell::default()))
        .with_roster(Arc::new(Roster));
    runtime.load(&id(APP), package, data).await.unwrap();
    runtime
}

async fn call(rt: &LuaRuntime, method: &str, args: Value) -> PortResult<Value> {
    rt.call_handler(&id(APP), method, args, user_ctx(0)).await
}

/// What `ctx:fail` said.
fn failure(result: PortResult<Value>) -> String {
    match result {
        Err(PortError::Rejected(message)) => message,
        other => panic!("expected the call to fail with a message, got {other:?}"),
    }
}

#[tokio::test]
async fn the_roster_is_the_hosts_with_only_what_a_person_picks_from() {
    let rt = agents_on(db()).await;
    let agents = call(&rt, "agents", Value::Null).await.unwrap();
    assert_eq!(
        agents,
        json!([
            {"id": "claude", "name": "Claude", "role": "Pair programmer", "avatar": "sparkles"},
            {"id": "other", "name": "Other", "role": ""}
        ])
    );
}

#[tokio::test]
async fn starting_a_chat_lists_it_first_and_opens_it_and_an_agent_not_on_the_roster_is_refused() {
    let rt = agents_on(db()).await;
    assert_eq!(call(&rt, "chats", Value::Null).await.unwrap(), json!({"chats": []}));
    let first = call(&rt, "startChat", json!({"agentId": "claude"})).await.unwrap();
    assert_eq!(first["title"], "New chat");
    assert_eq!(first["agentName"], "Claude");
    let id = first["id"].as_str().unwrap();
    assert!(
        id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()),
        "an id the host page accepts as a chat id: {id}"
    );
    let list = call(&rt, "chats", Value::Null).await.unwrap();
    assert_eq!(list["open"], id);
    assert_eq!(list["chats"][0]["id"], id);

    let refused = failure(call(&rt, "startChat", json!({"agentId": "ghost"})).await);
    assert!(refused.contains("There is no agent with that id on this host."), "{refused}");
    assert_eq!(call(&rt, "chats", Value::Null).await.unwrap()["chats"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_first_message_names_a_chat_later_ones_only_move_it_to_the_top() {
    let rt = agents_on(db()).await;
    let a = call(&rt, "startChat", json!({"agentId": "claude"})).await.unwrap();
    let b = call(&rt, "startChat", json!({"agentId": "other"})).await.unwrap();
    let a_id = a["id"].as_str().unwrap();
    let named = call(&rt, "noteMessage", json!({"id": a_id, "text": "  plan the garden\nwith beds"})).await.unwrap();
    assert_eq!(named["title"], "plan the garden");
    let again = call(&rt, "noteMessage", json!({"id": a_id, "text": "and a pond"})).await.unwrap();
    assert_eq!(again["title"], "plan the garden");
    let list = call(&rt, "chats", Value::Null).await.unwrap();
    assert_eq!(list["chats"][0]["id"], a_id, "the chat last used is first");
    assert_eq!(list["chats"][1]["id"], b["id"]);

    // a long first line is cut on a character boundary
    let c = call(&rt, "startChat", json!({"agentId": "claude"})).await.unwrap();
    let long = "é".repeat(60);
    let cut = call(&rt, "noteMessage", json!({"id": c["id"], "text": long})).await.unwrap();
    assert_eq!(cut["title"], format!("{}…", "é".repeat(47)));
    // an empty message leaves the default
    let d = call(&rt, "startChat", json!({"agentId": "claude"})).await.unwrap();
    assert_eq!(call(&rt, "noteMessage", json!({"id": d["id"], "text": "   "})).await.unwrap()["title"], "New chat");

    let missing = failure(call(&rt, "noteMessage", json!({"id": "nope", "text": "x"})).await);
    assert!(missing.contains("There is no chat with that id."), "{missing}");
}

#[tokio::test]
async fn the_open_chat_is_remembered_and_a_chat_that_is_gone_is_not_open() {
    let data = db();
    let rt = agents_on(data.clone()).await;
    let a = call(&rt, "startChat", json!({"agentId": "claude"})).await.unwrap();
    let b = call(&rt, "startChat", json!({"agentId": "claude"})).await.unwrap();
    assert_eq!(call(&rt, "openChat", json!({"id": a["id"]})).await.unwrap()["open"], a["id"]);
    // a new load of the app (the window reopened, the host restarted) opens on it again
    let again = agents_on(data.clone()).await;
    assert_eq!(call(&again, "chats", Value::Null).await.unwrap()["open"], a["id"]);
    assert_eq!(call(&again, "openChat", json!({"id": null})).await.unwrap().get("open"), None);
    let refused = failure(call(&again, "openChat", json!({"id": "nope"})).await);
    assert!(refused.contains("There is no chat with that id."), "{refused}");
    let _ = b;
}

#[tokio::test]
async fn the_router_refuses_a_call_the_contract_does_not_allow() {
    let rt = agents_on(db()).await;
    for (method, args) in [
        ("startChat", json!({})),
        ("startChat", json!({"agentId": "claude", "app": "notes"})),
        ("noteMessage", json!({"id": "x".repeat(65), "text": "hi"})),
        ("openChat", json!({"id": 3})),
    ] {
        match call(&rt, method, args.clone()).await {
            Err(PortError::Rpc { code: -32602, .. }) => {}
            other => panic!("{method}({args}) should be refused as invalid params, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_chat_made_by_the_view_tree_agents_app_is_still_listed() {
    // the app's step 1 is the old one, byte for byte
    let main = std::fs::read_to_string(project().join("server/main.luau")).unwrap();
    assert!(main.contains(&format!("[1] = [[{OLD_STEP_1}]]")), "apps/agents/server/main.luau changed migration step 1");

    // a database the old app made: its migration, recorded as the host records it, and a chat in it
    let data: Arc<dyn AppData> = db();
    // Luau drops the newline that follows `[[`, so the step the host recorded starts after it
    let applied = OLD_STEP_1.strip_prefix('\n').unwrap();
    data.migrate(vec![SqlMigration { version: 1, name: "migration 1".into(), sql: applied.into() }]).await.unwrap();
    data.exec(
        "insert into threads(id, agent_id, agent_name, title) values ('0123456789abcdef', 'claude', 'Claude', 'An old chat')"
            .into(),
        Params::None,
    )
    .await
    .unwrap();

    let rt = agents_on(data).await;
    let list = call(&rt, "chats", Value::Null).await.unwrap();
    assert_eq!(list["chats"][0]["id"], "0123456789abcdef");
    assert_eq!(list["chats"][0]["title"], "An old chat");
    let noted = call(&rt, "noteMessage", json!({"id": "0123456789abcdef", "text": "still here"})).await.unwrap();
    assert_eq!(noted["title"], "An old chat", "a named chat keeps its name");
}
