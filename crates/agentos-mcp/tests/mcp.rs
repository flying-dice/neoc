#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use agentos_application::{
    ports::AgentDirectory,
    testing::{Fakes, StaticDirectory},
    AppService,
};
use agentos_domain::{AgentId, AgentIdentity, AppId, ToolGrant};
use agentos_mcp::{mcp_router, McpState};
use rmcp::{
    model::{CallToolRequestParams, ClientConfig, ProtocolVersion},
    service::NotificationContext,
    transport::{streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport},
    ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient, ServiceExt,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

struct Harness {
    url: String,
    svc: Arc<AppService>,
    fakes: Fakes,
    cancel: CancellationToken,
}

async fn harness() -> Harness {
    let fakes = Fakes::new();
    let state = Arc::new(McpState::default());
    // the REAL mcp adapter is the ToolRegistry; everything else is an in-memory fake
    let svc = Arc::new(fakes.service_with_tools(state.clone()));
    let notes = AppId::parse("notes").unwrap();
    let dir: Arc<dyn AgentDirectory> = Arc::new(StaticDirectory(vec![
        (
            "tok-claude".into(),
            AgentIdentity {
                id: AgentId("claude".into()),
                name: "Claude".into(),
                grants: vec![ToolGrant::CallTools(notes)],
            },
        ),
        (
            "tok-other".into(),
            AgentIdentity { id: AgentId("other".into()), name: "Other".into(), grants: vec![] },
        ),
    ]));
    let cancel = CancellationToken::new();
    let router = mcp_router(state, svc.clone(), dir, cancel.child_token());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let c = cancel.clone();
    tokio::spawn(async move {
        axum::serve(listener, router).with_graceful_shutdown(async move { c.cancelled_owned().await }).await
    });
    Harness { url, svc, fakes, cancel }
}

#[derive(Clone, Default)]
struct Counting(Arc<AtomicUsize>);
impl ClientHandler for Counting {
    async fn on_tool_list_changed(&self, _: NotificationContext<RoleClient>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

async fn wait_for(n: &Arc<AtomicUsize>, at_least: usize) {
    for _ in 0..100 {
        if n.load(Ordering::SeqCst) >= at_least {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {at_least} tools/list_changed (got {})", n.load(Ordering::SeqCst));
}

fn transport(url: &str, token: &str) -> StreamableHttpClientTransport<reqwest::Client> {
    StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()).auth_header(token),
    )
}

fn text(r: &rmcp::model::CallToolResult) -> String {
    serde_json::to_value(&r.content).unwrap().to_string()
}

#[tokio::test]
async fn missing_or_wrong_bearer_token_is_401() {
    let h = harness().await;
    let c = reqwest::Client::new();
    for auth in [None, Some("Bearer nope"), Some("Basic abc")] {
        let mut r = c
            .post(&h.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body("{}");
        if let Some(a) = auth {
            r = r.header("authorization", a);
        }
        assert_eq!(r.send().await.unwrap().status(), 401, "{auth:?}");
    }
    h.cancel.cancel();
}

#[tokio::test]
async fn legacy_2025_session_gets_list_changed_on_install_and_uninstall_and_attribution_works() {
    let h = harness().await;
    let counter = Counting::default();
    let client = counter.clone().serve(transport(&h.url, "tok-claude")).await.unwrap();
    assert!(client.peer().list_tools(None).await.unwrap().tools.is_empty());

    // install while the session is open => tools/list_changed => new tool visible
    let url = h.fakes.add_app("notes", &[("add_note", "x")]);
    h.svc.install_from_url(&url).await.unwrap();
    wait_for(&counter.0, 1).await;
    let tools = client.peer().list_tools(None).await.unwrap().tools;
    assert_eq!(tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>(), vec!["notes__add_note"]);

    // per-agent identity: Claude is granted `notes`, Other is not
    let ok = client
        .peer()
        .call_tool(
            CallToolRequestParams::new("notes__add_note")
                .with_arguments(serde_json::json!({"a": 1}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_ne!(ok.is_error, Some(true));
    println!("claude call -> {}", text(&ok));
    let other = counter.clone().serve(transport(&h.url, "tok-other")).await.unwrap();
    let denied = other.peer().call_tool(CallToolRequestParams::new("notes__add_note")).await.unwrap();
    assert_eq!(denied.is_error, Some(true));
    assert!(text(&denied).contains("Other may not call"), "{}", text(&denied));
    println!("other call  -> {}", text(&denied));
    assert_eq!(h.fakes.runtime.calls(), 1, "only the permitted call reached the VM");

    // uninstall => list_changed => tool gone
    let before = counter.0.load(Ordering::SeqCst);
    h.svc.uninstall(&AppId::parse("notes").unwrap(), false).await.unwrap();
    wait_for(&counter.0, before + 1).await;
    assert!(client.peer().list_tools(None).await.unwrap().tools.is_empty());

    client.cancel().await.unwrap();
    other.cancel().await.unwrap();
    h.cancel.cancel();
}

#[tokio::test]
async fn modern_2026_listen_stream_gets_list_changed_without_sessions() {
    use rmcp::model::SubscriptionFilter;
    let h = harness().await;
    let client = ClientConfig::default()
        .serve_with_lifecycle(
            transport(&h.url, "tok-claude"),
            ClientLifecycleMode::Discover { preferred_versions: vec![ProtocolVersion::V_2026_07_28] },
        )
        .await
        .unwrap();
    let mut sub = client.listen(SubscriptionFilter::builder().tools_list_changed().build()).await.unwrap();
    let url = h.fakes.add_app("notes", &[("add_note", "x")]);
    h.svc.install_from_url(&url).await.unwrap();
    let n = tokio::time::timeout(Duration::from_secs(5), sub.next())
        .await
        .expect("notification in time")
        .unwrap();
    assert!(matches!(n, Some(rmcp::model::ServerNotification::ToolListChangedNotification(_))));
    assert_eq!(client.peer().list_tools(None).await.unwrap().tools.len(), 1);
    sub.cancel().await.unwrap();
    client.cancel().await.unwrap();
    h.cancel.cancel();
}
