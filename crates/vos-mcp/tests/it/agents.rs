#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Per-agent tool lists over MCP, with the real roster (`AgentService`) as the directory.

use rmcp::{
    model::CallToolRequestParams,
    service::NotificationContext,
    transport::{streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport},
    ClientHandler, RoleClient, ServiceExt,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use vos_application::{
    ports::AgentDirectory,
    testing::{Fakes, MemAgentStore, SeededRandom},
    AgentService, NewAgent,
};
use vos_domain::{AppId, Grants};
use vos_mcp::{mcp_router, McpState};

struct Harness {
    url: String,
    agents: Arc<AgentService>,
    svc: Arc<vos_application::AppService>,
    fakes: Fakes,
    cancel: CancellationToken,
}

async fn harness() -> Harness {
    let fakes = Fakes::new();
    let state = Arc::new(McpState::default());
    let svc = Arc::new(fakes.service_with_tools(state.clone()));
    // the roster tells the MCP adapter what changed through the same ToolRegistry port
    let agents = Arc::new(AgentService::new(
        Arc::new(MemAgentStore::default()),
        state.clone(),
        Arc::new(SeededRandom::default()),
    ));
    let dir: Arc<dyn AgentDirectory> = agents.clone();
    let cancel = CancellationToken::new();
    let router = mcp_router(state, svc.clone(), dir, cancel.child_token());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let c = cancel.clone();
    tokio::spawn(async move {
        axum::serve(listener, router).with_graceful_shutdown(async move { c.cancelled_owned().await }).await
    });
    Harness { url, agents, svc, fakes, cancel }
}

async fn agent(h: &Harness, id: &str) -> (vos_domain::AgentId, String) {
    let a = h
        .agents
        .create(NewAgent { id: Some(id.into()), name: id.to_uppercase(), ..NewAgent::default() })
        .await
        .unwrap();
    let token = h.agents.rotate_token(&a.id).await.unwrap();
    (a.id, token)
}

#[derive(Clone, Default)]
struct Counting(Arc<AtomicUsize>);
impl ClientHandler for Counting {
    async fn on_tool_list_changed(&self, _: NotificationContext<RoleClient>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn transport(url: &str, token: &str) -> StreamableHttpClientTransport<reqwest::Client> {
    StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()).auth_header(token),
    )
}

async fn names(client: &rmcp::service::RunningService<RoleClient, Counting>) -> Vec<String> {
    client.peer().list_tools(None).await.unwrap().tools.iter().map(|t| t.name.to_string()).collect()
}

async fn wait_for(n: &Arc<AtomicUsize>, at_least: usize) {
    for _ in 0..100 {
        if n.load(Ordering::SeqCst) >= at_least {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for tools/list_changed #{at_least} (got {})", n.load(Ordering::SeqCst));
}

#[tokio::test]
async fn an_agent_lists_only_the_tools_of_its_granted_apps_and_a_grant_change_reaches_only_that_agent() {
    let h = harness().await;
    for (app, tools) in
        [("notes", [("search?", "x"), ("append", "x")]), ("todo-list", [("add", "x"), ("done", "x")])]
    {
        let url = h.fakes.add_app(app, &tools);
        h.svc.install_from_url(&url).await.unwrap();
    }
    let (claude, claude_token) = agent(&h, "claude").await;
    let (_, other_token) = agent(&h, "other").await;
    let (claude_events, other_events) = (Counting::default(), Counting::default());
    let claude_client = claude_events.clone().serve(transport(&h.url, &claude_token)).await.unwrap();
    let other_client = other_events.clone().serve(transport(&h.url, &other_token)).await.unwrap();

    assert!(names(&claude_client).await.is_empty(), "deny by default: no grants, no tools");
    assert!(names(&other_client).await.is_empty());

    h.agents.set_grants(&claude, Grants::apps([AppId::parse("notes").unwrap()])).await.unwrap();
    wait_for(&claude_events.0, 1).await;
    assert_eq!(names(&claude_client).await, ["notes__append", "notes__search"]);
    assert!(names(&other_client).await.is_empty(), "another agent's list is unchanged");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(other_events.0.load(Ordering::SeqCst), 0, "and it was not told anything changed");

    // a tool of an app the agent was not granted is denied even when called by name
    let denied = claude_client.peer().call_tool(CallToolRequestParams::new("todo-list__add")).await.unwrap();
    assert_eq!(denied.is_error, Some(true));
    assert!(serde_json::to_value(&denied.content).unwrap().to_string().contains("may not call"));

    h.agents.set_grants(&claude, Grants::allow_all()).await.unwrap();
    wait_for(&claude_events.0, 2).await;
    assert_eq!(names(&claude_client).await.len(), 4);
    h.agents.set_grants(&claude, Grants::default()).await.unwrap();
    wait_for(&claude_events.0, 3).await;
    assert!(names(&claude_client).await.is_empty(), "revoking hides the tools again");

    claude_client.cancel().await.unwrap();
    other_client.cancel().await.unwrap();
    h.cancel.cancel();
}
