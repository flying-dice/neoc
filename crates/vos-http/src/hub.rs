//! The `/rpc` application protocol: maps JSON-RPC methods onto `AppService` use cases (driving side)
//! and fans `ServerEvent`s / notifications out to every connected client (driven side: it is the
//! `EventSink` and `Notifier` adapter).

use crate::rpc::{PreparedNotification, RpcHandler, RpcPeer};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tracing::Instrument;
use vos_application::{
    ports::{EventSink, Notifier, PortResult, ServerEvent, Shell},
    AgentError, AgentPatch, AgentService, AppError, AppInfo, AppService, AppStatus, DeviceService,
    FilesService, Listed, NewAgent, Place, PortError,
};
use vos_domain::{
    api::{
        AgentList, AgentRow, AppEvent, AppList, AppState, AppSummary, AppWindow, DeviceList, DeviceRow,
        FileEntry, FileEntryKind, FileList, FileLocations,
    },
    limits,
    protocol::{
        self, codes, AppNotification, HostNotification, RpcError, SessionHello, SessionWelcome, WindowOpen,
    },
    source_label, AgentId, AppId, DeviceId, FileError, FileErrorKind, FileKind, Grants, Notification,
    KIT_VERSION, PROTO_VERSION,
};

/// A client that revokes its own device gets its answer before the socket is closed.
const SELF_REVOKE_GRACE: Duration = Duration::from_millis(250);

/// An app's event is not queued for a client that already has more than this many bytes waiting (1 MiB, 1/32 of the bound that
/// closes a connection): an app that emits faster than a client's link can take it costs that client its events, not its connection.
pub const EVENT_QUEUE_BYTES: usize = 1 << 20;

#[derive(Default)]
pub struct RpcHub {
    app: OnceLock<Arc<AppService>>, // set once after construction (hub <-> service reference cycle)
    devices: OnceLock<Arc<DeviceService>>,
    agents: OnceLock<Arc<AgentService>>,
    files: OnceLock<Arc<FilesService>>,
    peers: Mutex<Vec<RpcPeer>>,
}

impl RpcHub {
    pub fn bind(&self, app: Arc<AppService>, devices: Arc<DeviceService>) {
        let _ = self.app.set(app);
        let _ = self.devices.set(devices);
    }

    /// The agent roster behind `agents.*`.
    pub fn bind_agents(&self, agents: Arc<AgentService>) {
        let _ = self.agents.set(agents);
    }

    /// The places behind `host.files.*`.
    pub fn bind_files(&self, files: Arc<FilesService>) {
        let _ = self.files.set(files);
    }

    /// `host.files.*`: the person browsing files from the Files page. One family of methods for both kinds of place.
    ///
    /// - With `app`, the place is the Documents of that installed app (never its cache or scratch files). A path is
    ///   relative to it and keeps the rules of an app's own `app.files` calls: no `..`, no absolute path, no symbolic
    ///   link followed, and an error never names a host path.
    /// - Without `app`, the place is This Computer: the host's whole file system. A path is absolute (`/Users/me`;
    ///   `/` or nothing is the top, on Windows the drives), links are followed, and the operating system alone says
    ///   no; what it says (no permission, not there) is the answer for that entry.
    ///
    /// A file's bytes do not travel here: `GET` and `PUT /files/...` download and upload one. A listing carries at
    /// most `MAX_LIST_ENTRIES` entries and says when there were more.
    ///
    /// Security: This Computer is unrestricted on purpose. The host is the person's own machine, and a paired device
    /// can already install apps (arbitrary code as the user running the host) and run containers on it, so reading
    /// and writing any file the host's process can gives it nothing it did not have. What holds is the door: an
    /// unpaired client cannot reach `/rpc` at all (the session guard answers before the socket is upgraded). An
    /// operator who wants none of it sets `host_files = false`.
    async fn files_request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let Some(files) = self.files.get() else { return Err(not_ready()) };
        let open = |app: Option<AppId>| {
            files.open(&app.map_or(Place::Computer, Place::App)).map_err(|e| file_err(&e))
        };
        match method {
            "host.files.locations" => {
                no_params(params)?;
                encode(&FileLocations {
                    computer: files.shares_computer(),
                    home: files.home().map(|home| home.to_string()),
                    max_upload_bytes: limits::MAX_UPLOAD_BYTES as u64,
                })
            }
            "host.files.list" => {
                let p: FileListParams = parse(params)?;
                let listing = open(p.app)?.list(&p.path).await.map_err(|e| file_err(&e))?;
                encode(&FileList {
                    entries: listing.entries.iter().map(file_entry).collect(),
                    truncated: listing.truncated,
                })
            }
            "host.files.remove" => {
                let p: FileRemoveParams = parse(params)?;
                open(p.app)?.remove(&p.path, p.recursive).await.map_err(|e| file_err(&e))?;
                Ok(Value::Null)
            }
            "host.files.createDir" => {
                let p: FilePathParams = parse(params)?;
                open(p.app)?.create_dir(&p.path).await.map_err(|e| file_err(&e))?;
                Ok(Value::Null)
            }
            "host.files.rename" => {
                let p: FileRenameParams = parse(params)?;
                open(p.app)?.rename(&p.from, &p.to).await.map_err(|e| file_err(&e))?;
                Ok(Value::Null)
            }
            _ => Err(method_not_found(method)),
        }
    }

    async fn agents_request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let Some(agents) = self.agents.get() else { return Err(not_ready()) };
        let changed = |id: &AgentId| self.broadcast("agents.changed", &json!({ "id": id }));
        match method {
            "agents.list" => {
                no_params(params)?;
                encode(&AgentList { agents: agents.list().iter().map(AgentRow::from).collect() })
            }
            "agents.create" => {
                let p: CreateAgentParams = parse(params)?;
                let agent = agents
                    .create(NewAgent {
                        id: p.id,
                        name: p.name,
                        avatar: p.avatar,
                        role: p.role,
                        grants: p.grants,
                    })
                    .await
                    .map_err(|e| agent_err(&e))?;
                // the token is shown once, here
                let token = agents.rotate_token(&agent.id).await.map_err(|e| agent_err(&e))?;
                changed(&agent.id);
                Ok(json!({ "agent": AgentRow::from(&agent), "token": token }))
            }
            "agents.update" => {
                let p: UpdateAgentParams = parse(params)?;
                let id = p.id.clone();
                let agent = agents.update(&id, p.into_patch()).await.map_err(|e| agent_err(&e))?;
                changed(&id);
                Ok(json!({ "agent": AgentRow::from(&agent) }))
            }
            "agents.delete" => {
                let id = parse::<AgentIdParams>(params)?.id;
                agents.delete(&id).await.map_err(|e| agent_err(&e))?;
                changed(&id);
                Ok(Value::Null)
            }
            "agents.grants.set" => {
                let p: GrantsParams = parse(params)?;
                let agent = agents.set_grants(&p.id, p.grants).await.map_err(|e| agent_err(&e))?;
                changed(&p.id);
                Ok(json!({ "agent": AgentRow::from(&agent) }))
            }
            "agents.token.rotate" => {
                let id = parse::<AgentIdParams>(params)?.id;
                let token = agents.rotate_token(&id).await.map_err(|e| agent_err(&e))?;
                Ok(json!({ "token": token }))
            }
            _ => Err(method_not_found(method)),
        }
    }

    /// `session.hello`: the shell's handshake. The host answers with the protocol it will speak (the lower of the
    /// two) and a name for this connection; it keeps nothing of a session, so a welcome is never `resumed`.
    fn hello(peer: &RpcPeer, params: Value) -> Result<Value, RpcError> {
        let hello: SessionHello = protocol::parse("session.hello", params).map_err(|e| e.to_rpc_error())?;
        if hello.proto == 0 {
            return Err(RpcError {
                code: codes::UNSUPPORTED_PROTO,
                message: format!("protocol {} is not served: this host speaks {PROTO_VERSION}", hello.proto),
                data: None,
            });
        }
        encode(&SessionWelcome {
            proto: hello.proto.min(PROTO_VERSION),
            kit: KIT_VERSION.to_string(),
            host_version: env!("CARGO_PKG_VERSION").to_owned(),
            session_id: format!("c{:x}", peer.connection_id()),
            resumed: false,
        })
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
        if method.starts_with("host.files.") {
            return self.files_request(method, params).await;
        }
        if method.starts_with("host.telemetry.") {
            return telemetry_request(method, params);
        }
        let Some(devices) = self.devices.get() else { return Err(not_ready()) };
        match method {
            "host.devices.list" => {
                no_params(params)?;
                let current = peer.device().map(|d| d.id.clone());
                let devices = devices.list().await.map_err(|e| rpc_err(&AppError::Port(e)))?;
                encode(&DeviceList {
                    devices: devices
                        .into_iter()
                        .map(|d| DeviceRow {
                            current: current.as_ref() == Some(&d.id),
                            id: d.id.0,
                            name: d.name,
                            created_at: d.created_at,
                            last_seen_at: d.last_seen_at,
                        })
                        .collect(),
                })
            }
            "host.devices.revoke" => {
                let p: RevokeParams = parse(params)?;
                devices.revoke(&p.id).await.map_err(|e| rpc_err(&AppError::Port(e)))?;
                self.disconnect_device(&p.id, peer);
                Ok(Value::Null)
            }
            _ => Err(method_not_found(method)),
        }
    }

    fn broadcast(&self, method: &str, params: &Value) {
        self.deliver(method, params, None);
    }

    /// Send one notification to every connected client: serialised once, outside the peers lock (a slow serialisation or a
    /// crowd of clients never holds up a connect or a disconnect). A client that has more than `skip_above` bytes waiting is
    /// passed over for this one (see [`EVENT_QUEUE_BYTES`]); peers whose socket is gone are forgotten.
    fn deliver(&self, method: &str, params: &Value, skip_above: Option<usize>) {
        let Ok(message) = PreparedNotification::new(method, params.clone()) else { return };
        let peers: Vec<RpcPeer> = self.peers.lock().clone();
        let mut gone = Vec::new();
        for peer in &peers {
            if skip_above.is_some_and(|limit| peer.queued_bytes() > limit) {
                tracing::debug!(
                    connection = peer.connection_id(),
                    method,
                    "not sent: this client is behind on what it has"
                );
                continue;
            }
            if peer.send_prepared(&message).is_err() {
                gone.push(peer.connection_id());
            }
        }
        if !gone.is_empty() {
            self.peers.lock().retain(|p| !gone.contains(&p.connection_id()));
        }
    }
}

/// One installed app as clients see it.
fn summary(a: &AppInfo) -> AppSummary {
    let (state, message) = match &a.status {
        AppStatus::Running => (AppState::Running, None),
        AppStatus::Disabled => (AppState::Disabled, None),
        AppStatus::Failed(m) => (AppState::Failed, Some(m.clone())),
    };
    AppSummary {
        id: a.id.to_string(),
        name: a.name.clone(),
        version: a.version.clone(),
        state,
        message,
        icon: a.display.icon.clone(),
        tint: a.display.tint.clone(),
        description: a.display.description.clone(),
        window: a.display.window.as_ref().map(|w| AppWindow {
            w: w.w,
            h: w.h,
            min_w: w.min_w,
            min_h: w.min_h,
        }),
        source: source_label(&a.source_url),
        permissions: a.display.permissions.iter().map(|p| (*p).to_owned()).collect(),
    }
}

/// One entry of a folder as clients see it.
fn file_entry(listed: &Listed) -> FileEntry {
    let info = &listed.info;
    FileEntry {
        link: listed.link,
        problem: listed.problem.clone(),
        name: info.name.clone(),
        kind: match info.kind {
            FileKind::File => FileEntryKind::File,
            FileKind::Dir => FileEntryKind::Dir,
            FileKind::Symlink => FileEntryKind::Link,
            FileKind::Other => FileEntryKind::Other,
        },
        size: info.size,
        // seconds with a fraction, to whole milliseconds: a time before 1970 or one that is not a number is "not recorded"
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        modified_at: info.modified.filter(|s| s.is_finite() && *s >= 0.0).map(|s| (s * 1000.0) as u64),
    }
}

/// A failed file call: the use case's message (which names no host path) and its stable kind as `data.kind`.
fn file_err(e: &FileError) -> RpcError {
    let code = match e.kind {
        FileErrorKind::NotFound => -32002,
        FileErrorKind::InvalidInput => codes::INVALID_PARAMS,
        FileErrorKind::AlreadyExists => -32004,
        _ => -32003,
    };
    RpcError { code, message: e.message.clone(), data: Some(json!({ "kind": e.kind.as_str() })) }
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

/// What an app page's `rpc` gets back when its handler fails: the handler's own message, as the app wrote it (an
/// error it raised, `ctx:fail`, or a result it could not return), under the app-error code. Anything else (the app
/// is not running, it is not installed) is an ordinary host error.
fn handler_err(e: &AppError) -> RpcError {
    match e {
        AppError::Port(PortError::Rejected(message)) => {
            RpcError { code: codes::APP_ERROR, message: message.clone(), data: None }
        }
        AppError::Port(PortError::Rpc { code, message, data }) => {
            RpcError { code: *code, message: message.clone(), data: data.clone() }
        }
        other => rpc_err(other),
    }
}

/// A result as JSON, or an internal error.
fn encode<T: serde::Serialize>(value: &T) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|e| RpcError {
        code: -32603,
        message: format!("could not encode the result: {e}"),
        data: None,
    })
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

/// Params of a method as its typed struct: a by-name object with exactly the declared fields. Anything else
/// (a missing or unknown field, a wrong type, an array) is a `-32602`, never a silent default.
fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|e| bad_params(&e))
}

/// A method that takes nothing: absent, `null` and `{}` are all fine, any field is not.
fn no_params(params: Value) -> Result<(), RpcError> {
    parse::<Option<NoParams>>(params).map(|_| ())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UrlParams {
    url: String,
}
/// `app.install`: `sha256` (from `app.preview`) pins the install to exactly the ref the person reviewed. It is
/// required: an install over the wire is always of something that was previewed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallParams {
    url: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeParams {
    id: DeviceId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppParams {
    app: AppId,
}
/// `host.files.list`: `path` is a directory of the place; absent or `""` is its top. `app` absent is This Computer,
/// where a path is absolute.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileListParams {
    #[serde(default)]
    app: Option<AppId>,
    #[serde(default)]
    path: String,
}
/// `host.files.remove`: a file, or a folder. A folder that is not empty goes only with `recursive: true` (the page
/// asks the person first, and says how much is in it); without it the answer is `not_empty`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRemoveParams {
    #[serde(default)]
    app: Option<AppId>,
    path: String,
    #[serde(default)]
    recursive: bool,
}
/// `host.files.createDir`: the new folder, by its path.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePathParams {
    #[serde(default)]
    app: Option<AppId>,
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRenameParams {
    #[serde(default)]
    app: Option<AppId>,
    from: String,
    to: String,
}
/// `host.telemetry.*` (NEOC-521 POC): what was recorded, read by a paired client; `ingest` is the shell passing on what
/// a frame of `app` recorded (the shell names the app, never the frame).
fn telemetry_request(method: &str, params: Value) -> Result<Value, RpcError> {
    use crate::telemetry;
    match method {
        "host.telemetry.query" => encode(&telemetry::query(&parse(params)?)),
        "host.telemetry.traces" => {
            let p: TracesParams = parse(params)?;
            encode(&telemetry::traces(p.app.as_deref(), p.limit))
        }
        "host.telemetry.ingest" => {
            let p: IngestParams = parse(params)?;
            Ok(json!({ "kept": telemetry::ingest(p.app.as_str(), p.batch) }))
        }
        _ => Err(method_not_found(method)),
    }
}
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct TracesParams {
    app: Option<String>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
struct IngestParams {
    app: AppId,
    #[serde(flatten)]
    batch: crate::telemetry::Batch,
}
/// `app.call`: the shell forwards an app page's `rpc(method, args)`. `app` is the app of the frame the message
/// came from (the shell decides it, never the page); `args` is whatever JSON the page sent.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallParams {
    app: AppId,
    method: String,
    #[serde(default)]
    args: Value,
    /// The W3C trace context of the page's call (NEOC-521 POC): the handler's span joins the page's trace.
    #[serde(default)]
    traceparent: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UninstallParams {
    app: AppId,
    #[serde(default)]
    purge: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentIdParams {
    id: AgentId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantsParams {
    id: AgentId,
    grants: Grants,
}

#[async_trait]
impl RpcHandler for RpcHub {
    async fn connected(&self, peer: RpcPeer) {
        self.peers.lock().push(peer);
    }

    async fn disconnected(&self, peer: &RpcPeer) {
        self.peers.lock().retain(|p| !p.is_same_connection(peer));
    }

    /// `session.bye` (the client is going away) needs nothing done: the socket closing is what ends a connection.
    async fn notification(&self, _peer: &RpcPeer, method: &str, _params: Value) {
        if method != "session.bye" {
            tracing::debug!(%method, "ignored an unknown notification");
        }
    }

    async fn request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError> {
        if method.starts_with("host.") {
            return self.host_request(peer, method, params).await;
        }
        if method.starts_with("agents.") {
            return self.agents_request(method, params).await;
        }
        if method == "session.hello" {
            return Self::hello(peer, params);
        }
        let Some(app) = self.app.get() else { return Err(not_ready()) };
        match method {
            "app.install" => {
                let p: InstallParams = parse(params)?;
                let id = app.install_previewed(&p.url, &p.sha256).await.map_err(|e| rpc_err(&e))?;
                Ok(json!({ "app": id }))
            }
            "app.uninstall" => {
                let p: UninstallParams = parse(params)?;
                app.uninstall(&p.app, p.purge).await.map_err(|e| rpc_err(&e))?;
                Ok(Value::Null)
            }
            "app.list" => {
                no_params(params)?;
                encode(&AppList { apps: app.apps().iter().map(summary).collect() })
            }
            "app.available" => {
                no_params(params)?;
                encode(&app.catalog().await)
            }
            "app.preview" => {
                let p: UrlParams = parse(params)?;
                encode(&app.preview(&p.url).await.map_err(|e| rpc_err(&e))?)
            }
            "app.call" => {
                let p: CallParams = parse(params)?;
                let (trace_id, parent) =
                    p.traceparent.as_deref().and_then(crate::telemetry::parse_traceparent).unzip();
                let span = tracing::info_span!(
                    "app.call",
                    app = %p.app,
                    method = %p.method,
                    trace_id = trace_id.as_deref().unwrap_or(""),
                    parent_span_id = parent.as_deref().unwrap_or(""),
                    outcome = tracing::field::Empty,
                    error = tracing::field::Empty,
                );
                let out = app
                    .call_handler(&p.app, &p.method, p.args)
                    .instrument(span.clone())
                    .await
                    .map_err(|e| handler_err(&e));
                match &out {
                    Ok(_) => span.record("outcome", "ok"),
                    Err(e) => span.record("outcome", "error").record("error", e.message.as_str()),
                };
                out
            }
            "app.enable" | "app.disable" | "app.deleteData" | "app.reload" => {
                let p: AppParams = parse(params)?;
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
                let note = HostNotification::Notify(AppNotification {
                    app: n.app.to_string(),
                    title: n.title,
                    body: n.body,
                });
                let (method, params) = note.to_rpc();
                self.broadcast(method, &params);
            }
        }
    }
}

/// What apps ask of the shell: opening a window and an event both go to every connected client.
#[async_trait]
impl Shell for RpcHub {
    async fn open_window(&self, app: &AppId, window: &str, params: Map<String, Value>) {
        let note = HostNotification::WindowOpen(WindowOpen {
            app: app.to_string(),
            window: window.to_owned(),
            params: (!params.is_empty()).then_some(params),
        });
        let (method, params) = note.to_rpc();
        self.broadcast(method, &params);
    }
    /// `app.emit`: every connected shell hears it and hands it to the frames of that app. The frames of other
    /// apps never see it: the shell routes by the app named here.
    async fn emit_event(&self, app: &AppId, name: &str, data: Value) {
        if let Ok(event) =
            serde_json::to_value(AppEvent { app: app.to_string(), name: name.to_owned(), data })
        {
            // an event is a signal ("something changed, fetch again"), not a log: a client that is behind does not get it queued
            // behind everything else until the byte bound closes its connection
            self.deliver("app.event", &event, Some(EVENT_QUEUE_BYTES));
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
    #[serde(default)]
    grants: Grants,
}

/// The fields `agents.update` may carry; `avatar: null` clears the avatar, an absent field is left alone.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateAgentParams {
    id: AgentId,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[allow(clippy::option_option)]
    // "absent" and "null" mean different things (AgentPatch says the same)
    avatar: Option<Option<String>>,
    #[serde(default)]
    role: Option<String>,
}

impl UpdateAgentParams {
    fn into_patch(self) -> AgentPatch {
        AgentPatch { name: self.name, avatar: self.avatar, role: self.role }
    }
}

/// A field that is present, null or not: `Some(None)` for `null`, so "absent" and "null" stay apart.
#[allow(clippy::option_option)] // see the field it serves
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::test_peer;

    /// What an app's event does to a client that cannot keep up with it: it is skipped for that client (which keeps its
    /// connection), and a client that is keeping up gets it; any other notification is still queued for both.
    #[tokio::test]
    async fn an_app_event_is_not_queued_for_a_client_that_is_behind_on_what_it_has() {
        let hub = RpcHub::default();
        let (fast, mut fast_rx) = test_peer(1);
        let (slow, mut slow_rx) = test_peer(2);
        // the slow client has more than the event limit waiting (a message nobody has taken off the queue yet)
        let pad = "x".repeat(EVENT_QUEUE_BYTES + 10);
        slow.notify("pad", json!({ "pad": pad })).unwrap();
        assert!(slow.queued_bytes() > EVENT_QUEUE_BYTES);
        hub.peers.lock().extend([fast.clone(), slow.clone()]);

        hub.deliver(
            "app.event",
            &json!({ "app": "a", "name": "tick", "data": null }),
            Some(EVENT_QUEUE_BYTES),
        );
        assert!(fast_rx.recv().await.unwrap().contains("app.event"));
        let first = slow_rx.recv().await.unwrap();
        assert!(first.contains("pad") && !first.contains("app.event"), "only what was already waiting");
        assert!(slow_rx.try_recv().is_err(), "the event was not queued for the slow client");
        assert_eq!(hub.peers.lock().len(), 2, "and the slow client keeps its connection");

        // everything else (an uninstall, say) is queued for both, serialised once
        hub.broadcast("app.uninstalled", &json!({ "app": "a" }));
        assert!(fast_rx.recv().await.unwrap().contains("app.uninstalled"));
        assert!(slow_rx.recv().await.unwrap().contains("app.uninstalled"));
    }

    #[tokio::test]
    async fn a_peer_whose_connection_is_over_is_forgotten_by_the_next_delivery() {
        let hub = RpcHub::default();
        let (gone, rx) = test_peer(1);
        let (alive, mut alive_rx) = test_peer(2);
        drop(rx);
        hub.peers.lock().extend([gone, alive]);
        hub.broadcast("app.installed", &json!({ "app": "a" }));
        assert!(alive_rx.recv().await.unwrap().contains("app.installed"));
        assert_eq!(hub.peers.lock().len(), 1);
    }
}
