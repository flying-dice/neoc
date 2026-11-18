//! The composition root of the hexagon. Reads like a wiring diagram: build every adapter, hand each to the port it
//! implements, mount the driving adapters on one axum router. No business logic lives here.

use agentos_acp::{AcpConnector, AgentLaunch};
use agentos_agui::{agui_router, AgUiEvent, Role, RunAgentInput, RunHandler};
use agentos_application::{
    ports::{AgentDirectory, AppRuntime, DataStore, EventSink, Fetcher, ToolRegistry},
    AppService,
};
use agentos_domain::{AgentId, AgentIdentity};
use agentos_fetch::UrlFetcher;
use agentos_http::{origin_guard, rpc_router, static_router, AllowedOrigins, RpcHub};
use agentos_luau::{Limits, LuauRuntime};
use agentos_mcp::{mcp_router, McpState};
use agentos_sqlite::SqliteDataStore;
use axum::Router;
use futures_util::{stream, stream::BoxStream, StreamExt};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub struct HostConfig {
    /// `None` = in-memory databases (tests, `dev:web`).
    pub data_dir: Option<PathBuf>,
    /// Built Vite client; `None` = API only (dev: Vite serves the client).
    pub client_dir: Option<PathBuf>,
    pub port: u16,
    pub dev_client_port: Option<u16>,
    /// (bearer token, identity)
    pub agents: Vec<(String, AgentIdentity)>,
    pub acp: HashMap<AgentId, AgentLaunch>,
}

pub struct Host {
    pub router: Router,
    pub service: Arc<AppService>,
    pub shutdown: CancellationToken,
}

struct TokenDirectory(Vec<(String, AgentIdentity)>);
impl AgentDirectory for TokenDirectory {
    fn authenticate(&self, token: &str) -> Option<AgentIdentity> {
        // compare every entry (no early exit on the first match)
        let mut found = None;
        for (t, id) in &self.0 {
            if constant_time_eq(t.as_bytes(), token.as_bytes()) {
                found = Some(id.clone());
            }
        }
        found
    }
}
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Placeholder AG-UI run: the real one maps ACP `session/update` to AG-UI events.
struct PlaceholderRun;
impl RunHandler for PlaceholderRun {
    fn run(&self, i: RunAgentInput) -> BoxStream<'static, AgUiEvent> {
        stream::iter([
            AgUiEvent::RunStarted {
                thread_id: i.thread_id.clone(),
                run_id: i.run_id.clone(),
                parent_run_id: None,
            },
            AgUiEvent::TextMessageStart { message_id: "m1".into(), role: Role::Assistant },
            AgUiEvent::TextMessageContent {
                message_id: "m1".into(),
                delta: "ACP -> AG-UI mapping goes here".into(),
            },
            AgUiEvent::TextMessageEnd { message_id: "m1".into() },
            AgUiEvent::RunFinished { thread_id: i.thread_id, run_id: i.run_id, result: None },
        ])
        .boxed()
    }
}

pub fn build(cfg: HostConfig) -> anyhow::Result<Host> {
    let shutdown = CancellationToken::new();

    // driven adapters -> ports
    let hub = Arc::new(RpcHub::default()); // EventSink + Notifier + the /rpc handler
    let mcp_state = Arc::new(McpState::default()); // ToolRegistry
    let fetcher: Arc<dyn Fetcher> = Arc::new(UrlFetcher::new(8 * 1024 * 1024)?);
    let data: Arc<dyn DataStore> = Arc::new(SqliteDataStore::new(cfg.data_dir.map(|d| d.join("apps"))));
    let runtime: Arc<dyn AppRuntime> = Arc::new(LuauRuntime::new(hub.clone(), Limits::default()));
    let tools: Arc<dyn ToolRegistry> = mcp_state.clone();
    let events: Arc<dyn EventSink> = hub.clone();
    let agents = Arc::new(AcpConnector::new(cfg.acp));

    // application
    let service = Arc::new(AppService::new(fetcher, data, runtime, tools, events, agents));
    hub.bind(service.clone());

    // driving adapters
    let allowed = Arc::new(AllowedOrigins::loopback(cfg.port, cfg.dev_client_port));
    let directory: Arc<dyn AgentDirectory> = Arc::new(TokenDirectory(cfg.agents));
    let mut router = rpc_router(hub).merge(agui_router(Arc::new(PlaceholderRun))).merge(mcp_router(
        mcp_state,
        service.clone(),
        directory,
        shutdown.child_token(),
    ));
    if let Some(dir) = cfg.client_dir {
        router = router.merge(static_router(&dir));
    }
    let router = router.layer(axum::middleware::from_fn_with_state(allowed, origin_guard));

    Ok(Host { router, service, shutdown })
}
