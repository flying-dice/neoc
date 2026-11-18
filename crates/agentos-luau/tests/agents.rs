#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The agent-facing parts of the app API: `app.thread()` sources and `app.agents:list()`.

mod common;

use agentos_application::{
    live::SourceKind,
    ports::{AgentCard, AgentRoster},
};
use agentos_domain::{AgentId, Capabilities};
use agentos_luau::{AppVm, VmConfig};
use common::{agent_ctx, db, id, load, package};
use serde_json::json;
use std::sync::Arc;

const MANIFEST: &str =
    r#"app.manifest { id = "probe", name = "Probe", icon = "gear", permissions = { "data", "agents" } }"#;

struct Cards;
impl AgentRoster for Cards {
    fn cards(&self) -> Vec<AgentCard> {
        vec![
            AgentCard {
                id: AgentId("claude".into()),
                name: "Claude".into(),
                avatar: Some("sparkles".into()),
                role: "Pair programmer".into(),
            },
            AgentCard {
                id: AgentId("other".into()),
                name: "Other".into(),
                avatar: None,
                role: String::new(),
            },
        ]
    }
}

async fn with_roster(body: &str) -> AppVm {
    let config = VmConfig { roster: Arc::new(Cards), ..VmConfig::default() };
    AppVm::load(&id("probe"), package(&format!("{MANIFEST}\n{body}"), &[]), db(), config).await.unwrap()
}

#[tokio::test]
async fn app_thread_declares_a_source_that_runs_no_query_and_names_the_thread_and_the_agent() {
    let vm = load(&format!("{MANIFEST}\nlocal chat = app.thread()\nassert(chat ~= nil)")).await;
    let sources = vm.sources();
    assert_eq!(sources.len(), 1);
    let def = &sources[0];
    assert_eq!(def.kind, SourceKind::Thread);
    assert_eq!(def.info.named_params, ["thread", "agent"]);
    assert!(def.sql.is_empty() && def.info.tables.is_empty(), "nothing to query, nothing to invalidate");
}

#[tokio::test]
async fn a_thread_handle_needs_both_params_and_no_others() {
    let vm = with_roster(
        r#"
        local chat = app.thread()
        local function try(f) local ok, err = pcall(f); return ok, tostring(err) end
        app.tool { name = "probe", run = function()
          local ok_both = try(function() return chat { thread = "t1", agent = "claude" } end)
          local ok_missing, why_missing = try(function() return chat { thread = "t1" } end)
          local ok_extra, why_extra = try(function() return chat { thread = "t1", agent = "a", bogus = 1 } end)
          return { both = ok_both, missing = ok_missing, why_missing = why_missing, extra = ok_extra, why_extra = why_extra }
        end }
        "#,
    )
    .await;
    let out = vm.call_tool("probe", json!({}), &agent_ctx(0)).await.unwrap();
    assert_eq!(out["both"], true);
    assert_eq!(out["missing"], false);
    assert!(out["why_missing"].as_str().unwrap().contains("needs param `agent`"), "{out}");
    assert_eq!(out["extra"], false);
    assert!(out["why_extra"].as_str().unwrap().contains("no param `bogus`"), "{out}");
}

#[tokio::test]
async fn the_roster_is_listed_as_cards_without_grants_or_tokens() {
    let vm = with_roster(r#"app.tool { name = "who", run = function() return app.agents:list() end }"#).await;
    let out = vm.call_tool("who", json!({}), &agent_ctx(0)).await.unwrap();
    assert_eq!(
        out,
        json!([
            {"id": "claude", "name": "Claude", "avatar": "sparkles", "role": "Pair programmer"},
            {"id": "other", "name": "Other", "role": ""}
        ])
    );
}

#[tokio::test]
async fn listing_agents_needs_the_agents_permission() {
    let config = VmConfig {
        capabilities: Capabilities { data: true, ..Capabilities::default() },
        roster: Arc::new(Cards),
        ..VmConfig::default()
    };
    let vm = AppVm::load(
        &id("probe"),
        package(
            r#"app.manifest { id = "probe", name = "Probe", icon = "gear", permissions = { "data" } }
               app.tool { name = "who", run = function() return app.agents:list() end }"#,
            &[],
        ),
        db(),
        config,
    )
    .await
    .unwrap();
    let err = vm.call_tool("who", json!({}), &agent_ctx(0)).await.unwrap_err().to_string();
    assert!(err.contains("agents") && err.contains("permission"), "{err}");
}
