//! The `/rpc` application protocol: maps JSON-RPC methods onto `AppService` use cases (driving side)
//! and fans `ServerEvent`s / notifications out to every connected client (driven side: it is the
//! `EventSink` and `Notifier` adapter).

use crate::rpc::{RpcHandler, RpcPeer};
use agentos_application::{
    ports::{EventSink, Notifier, PortResult, ServerEvent},
    AgentError, AgentPatch, AgentService, AppError, AppService, AppStatus, DeviceService, NewAgent,
    PortError, ViewError, ViewService, ViewSink,
};
use agentos_domain::{
    protocol::RpcError,
    ui::protocol::{codes, ClientNotification, ClientRequest, HostNotification, ParseError, SessionWelcome},
    ui::{KIT_VERSION, PROTO_VERSION},
    AgentId, AgentRuntime, AppId, ApprovalPolicy, DeviceId, Grants, Notification,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

/// A client that revokes its own device gets its answer before the socket is closed.
const SELF_REVOKE_GRACE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct RpcHub {
    app: OnceLock<Arc<AppService>>, // set once after construction (hub <-> service reference cycle)
    devices: OnceLock<Arc<DeviceService>>,
    views: OnceLock<Arc<ViewService>>,
    agents: OnceLock<Arc<AgentService>>,
    peers: Mutex<Vec<RpcPeer>>,
}

/// A view's notifications go straight into its socket's outbound queue.
struct PeerSink(RpcPeer);
impl ViewSink for PeerSink {
    fn send(&self, notification: HostNotification) {
        let (method, params) = notification.to_rpc();
        let _ = self.0.notify(method, params);
    }
}

impl RpcHub {
    pub fn bind(&self, app: Arc<AppService>, devices: Arc<DeviceService>) {
        let _ = self.app.set(app);
        let _ = self.devices.set(devices);
    }

    /// The view service behind `session.*`, `view.*`, `row.*`, `state.*` and `source.*`.
    pub fn bind_views(&self, views: Arc<ViewService>) {
        let _ = self.views.set(views);
    }

    /// The agent roster behind `agents.*`.
    pub fn bind_agents(&self, agents: Arc<AgentService>) {
        let _ = self.agents.set(agents);
    }

    async fn agents_request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let Some(agents) = self.agents.get() else { return Err(not_ready()) };
        let changed = |id: &AgentId| self.broadcast("agents.changed", &json!({ "id": id }));
        match method {
            "agents.list" => Ok(json!({ "agents": agents.list() })),
            "agents.create" => {
                let p: CreateAgentParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                let agent = agents
                    .create(NewAgent {
                        id: p.id,
                        name: p.name,
                        avatar: p.avatar,
                        role: p.role,
                        runtime: p.runtime,
                        grants: p.grants,
                        approval: p.approval,
                    })
                    .await
                    .map_err(|e| agent_err(&e))?;
                // the token is shown once, here
                let token = agents.rotate_token(&agent.id).await.map_err(|e| agent_err(&e))?;
                changed(&agent.id);
                Ok(json!({ "agent": agent, "token": token }))
            }
            "agents.update" => {
                let id = agent_id_of(&params)?;
                let patch = agent_patch(&params)?;
                let agent = agents.update(&id, patch).await.map_err(|e| agent_err(&e))?;
                changed(&id);
                Ok(json!({ "agent": agent }))
            }
            "agents.delete" => {
                let id = agent_id_of(&params)?;
                agents.delete(&id).await.map_err(|e| agent_err(&e))?;
                changed(&id);
                Ok(Value::Null)
            }
            "agents.grants.set" => {
                let id = agent_id_of(&params)?;
                let grants: Grants =
                    serde_json::from_value(params.get("grants").cloned().unwrap_or(Value::Null))
                        .map_err(|e| bad_params(&e))?;
                let agent = agents.set_grants(&id, grants).await.map_err(|e| agent_err(&e))?;
                changed(&id);
                Ok(json!({ "agent": agent }))
            }
            "agents.token.rotate" => {
                let id = agent_id_of(&params)?;
                let token = agents.rotate_token(&id).await.map_err(|e| agent_err(&e))?;
                Ok(json!({ "token": token }))
            }
            _ => Err(method_not_found(method)),
        }
    }

    async fn view_request(&self, peer: &RpcPeer, request: ClientRequest) -> Result<Value, RpcError> {
        let Some(views) = self.views.get() else { return Err(not_ready()) };
        let conn = peer.connection_id();
        match request {
            ClientRequest::Hello(h) => {
                if h.proto == 0 {
                    return Err(RpcError {
                        code: codes::UNSUPPORTED_PROTO,
                        message: format!(
                            "protocol {} is not served: this host speaks {PROTO_VERSION}",
                            h.proto
                        ),
                        data: None,
                    });
                }
                encode(&SessionWelcome {
                    proto: h.proto.min(PROTO_VERSION),
                    kit: KIT_VERSION.to_string(),
                    host_version: env!("CARGO_PKG_VERSION").to_owned(),
                    session_id: format!("c{conn:x}"),
                    resumed: false,
                })
            }
            ClientRequest::ViewOpen(o) => {
                let sink: Arc<dyn ViewSink> = Arc::new(PeerSink(peer.clone()));
                encode(&views.open(conn, sink, o).await.map_err(|e| view_err(&e))?)
            }
            ClientRequest::ViewSync(r) => {
                encode(&views.sync(conn, &r.view_id).await.map_err(|e| view_err(&e))?)
            }
            ClientRequest::ViewEvent(e) => encode(&views.event(conn, e).await.map_err(|e| view_err(&e))?),
            ClientRequest::RowSet(r) => encode(&views.row_set(conn, r).await.map_err(|e| view_err(&e))?),
            ClientRequest::RowRestore(r) => {
                encode(&views.row_restore(conn, r).await.map_err(|e| view_err(&e))?)
            }
            ClientRequest::SourceSet(r) => {
                encode(&views.source_set(conn, r).await.map_err(|e| view_err(&e))?)
            }
            ClientRequest::StateSet(_) => Err(RpcError {
                code: codes::APP_ERROR,
                message: "window state is not two-way bindable yet: bind a source field or a client value"
                    .into(),
                data: None,
            }),
            ClientRequest::ViewClose(r) => {
                views.close(conn, &r.view_id).await.map_err(|e| view_err(&e))?;
                Ok(Value::Null)
            }
        }
    }

    /// Hang up every socket of a device (it was revoked); the caller's own socket gets a short grace
    /// period so the revoke reply still arrives.
    fn disconnect_device(&self, device: &DeviceId, caller: &RpcPeer) {
        let doomed: Vec<RpcPeer> = self
            .peers
            .lock()
            .iter()
            .filter(|p| p.device().is_some_and(|d| d.id == *device))
            .cloned()
            .collect();
        for peer in doomed {
            if peer.is_same_connection(caller) {
                tokio::spawn(async move {
                    tokio::time::sleep(SELF_REVOKE_GRACE).await;
                    peer.close();
                });
            } else {
                peer.close();
            }
        }
    }

    async fn host_request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError> {
        let Some(devices) = self.devices.get() else { return Err(not_ready()) };
        match method {
            "host.devices.list" => {
                let current = peer.device().map(|d| d.id.clone());
                let devices = devices.list().await.map_err(|e| rpc_err(&AppError::Port(e)))?;
                let rows: Vec<Value> = devices
                    .iter()
                    .map(|d| {
                        let mut row = serde_json::to_value(d).unwrap_or(Value::Null);
                        row["current"] = json!(current.as_ref() == Some(&d.id));
                        row
                    })
                    .collect();
                Ok(json!({ "devices": rows }))
            }
            "host.devices.revoke" => {
                let p: RevokeParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                devices.revoke(&p.id).await.map_err(|e| rpc_err(&AppError::Port(e)))?;
                self.disconnect_device(&p.id, peer);
                Ok(Value::Null)
            }
            _ => Err(method_not_found(method)),
        }
    }

    fn broadcast(&self, method: &str, params: &Value) {
        // drop peers whose socket is gone
        self.peers.lock().retain(|p| p.notify(method, params.clone()).is_ok());
    }
}

fn agent_err(e: &AgentError) -> RpcError {
    let code = match e {
        AgentError::Domain(_) => -32001,
        AgentError::NotFound(_) => -32002,
        AgentError::Port(_) => -32003,
        AgentError::Exists(_) => -32004,
    };
    RpcError { code, message: e.to_string(), data: None }
}

fn agent_id_of(params: &Value) -> Result<AgentId, RpcError> {
    let id = params.get("id").and_then(Value::as_str).ok_or_else(|| RpcError {
        code: -32602,
        message: "invalid params: `id` is required".into(),
        data: None,
    })?;
    Ok(AgentId(id.to_owned()))
}

/// The fields `agents.update` may carry; `avatar: null` clears the avatar, an absent field is left alone.
fn agent_patch(params: &Value) -> Result<AgentPatch, RpcError> {
    let p: UpdateAgentParams = serde_json::from_value(params.clone()).map_err(|e| bad_params(&e))?;
    Ok(AgentPatch {
        name: p.name,
        avatar: params.get("avatar").map(|v| v.as_str().map(str::to_owned)),
        role: p.role,
        runtime: p.runtime,
        approval: p.approval,
    })
}

fn rpc_err(e: &AppError) -> RpcError {
    let code = match e {
        AppError::Domain(_) => -32001,
        AppError::Port(PortError::NotFound(_)) | AppError::NotInstalled(_) => -32002,
        AppError::Port(_) => -32003,
        AppError::AlreadyInstalled(_) => -32004,
        AppError::NotRunning { .. } => -32005,
    };
    RpcError { code, message: e.to_string(), data: None }
}

/// A result as JSON, or an internal error.
fn encode<T: serde::Serialize>(value: &T) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|e| RpcError {
        code: -32603,
        message: format!("could not encode the result: {e}"),
        data: None,
    })
}

fn view_err(e: &ViewError) -> RpcError {
    e.to_rpc()
}

fn not_ready() -> RpcError {
    RpcError { code: -32603, message: "host not ready".into(), data: None }
}

fn method_not_found(method: &str) -> RpcError {
    RpcError { code: -32601, message: format!("method `{method}` not found"), data: None }
}

fn bad_params(e: &serde_json::Error) -> RpcError {
    RpcError { code: -32602, message: format!("invalid params: {e}"), data: None }
}

#[derive(Deserialize)]
struct InstallParams {
    url: String,
}
#[derive(Deserialize)]
struct RevokeParams {
    id: DeviceId,
}
#[derive(Deserialize)]
struct AppParams {
    app: AppId,
}
#[derive(Deserialize)]
struct UninstallParams {
    app: AppId,
    #[serde(default)]
    purge: bool,
}

#[async_trait]
impl RpcHandler for RpcHub {
    async fn connected(&self, peer: RpcPeer) {
        self.peers.lock().push(peer);
    }

    async fn disconnected(&self, peer: &RpcPeer) {
        self.peers.lock().retain(|p| !p.is_same_connection(peer));
        if let Some(views) = self.views.get() {
            views.connection_closed(peer.connection_id()).await;
        }
    }

    async fn notification(&self, peer: &RpcPeer, method: &str, params: Value) {
        let Some(views) = self.views.get() else { return };
        match ClientNotification::from_rpc(method, params) {
            Ok(ClientNotification::ClientState(n)) => views.client_state(peer.connection_id(), n).await,
            Ok(ClientNotification::Bye) => views.connection_closed(peer.connection_id()).await,
            Ok(ClientNotification::Resize(_) | ClientNotification::Visibility(_)) => {}
            Err(e) => tracing::debug!(%method, error = %e, "ignored a malformed notification"),
        }
    }

    async fn request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError> {
        if method.starts_with("host.") {
            return self.host_request(peer, method, params).await;
        }
        if method.starts_with("agents.") {
            return self.agents_request(method, params).await;
        }
        // the view protocol first; anything it does not know is an app-management method below
        match ClientRequest::from_rpc(method, params.clone()) {
            Ok(request) => return self.view_request(peer, request).await,
            Err(ParseError::UnknownMethod(_)) => {}
            Err(e) => return Err(e.to_rpc_error()),
        }
        let Some(app) = self.app.get() else { return Err(not_ready()) };
        match method {
            "app.install" => {
                let p: InstallParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                let id = app.install_from_url(&p.url).await.map_err(|e| rpc_err(&e))?;
                Ok(json!({ "app": id }))
            }
            "app.uninstall" => {
                let p: UninstallParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                app.uninstall(&p.app, p.purge).await.map_err(|e| rpc_err(&e))?;
                Ok(Value::Null)
            }
            "app.list" => {
                let apps: Vec<Value> = app
                    .apps()
                    .into_iter()
                    .map(|a| {
                        let (state, message) = match &a.status {
                            AppStatus::Running => ("running", None),
                            AppStatus::Disabled => ("disabled", None),
                            AppStatus::Failed(m) => ("failed", Some(m.clone())),
                        };
                        json!({ "id": a.id, "name": a.name, "version": a.version, "state": state, "message": message })
                    })
                    .collect();
                Ok(json!({ "apps": apps }))
            }
            "app.enable" | "app.disable" | "app.deleteData" | "app.reload" => {
                let p: AppParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                match method {
                    "app.enable" => app.enable(&p.app).await,
                    "app.disable" => app.disable(&p.app).await,
                    "app.deleteData" => app.delete_data(&p.app).await,
                    _ => app.reload(&p.app).await,
                }
                .map_err(|e| rpc_err(&e))?;
                Ok(Value::Null)
            }
            _ => Err(method_not_found(method)),
        }
    }
}

#[async_trait]
impl EventSink for RpcHub {
    async fn emit(&self, event: ServerEvent) {
        match event {
            ServerEvent::AppInstalled(app) => self.broadcast("app.installed", &json!({ "app": app })),
            ServerEvent::AppUninstalled(app) => self.broadcast("app.uninstalled", &json!({ "app": app })),
            ServerEvent::AppEnabled(app) => self.broadcast("app.enabled", &json!({ "app": app })),
            ServerEvent::AppDisabled(app) => self.broadcast("app.disabled", &json!({ "app": app })),
            ServerEvent::AppFailed { app, message } => {
                self.broadcast("app.failed", &json!({ "app": app, "message": message }));
            }
            ServerEvent::Notification(n) => {
                if let Ok(v) = serde_json::to_value(n) {
                    self.broadcast("notify", &v);
                }
            }
        }
    }
}

#[async_trait]
impl Notifier for RpcHub {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        self.emit(ServerEvent::Notification(n)).await;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAgentParams {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    role: String,
    runtime: AgentRuntime,
    #[serde(default)]
    grants: Grants,
    #[serde(default)]
    approval: ApprovalPolicy,
}

#[derive(Deserialize)]
struct UpdateAgentParams {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    runtime: Option<AgentRuntime>,
    #[serde(default)]
    approval: Option<ApprovalPolicy>,
}
