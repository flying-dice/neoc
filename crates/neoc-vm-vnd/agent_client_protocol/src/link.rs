//! The connection under `vnd:agent_client_protocol`: the WebSocket, and the SDK's v1 client running over it.
//!
//! The socket is opened here (the upgrade's status, this end's address and the headers are ours to see and set)
//! and handed to the SDK as its `Lines` transport, one text frame per JSON-RPC message. The SDK's connection runs on
//! a task of the engine's [`Life`] until the script closes it, every handle to it is gone, or the agent hangs up.
//! Each session the script starts or loads is an SDK `ActiveSession`, which receives what the agent sends for that
//! session (its updates, its permission requests, the answer to a prompt) on a stream of its own; a turn reads it.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use agent_client_protocol::v1::{ActiveSession, ConnectionTo, SessionMessage, schema};
use agent_client_protocol::{Agent, Client, Dispatch, Lines};
use futures_util::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use mlua_ir::ir::{Field, Ty, TypeBody};
use mlua_ir::{Registry, Shape};
use parking_lot::Mutex;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message, http};

use crate::Data;
use crate::agent_client_protocol::{
    AcpError, Implementation, InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOutcome, PromptRequest, SessionUpdate, StopReason,
    ToolCallUpdate, TurnEvent,
};
use neoc_vm_core::lifecycle::Life;

/// How much of an error response's body is quoted in the error, in characters.
const ERROR_BODY: usize = 2048;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

fn invalid(reason: String) -> AcpError {
    AcpError::InvalidInput { reason }
}

// ---------------------------------------------------------------------------------------------------------------
// Between the script's tables and the SDK's types
// ---------------------------------------------------------------------------------------------------------------

/// `mime_type` as `mimeType`.
fn camel_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '_' && !out.is_empty() {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `value` with every object key in camelCase. Only for what a script sends: those types hold no map (every key is
/// a field name).
fn camel(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            Value::Object(fields.into_iter().map(|(key, value)| (camel_key(&key), camel(value))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(camel).collect()),
        other => other,
    }
}

/// `mimeType` as `mime_type`.
fn snake_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 2);
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The declared types a script's table is made of, by name.
type Declared = HashMap<String, TypeBody>;

/// `value`, from the wire, as the script's type `ty` has it: the fields that type declares under their snake_case
/// names, and the ones it does not declare left out (a record of the module refuses a key it does not have). The
/// shape says what is a field: data whose shape the agent decides (a tool's raw input) is kept as it came.
fn shaped(value: Value, ty: &Ty, declared: &Declared) -> Value {
    match (ty, value) {
        (Ty::Optional { inner }, value) => shaped(value, inner, declared),
        (Ty::List { item }, Value::Array(items)) => {
            Value::Array(items.into_iter().map(|value| shaped(value, item, declared)).collect())
        }
        (Ty::Map { value: of, .. }, Value::Object(entries)) => Value::Object(
            entries.into_iter().map(|(key, value)| (key, shaped(value, of, declared))).collect(),
        ),
        (Ty::Named { name }, value) => match (declared.get(name), value) {
            (Some(TypeBody::Record { fields }), Value::Object(given)) => {
                Value::Object(record(given, fields, declared))
            }
            (Some(TypeBody::TaggedUnion { tag, variants }), Value::Object(given)) => {
                let kind = given.get(tag).and_then(Value::as_str).map(str::to_owned);
                match variants.iter().find(|variant| Some(&variant.tag_value) == kind.as_ref()) {
                    Some(variant) => {
                        let mut fields = record(given, &variant.fields, declared);
                        fields.insert(tag.clone(), Value::String(variant.tag_value.clone()));
                        Value::Object(fields)
                    }
                    // a kind the type does not have: refused as it is
                    None => Value::Object(given),
                }
            }
            (Some(TypeBody::Alias { ty }), value) => shaped(value, ty, declared),
            (_, value) => value,
        },
        (_, value) => value,
    }
}

fn record(
    given: serde_json::Map<String, Value>,
    fields: &[Field],
    declared: &Declared,
) -> serde_json::Map<String, Value> {
    given
        .into_iter()
        .filter_map(|(key, value)| {
            let name = snake_key(&key);
            let field = fields.iter().find(|field| field.name == name)?;
            Some((name, shaped(value, &field.ty, declared)))
        })
        .collect()
}

/// `value`, from the wire, read as the script's type `T`.
fn script<T: Shape + DeserializeOwned>(value: Value) -> Result<T, serde_json::Error> {
    let mut registry = Registry::new();
    let ty = T::shape(&mut registry);
    let declared: Declared = registry.into_types().into_iter().map(|def| (def.name, def.body)).collect();
    serde_json::from_value(shaped(value, &ty, &declared))
}

/// A script's table as the SDK's type: the wire's names, and no `type` on a `stdio` MCP server.
fn to_sdk<T: DeserializeOwned>(params: &impl Serialize) -> Result<T, AcpError> {
    let mut value = camel(serde_json::to_value(params).map_err(|e| invalid(e.to_string()))?);
    if let Some(Value::Array(servers)) = value.get_mut("mcpServers") {
        for server in servers.iter_mut().filter_map(Value::as_object_mut) {
            if server.get("type").and_then(Value::as_str) == Some("stdio") {
                server.remove("type");
            }
        }
    }
    serde_json::from_value(value).map_err(|e| invalid(e.to_string()))
}

/// The SDK's value as the script's table.
fn from_sdk<T: Shape + DeserializeOwned>(value: &impl Serialize, what: &str) -> Result<T, AcpError> {
    serde_json::to_value(value)
        .and_then(script)
        .map_err(|e| AcpError::Protocol { reason: format!("{what} is not what ACP v1 says: {e}") })
}

/// A `session/update` as a script reads it: the wire's `sessionUpdate` is its `type`. An update that does not read
/// as its kind (or of a kind the module does not know) is `other`, as it was sent.
fn session_update(update: Value) -> SessionUpdate {
    let kind = update.get("sessionUpdate").and_then(Value::as_str).unwrap_or_default().to_owned();
    if let Value::Object(fields) = &update {
        let mut typed = fields.clone();
        typed.remove("sessionUpdate");
        typed.insert("type".to_owned(), Value::String(kind.clone()));
        if kind != "other"
            && let Ok(read) = script(Value::Object(typed))
        {
            return read;
        }
    }
    SessionUpdate::Other { session_update: kind, update: Data(update) }
}

/// A request that failed on `connection`, as the script's error.
fn failure(connection: &ConnectionTo<Agent>, error: &agent_client_protocol::Error) -> AcpError {
    if agent_client_protocol::is_incoming_transport_closed(error) || connection.is_incoming_closed() {
        return AcpError::Closed { reason: "the agent closed the connection".to_owned() };
    }
    // a send to a connection that has ended fails with an internal error of its own
    if agent_client_protocol::is_connection_terminated(error) || connection.termination().is_some() {
        return AcpError::Closed { reason: "the connection is closed".to_owned() };
    }
    let reason = match &error.data {
        Some(data) => format!("{} ({data})", error.message),
        None => error.message.clone(),
    };
    AcpError::Rpc { code: i64::from(i32::from(error.code)), reason }
}

fn cancelled() -> schema::RequestPermissionResponse {
    schema::RequestPermissionResponse::new(schema::RequestPermissionOutcome::Cancelled)
}

// ---------------------------------------------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------------------------------------------

/// The TLS client settings of `vnd:hyper`: rustls over the `ring` provider, the certificate checked by the platform
/// verifier.
fn tls_connector() -> Result<tokio_rustls::TlsConnector, AcpError> {
    use rustls_platform_verifier::BuilderVerifierExt;
    use tokio_rustls::rustls::{self, ClientConfig};

    let failed = |e: String| AcpError::Connect { reason: format!("TLS: {e}") };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| failed(e.to_string()))?
        .with_platform_verifier()
        .map_err(|e| failed(format!("the platform certificate verifier is unavailable: {e}")))?
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

/// What a failed WebSocket handshake is to a script.
fn handshake_error(e: tungstenite::Error) -> AcpError {
    match e {
        tungstenite::Error::Http(response) => {
            let body = response.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            let body: String = body.chars().take(ERROR_BODY).collect();
            AcpError::Status { code: response.status().as_u16(), body: body.trim().to_owned() }
        }
        tungstenite::Error::Io(e) => AcpError::Connect { reason: e.to_string() },
        other => AcpError::Protocol { reason: format!("the WebSocket upgrade failed: {other}") },
    }
}

/// The WebSocket to `url`, with `headers` on the upgrade, as the SDK's `Lines`; and this end's IP address.
async fn open(
    url: &str,
    headers: BTreeMap<String, String>,
) -> Result<
    (
        Lines<
            impl futures_util::Sink<String, Error = std::io::Error> + Send + 'static,
            impl futures_util::Stream<Item = std::io::Result<String>> + Send + 'static,
        >,
        String,
    ),
    AcpError,
> {
    let mut upgrade = url.into_client_request().map_err(|e| invalid(format!("{url}: {e}")))?;
    let uri = upgrade.uri().clone();
    let tls = match uri.scheme_str() {
        Some("ws") => false,
        Some("wss") => true,
        _ => return Err(invalid(format!("{url}: not a ws:// or wss:// URL"))),
    };
    let host = uri.host().ok_or_else(|| invalid(format!("{url}: no host")))?;
    let host = host.trim_start_matches('[').trim_end_matches(']').to_owned();
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    for (name, value) in headers {
        let header = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| invalid(format!("header `{name}`: {e}")))?;
        let value =
            http::HeaderValue::from_str(&value).map_err(|e| invalid(format!("header `{name}`: {e}")))?;
        upgrade.headers_mut().insert(header, value);
    }

    let connect = |e: std::io::Error| AcpError::Connect { reason: format!("{host}:{port}: {e}") };
    let tcp = TcpStream::connect((host.as_str(), port)).await.map_err(connect)?;
    // small messages, one at a time: each goes out as it is written
    tcp.set_nodelay(true).map_err(connect)?;
    let local_address = tcp.local_addr().map_err(connect)?.ip().to_string();
    let io: Box<dyn Io> = if tls {
        let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| invalid(format!("{url}: {e}")))?;
        let stream = tls_connector()?
            .connect(name, tcp)
            .await
            .map_err(|e| AcpError::Connect { reason: format!("TLS handshake with {host}: {e}") })?;
        Box::new(stream)
    } else {
        Box::new(tcp)
    };
    let (socket, _) = tokio_tungstenite::client_async(upgrade, io).await.map_err(handshake_error)?;

    let (sink, stream) = socket.split();
    let outgoing = sink
        .sink_map_err(std::io::Error::other)
        .with(|line: String| std::future::ready(Ok::<_, std::io::Error>(Message::text(line))));
    // a text frame is one message; the agent's close frame is the end, binary frames are ignored, and tungstenite
    // answers pings itself
    let incoming = stream
        .map_err(std::io::Error::other)
        .try_take_while(|message| std::future::ready(Ok(!message.is_close())))
        .try_filter_map(|message| {
            std::future::ready(Ok(match message {
                Message::Text(text) => Some(text.as_str().to_owned()),
                _ => None,
            }))
        });
    Ok((Lines::new(outgoing, incoming), local_address))
}

/// Connect to `url` and initialize the agent: the connection, the agent's answer and this end's address.
pub(crate) async fn connect(
    url: &str,
    headers: BTreeMap<String, String>,
    client_info: &Implementation,
    life: &Life,
) -> Result<(Link, InitializeResponse, String), AcpError> {
    let (transport, local_address) = open(url, headers).await?;
    let (connection_tx, connection_rx) = oneshot::channel();
    let (close_tx, close_rx) = oneshot::channel::<()>();
    life.spawn(Client.v1().connect_with(transport, async move |cx: ConnectionTo<Agent>| {
        let _ = connection_tx.send(cx.clone());
        // until the script closes the connection (or lets go of every handle to it), or the agent hangs up
        futures_util::future::select(close_rx, std::pin::pin!(cx.incoming_closed())).await;
        Ok(())
    }));
    let cx = connection_rx
        .await
        .map_err(|_| AcpError::Closed { reason: "the connection is closed".to_owned() })?;
    let link = Link { cx, close: Mutex::new(Some(close_tx)), sessions: Mutex::new(HashMap::new()) };

    let initialize: schema::InitializeRequest = to_sdk(&json!({
        "protocol_version": 1,
        "client_capabilities": { "fs": { "read_text_file": false, "write_text_file": false }, "terminal": false },
        "client_info": client_info,
    }))?;
    let answer = link.cx.send_request(initialize).block_task().await.map_err(|e| failure(&link.cx, &e))?;
    // another version is refused by the SDK's protocol guard (-32600)
    let agent: InitializeResponse = from_sdk(&answer, "the answer to initialize")?;
    Ok((link, agent, local_address))
}

// ---------------------------------------------------------------------------------------------------------------
// The connection and its sessions
// ---------------------------------------------------------------------------------------------------------------

/// A session this connection started or loaded.
struct Session {
    /// The SDK's session: what the agent sends for it, in order.
    active: tokio::sync::Mutex<ActiveSession<'static, Agent>>,
    /// A turn is going.
    busy: AtomicBool,
    /// Turns the script let go of before their answer came: what is read until that many answers have been is
    /// theirs, and dropped.
    abandoned: AtomicU32,
    /// The permission requests the turn has been given and not answered.
    permissions: Permissions,
}

/// Permission requests given to the script and not answered yet, by the id the script names them by.
type Permissions =
    Mutex<HashMap<String, agent_client_protocol::Responder<schema::RequestPermissionResponse>>>;

/// Answer every waiting permission request `cancelled`.
fn cancel_all(permissions: &Permissions) {
    let waiting: Vec<_> = permissions.lock().drain().map(|(_, responder)| responder).collect();
    for responder in waiting {
        let _ = responder.respond(cancelled());
    }
}

impl Session {
    fn new(active: ActiveSession<'static, Agent>) -> Self {
        Self {
            active: tokio::sync::Mutex::new(active),
            busy: AtomicBool::new(false),
            abandoned: AtomicU32::new(0),
            permissions: Mutex::new(HashMap::new()),
        }
    }

    fn cancel_permissions(&self) {
        cancel_all(&self.permissions);
    }
}

/// What a message the agent sent for a session is to a turn: an event, or nothing (it was answered, or ignored).
fn event_of(permissions: &Permissions, message: Dispatch) -> Option<TurnEvent> {
    let message = match message {
        Dispatch::Notification(notification) => {
            // read from the wire's JSON, not the SDK's type: an update of a kind the SDK's stable schema lacks is
            // still given, as `other`
            if notification.method() == "session/update" {
                let update = notification.params().get("update").cloned().unwrap_or(Value::Null);
                return Some(TurnEvent::Update { update: session_update(update) });
            }
            return None;
        }
        other => other,
    };
    match message.into_request::<schema::RequestPermissionRequest>() {
        Ok(Ok((request, responder))) => {
            // the JSON-RPC id, integers and strings written without quotes
            let id = responder.id().to_string();
            let tool_call: Result<ToolCallUpdate, _> = from_sdk(&request.tool_call, "a permission request");
            let options: Result<Vec<PermissionOption>, _> =
                from_sdk(&request.options, "a permission request");
            let (Ok(tool_call), Ok(options)) = (tool_call, options) else {
                let _ = responder.respond_with_error(agent_client_protocol::Error::invalid_params());
                return None;
            };
            permissions.lock().insert(id.clone(), responder);
            Some(TurnEvent::Permission { id, tool_call, options })
        }
        // fs/*, terminal/* and anything else: this client offers none of it
        Ok(Err(Dispatch::Request(_, responder))) => {
            let _ = responder.respond_with_error(agent_client_protocol::Error::method_not_found());
            None
        }
        // a malformed request is answered by the SDK when its responder is dropped
        Ok(Err(_)) | Err(_) => None,
    }
}

/// A connection, as the script's handles hold it. When the last one is dropped the connection closes.
pub(crate) struct Link {
    cx: ConnectionTo<Agent>,
    /// Ends the SDK's connection, which closes the socket.
    close: Mutex<Option<oneshot::Sender<()>>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl Link {
    fn closed(&self) -> Option<AcpError> {
        self.close
            .lock()
            .is_none()
            .then(|| AcpError::Closed { reason: "the connection was closed".to_owned() })
    }

    pub(crate) async fn new_session(
        &self,
        params: &NewSessionRequest,
    ) -> Result<NewSessionResponse, AcpError> {
        if let Some(closed) = self.closed() {
            return Err(closed);
        }
        let request: schema::NewSessionRequest = to_sdk(params)?;
        let setup = self
            .cx
            .build_session_from(request)
            .block_task()
            .start_session()
            .await
            .map_err(|e| failure(&self.cx, &e))?;
        let response: NewSessionResponse = from_sdk(&setup.response, "the answer to session/new")?;
        self.sessions
            .lock()
            .insert(setup.session.session_id().to_string(), Arc::new(Session::new(setup.session)));
        Ok(response)
    }

    pub(crate) async fn load_session(
        &self,
        params: &LoadSessionRequest,
    ) -> Result<LoadSessionResponse, AcpError> {
        if let Some(closed) = self.closed() {
            return Err(closed);
        }
        let request: schema::LoadSessionRequest = to_sdk(params)?;
        let setup = self
            .cx
            .load_session_from(request)
            .block_task()
            .start_session()
            .await
            .map_err(|e| failure(&self.cx, &e))?;
        let response: LoadSessionResponse = from_sdk(&setup.response, "the answer to session/load")?;
        let mut active = setup.session;
        // the history the agent replayed came before its answer, so it is all queued now: drop it (a request in it
        // is answered as a turn would answer it)
        let permissions = Permissions::default();
        while let Some(Ok(message)) = active.read_update().now_or_never() {
            if let SessionMessage::Message(message) = message {
                let _ = event_of(&permissions, message.into_raw());
                cancel_all(&permissions);
            }
        }
        self.sessions.lock().insert(params.session_id.clone(), Arc::new(Session::new(active)));
        Ok(response)
    }

    pub(crate) fn prompt(&self, params: &PromptRequest) -> Result<TurnState, AcpError> {
        if let Some(closed) = self.closed() {
            return Err(closed);
        }
        let session = self.sessions.lock().get(&params.session_id).cloned().ok_or_else(|| {
            invalid(format!("session {} was not started or loaded on this connection", params.session_id))
        })?;
        let prompt: Vec<schema::ContentBlock> = to_sdk(&params.prompt)?;
        if session.busy.swap(true, Ordering::SeqCst) {
            return Err(invalid(format!("session {} already has a turn going", params.session_id)));
        }
        let sent = match session.active.try_lock() {
            Ok(mut active) => active.send_prompt_blocks(prompt).map_err(|e| failure(&self.cx, &e)),
            Err(_) => Err(invalid(format!("session {} is being read", params.session_id))),
        };
        if let Err(e) = sent {
            session.busy.store(false, Ordering::SeqCst);
            return Err(e);
        }
        Ok(TurnState { session, finished: AtomicBool::new(false) })
    }

    pub(crate) fn cancel(&self, session_id: &str) -> Result<(), AcpError> {
        if let Some(closed) = self.closed() {
            return Err(closed);
        }
        if let Some(session) = self.sessions.lock().get(session_id) {
            session.cancel_permissions();
        }
        let cancel: schema::CancelNotification = to_sdk(&json!({ "session_id": session_id }))?;
        self.cx.send_notification(cancel).map_err(|e| failure(&self.cx, &e))
    }

    pub(crate) fn close(&self) {
        if let Some(close) = self.close.lock().take() {
            let _ = close.send(());
        }
    }
}

/// A turn, as the script's handle holds it.
pub(crate) struct TurnState {
    session: Arc<Session>,
    /// The turn gave its `done` event or its error: nothing follows.
    finished: AtomicBool,
}

impl TurnState {
    pub(crate) async fn next(&self) -> Result<Option<TurnEvent>, AcpError> {
        let mut active = self.session.active.lock().await;
        if self.finished.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let end = |outcome: Result<Option<TurnEvent>, AcpError>| {
            self.finished.store(true, Ordering::SeqCst);
            self.session.cancel_permissions();
            self.session.busy.store(false, Ordering::SeqCst);
            outcome
        };
        let connection = active.connection().clone();
        let lost = || AcpError::Closed { reason: "the connection is closed".to_owned() };
        loop {
            // the session's stream stays open after the connection ends (the session holds a sender of its own):
            // what was queued is read first, then the end of the connection ends the turn
            let message = tokio::select! {
                biased;
                message = active.read_update() => message,
                () = async { connection.terminated().await; } => Err(agent_client_protocol::Error::internal_error()),
            };
            let Ok(message) = message else { return end(Err(lost())) };
            let abandoned = self.session.abandoned.load(Ordering::SeqCst) > 0;
            match message {
                SessionMessage::PromptResponse(_) if abandoned => {
                    self.session.abandoned.fetch_sub(1, Ordering::SeqCst);
                }
                SessionMessage::PromptResponse(Ok(response)) => {
                    let stop_reason: Result<StopReason, _> =
                        from_sdk(&response.stop_reason, "the answer to session/prompt");
                    return end(stop_reason.map(|stop_reason| Some(TurnEvent::Done { stop_reason })));
                }
                SessionMessage::PromptResponse(Err(e)) => return end(Err(failure(&connection, &e))),
                SessionMessage::Message(message) => {
                    let event = event_of(&self.session.permissions, message.into_raw());
                    if abandoned {
                        self.session.cancel_permissions();
                    } else if let Some(event) = event {
                        return Ok(Some(event));
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn respond(&self, id: &str, outcome: &PermissionOutcome) -> Result<(), AcpError> {
        let responder = self.session.permissions.lock().remove(id);
        let responder =
            responder.ok_or_else(|| invalid(format!("no permission request `{id}` is waiting")))?;
        let answer: schema::RequestPermissionResponse = to_sdk(&json!({ "outcome": outcome }))?;
        responder
            .respond(answer)
            .map_err(|_| AcpError::Closed { reason: "the connection is closed".to_owned() })
    }
}

impl Drop for TurnState {
    /// A turn the script let go of before it ended: what is left of it is dropped by the session's next turn, the
    /// permission requests it was given are answered `cancelled`, and the session can have another turn.
    fn drop(&mut self) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }
        self.session.abandoned.fetch_add(1, Ordering::SeqCst);
        self.session.cancel_permissions();
        self.session.busy.store(false, Ordering::SeqCst);
    }
}
