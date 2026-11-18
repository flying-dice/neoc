//! Driving adapter: MCP over Streamable HTTP (rmcp). It is a *driving* adapter (agents call in) and also
//! implements the `ToolRegistry` port (the application publishes tools into it).
//!
//! - one handler per MCP session, sharing `McpState` (the tool catalogue + live peers, each tagged with its agent)
//! - `tools/list` shows an agent only the tools of apps it is granted
//! - `ToolRegistry::publish/retract` -> `notifications/tools/list_changed` to every live session;
//!   `ToolRegistry::visibility_changed(agent)` (its grants were edited) -> to that agent's sessions only
//! - per-agent bearer tokens (`AgentDirectory` port) -> `AgentIdentity` in request extensions ->
//!   `AppService::call_tool` does the permission check and attribution.

use agentos_application::{
    ports::{AgentDirectory, PortResult, ToolRegistry},
    AppService,
};
use agentos_domain::{AgentId, AgentIdentity, AppId, QualifiedToolName, ToolSpec};
use async_trait::async_trait;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use parking_lot::RwLock;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
        PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig, SubscriptionFilter, Tool,
        ToolAnnotations,
    },
    service::{NotificationContext, RequestContext, SubscriptionContext},
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ErrorData, Peer, RoleServer, ServerHandler,
};
use std::{borrow::Cow, collections::HashMap, future::Future, sync::Arc};
use tokio_util::sync::CancellationToken;

/// The agent a request was authenticated as (the bearer middleware put it in the request's extensions).
fn identity_of(extensions: &rmcp::model::Extensions) -> Option<AgentIdentity> {
    extensions.get::<axum::http::request::Parts>().and_then(|p| p.extensions.get::<AgentIdentity>()).cloned()
}

/// Shared by every session handler and by the `ToolRegistry` implementation.
pub struct McpState {
    tools: RwLock<HashMap<AppId, Vec<ToolSpec>>>,
    peers: RwLock<Vec<(AgentId, Peer<RoleServer>)>>,
    /// Fan-out for the 2026-07-28 protocol (`subscriptions/listen`), where there are no sessions/peers: the
    /// agent whose list changed, or `None` for everyone.
    changed: tokio::sync::broadcast::Sender<Option<AgentId>>,
}

impl Default for McpState {
    fn default() -> Self {
        Self {
            tools: RwLock::default(),
            peers: RwLock::default(),
            changed: tokio::sync::broadcast::channel(16).0,
        }
    }
}

impl McpState {
    /// Tell the sessions of `agent` (every session for `None`) that their tool list changed.
    async fn notify(&self, agent: Option<&AgentId>) {
        let _ = self.changed.send(agent.cloned());
        let peers: Vec<_> = self.peers.read().clone();
        let mut alive = Vec::new();
        for (owner, p) in peers {
            if agent.is_some_and(|a| *a != owner) {
                alive.push((owner, p));
                continue;
            }
            // a closed session errors here: drop it from the list
            if p.notify_tool_list_changed().await.is_ok() {
                alive.push((owner, p));
            }
        }
        *self.peers.write() = alive;
    }
}

#[async_trait]
impl ToolRegistry for McpState {
    async fn publish(&self, app: &AppId, tools: Vec<ToolSpec>) -> PortResult<()> {
        self.tools.write().insert(app.clone(), tools);
        self.notify(None).await;
        Ok(())
    }
    async fn retract(&self, app: &AppId) -> PortResult<()> {
        self.tools.write().remove(app);
        self.notify(None).await;
        Ok(())
    }
    async fn visibility_changed(&self, agent: &AgentId) -> PortResult<()> {
        self.notify(Some(agent)).await;
        Ok(())
    }
}

#[derive(Clone)]
struct McpHandler {
    state: Arc<McpState>,
    app: Arc<AppService>,
}

fn to_tool(app: &AppId, t: &ToolSpec) -> Option<Tool> {
    let name = QualifiedToolName::new(app.clone(), &t.name).ok()?.to_string();
    let schema = t.input_schema.as_object().cloned().unwrap_or_default();
    let mut tool = Tool::new(name, t.description.clone(), Arc::new(schema));
    if let Some(title) = &t.title {
        tool = tool.with_title(title.clone());
    }
    if t.read_only {
        tool = tool.with_annotations(ToolAnnotations::new().read_only(true));
    }
    Some(tool)
}

impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_tool_list_changed().build())
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        // 2025-11-25 is what today's agents speak and has `tools/list_changed` over a session.
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25])
    }

    fn accepted_subscription_filter(&self, requested: &SubscriptionFilter) -> Option<SubscriptionFilter> {
        Some(requested.supported_by(&self.get_info().capabilities))
    }

    /// 2026-07-28: a client opens `subscriptions/listen` and we push `tools/list_changed` into it.
    async fn listen(&self, ctx: SubscriptionContext) -> Result<(), ErrorData> {
        let me = identity_of(&ctx.request_context().extensions).map(|i| i.id);
        let mut rx = self.state.changed.subscribe();
        loop {
            tokio::select! {
                () = ctx.cancelled() => return Ok(()),
                r = rx.recv() => match r {
                    Ok(target) => {
                        // someone else's grants changed: nothing to tell this listener
                        if target.is_some() && target != me { continue; }
                        if ctx.sink().notify_tool_list_changed().await.is_err() { return Ok(()); }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if ctx.sink().notify_tool_list_changed().await.is_err() { return Ok(()); }
                    }
                    Err(_) => return Ok(()),
                }
            }
        }
    }

    async fn on_initialized(&self, ctx: NotificationContext<RoleServer>) {
        if let Some(who) = identity_of(&ctx.extensions) {
            self.state.peers.write().push((who.id, ctx.peer.clone()));
        }
    }

    fn list_tools(
        &self,
        _req: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        // an agent sees the tools of the apps it is granted, and no others
        let who = identity_of(&ctx.extensions);
        let granted = |app: &AppId, tool: &ToolSpec| {
            who.as_ref().is_some_and(|w| {
                QualifiedToolName::new(app.clone(), &tool.name)
                    .is_ok_and(|q| w.authorize_tool_call(&q).is_ok())
            })
        };
        let tools = self.state.tools.read();
        let mut out: Vec<Tool> = tools
            .iter()
            .flat_map(|(app, ts)| ts.iter().filter(move |t| granted(app, t)).filter_map(|t| to_tool(app, t)))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        std::future::ready(Ok(ListToolsResult { tools: out, ..Default::default() }))
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let who = ctx
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<AgentIdentity>())
            .cloned()
            .ok_or_else(|| ErrorData::invalid_request("no agent identity", None))?;
        let name = QualifiedToolName::parse(&req.name)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let args = serde_json::Value::Object(req.arguments.unwrap_or_default());
        match self.app.call_tool(&who, &name, args).await {
            Ok(v) => Ok(CallToolResult::success(vec![ContentBlock::text(v.to_string())]).into()),
            // tool-level failure (permission, app error) is reported in-band so the model can see it
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into()),
        }
    }
}

/// Bearer-token middleware: 401 without a valid token, else attaches the `AgentIdentity`.
async fn authenticate(State(dir): State<Arc<dyn AgentDirectory>>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match token.and_then(|t| dir.authenticate(t)) {
        Some(identity) => {
            req.extensions_mut().insert(identity);
            next.run(req).await
        }
        None => {
            (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "unauthorized").into_response()
        }
    }
}

/// The `/mcp` router; merge it into the host's router.
///
/// Authentication is the per-agent bearer token only: a paired browser session (cookie) is **not** an MCP
/// credential, and an MCP token is not a session. rmcp's own `Host` allow-list is switched off because the
/// host mounts this router behind its Origin/Host guard (`agentos_http::HostGuard`), which also knows the
/// LAN addresses, hostname and `*.local` names that an rmcp allow-list of fixed strings cannot.
pub fn mcp_router(
    state: Arc<McpState>,
    app: Arc<AppService>,
    dir: Arc<dyn AgentDirectory>,
    cancel: CancellationToken,
) -> Router {
    let config = StreamableHttpServerConfig::default()
        .disable_allowed_hosts()
        .with_legacy_session_mode(true) // sessions => server-initiated list_changed on a GET stream
        .with_sse_keep_alive(Some(std::time::Duration::from_secs(15)))
        .with_cancellation_token(cancel);
    let service: StreamableHttpService<McpHandler, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(McpHandler { state: state.clone(), app: app.clone() }),
        Arc::default(),
        config,
    );
    Router::new().nest_service("/mcp", service).layer(axum::middleware::from_fn_with_state(dir, authenticate))
}
