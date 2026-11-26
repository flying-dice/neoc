#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The agent-facing part of the app API: `app.agents:list()`.

use crate::common;

use common::{agent_ctx, db, id, package};
use serde_json::json;
use std::sync::Arc;
use vos_application::ports::{AgentCard, AgentRoster};
use vos_domain::AgentId;
use vos_lua::{AppVm, VmConfig};

/// The first line of every source here: the line numbers the tests name are the body's plus one.
const MANIFEST: &str = "-- probe";

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
async fn the_roster_is_listed_as_cards_without_grants_or_tokens() {
    let vm = with_roster(r#"RPC.on_request("who", function() return app.agents:list() end)"#).await;
    let out = common::serve(&vm, "who", json!({}), &agent_ctx(0)).await.unwrap();
    assert_eq!(
        out,
        json!([
            {"id": "claude", "name": "Claude", "avatar": "sparkles", "role": "Pair programmer"},
            {"id": "other", "name": "Other", "role": ""}
        ])
    );
}
