#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use agentos_acp::{handshake, AcpConnector, AgentLaunch};
use agentos_application::ports::AgentConnector;
use agentos_domain::AgentId;
use std::{collections::HashMap, path::PathBuf, time::Duration};

fn fake() -> AgentLaunch {
    AgentLaunch { program: PathBuf::from(env!("CARGO_BIN_EXE_fake_agent")), args: vec![] }
}

#[tokio::test]
async fn handshake_with_fake_agent() {
    let hello =
        tokio::time::timeout(Duration::from_secs(10), handshake(&fake())).await.expect("no hang").unwrap();
    assert_eq!(hello.name.as_deref(), Some("fake-agent"));
    println!("fake agent hello: {hello:?}");
}

#[tokio::test]
async fn connector_port_starts_a_session() {
    let c = AcpConnector::new(HashMap::from([(AgentId("fake".into()), fake())]));
    let run = tokio::time::timeout(Duration::from_secs(10), c.start_run(&AgentId("fake".into()), "hi"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.0, "sess-1");
    assert!(c.start_run(&AgentId("nope".into()), "hi").await.is_err());
}

/// Real adapter: `node <repo>/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js`, initialize only.
/// Skipped when node or the package is not present.
#[tokio::test]
async fn handshake_with_real_claude_agent_acp_via_node() {
    let index = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js");
    if !index.exists() || std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: node or claude-agent-acp not installed");
        return;
    }
    let launch =
        AgentLaunch { program: PathBuf::from("node"), args: vec![index.to_string_lossy().into_owned()] };
    let hello = tokio::time::timeout(Duration::from_secs(30), handshake(&launch))
        .await
        .expect("handshake in 30s")
        .unwrap();
    println!("claude-agent-acp hello: {hello:?}");
    assert!(hello.name.is_some());
}
