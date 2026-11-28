//! The connection under `vnd:agent_client_protocol`: the SDK's v1 client running over a transport of lines.
//!
//! The transport is a `lib:websocket` socket's lines, one text frame per JSON-RPC message. The SDK's connection runs
//! on a task of the engine's [`Life`] until the script closes it, every handle to it is gone, or the agent hangs up.
//! Each session the script starts or loads is an SDK `ActiveSession`, which receives what the agent sends for that
//! session (its updates, its permission requests, the answer to a prompt) on a stream of its own; `read_update` reads
//! it.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use agent_client_protocol::v1::{ActiveSession, ConnectionTo, SessionMessage, schema};
use agent_client_protocol::{Agent, Client, Dispatch, Lines};
use futures_util::FutureExt;
use mlua_ir::ir::{Field, Ty, TypeBody};
use mlua_ir::{Registry, Shape};
use parking_lot::Mutex;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::Data;
use crate::agent_client_protocol::{
    AcpError, ContentBlock, Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOutcome,
    SessionMessage as Message, SessionModeState, SessionUpdate, StopReason, ToolCallUpdate,
};
use neoc_vm_core::lifecycle::Life;

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
// The connection and its sessions
// ---------------------------------------------------------------------------------------------------------------

/// A session this connection started or loaded.
pub(crate) struct Session {
    pub(crate) id: String,
    pub(crate) modes: Option<SessionModeState>,
    connection: ConnectionTo<Agent>,
    /// The SDK's session: what the agent sends for it, in order.
    active: tokio::sync::Mutex<ActiveSession<'static, Agent>>,
    /// The permission requests given to the script and not answered.
    permissions: Arc<Permissions>,
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

impl Drop for Session {
    fn drop(&mut self) {
        cancel_all(&self.permissions);
    }
}

/// What a message the agent sent for a session is to the script: a message, or nothing (it was answered, or
/// ignored).
fn message_of(permissions: &Permissions, message: Dispatch) -> Option<Message> {
    let message = match message {
        Dispatch::Notification(notification) => {
            // read from the wire's JSON, not the SDK's type: an update of a kind the SDK's stable schema lacks is
            // still given, as `other`
            if notification.method() == "session/update" {
                let update = notification.params().get("update").cloned().unwrap_or(Value::Null);
                return Some(Message::Update { update: session_update(update) });
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
            Some(Message::RequestPermission { id, tool_call, options })
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

impl Session {
    pub(crate) fn send_prompt(&self, prompt: Vec<ContentBlock>) -> Result<(), AcpError> {
        if self.connection.termination().is_some() {
            return Err(AcpError::Closed { reason: "the connection is closed".to_owned() });
        }
        let prompt: Vec<schema::ContentBlock> = to_sdk(&prompt)?;
        match self.active.try_lock() {
            Ok(mut active) => active.send_prompt_blocks(prompt).map_err(|e| failure(&self.connection, &e)),
            Err(_) => Err(invalid(format!("session {} is being read; send the prompt first", self.id))),
        }
    }

    pub(crate) async fn read_update(&self) -> Result<Message, AcpError> {
        let mut active = self.active.lock().await;
        let connection = self.connection.clone();
        let lost = || AcpError::Closed { reason: "the connection is closed".to_owned() };
        loop {
            // the session's stream stays open after the connection ends (the session holds a sender of its own):
            // what was queued is read first, then the end of the connection ends the read
            let message = tokio::select! {
                biased;
                message = active.read_update() => message,
                () = async { connection.terminated().await; } => Err(agent_client_protocol::Error::internal_error()),
            };
            let Ok(message) = message else { return Err(lost()) };
            match message {
                SessionMessage::PromptResponse(Ok(response)) => {
                    // the turn is over: what it asked and was not answered is answered `cancelled`
                    cancel_all(&self.permissions);
                    let stop_reason: StopReason =
                        from_sdk(&response.stop_reason, "the answer to session/prompt")?;
                    return Ok(Message::PromptResponse { stop_reason });
                }
                SessionMessage::PromptResponse(Err(e)) => {
                    cancel_all(&self.permissions);
                    return Err(failure(&connection, &e));
                }
                SessionMessage::Message(message) => {
                    if let Some(message) = message_of(&self.permissions, message.into_raw()) {
                        return Ok(message);
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn respond(&self, id: &str, outcome: &PermissionOutcome) -> Result<(), AcpError> {
        let responder = self.permissions.lock().remove(id);
        let responder =
            responder.ok_or_else(|| invalid(format!("no permission request `{id}` is waiting")))?;
        let answer: schema::RequestPermissionResponse = to_sdk(&json!({ "outcome": outcome }))?;
        responder
            .respond(answer)
            .map_err(|_| AcpError::Closed { reason: "the connection is closed".to_owned() })
    }
}

/// A connection, as the script's handles hold it. When the last one is dropped the connection closes.
pub(crate) struct Link {
    cx: ConnectionTo<Agent>,
    name: String,
    /// Ends the SDK's connection, which drops the transport.
    close: Mutex<Option<oneshot::Sender<()>>>,
    /// The permissions of the sessions started on it, by session: what a cancel answers.
    sessions: Mutex<HashMap<String, Weak<Permissions>>>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}

/// A connection that is starting: the SDK hands over its `ConnectionTo` once the transport is up.
async fn connection(rx: oneshot::Receiver<ConnectionTo<Agent>>) -> Result<ConnectionTo<Agent>, AcpError> {
    rx.await.map_err(|_| AcpError::Closed { reason: "the connection is closed".to_owned() })
}

impl Link {
    /// Start the SDK's v1 client over `lines` on a task of `life`.
    pub(crate) fn start(life: &Life, lines: neoc_vm_lib_websocket::Lines, name: String) -> LinkStart {
        let neoc_vm_lib_websocket::Lines { outgoing, incoming } = lines;
        let transport = Lines::new(outgoing, incoming);
        let (connection_tx, connection_rx) = oneshot::channel();
        let (close_tx, close_rx) = oneshot::channel::<()>();
        life.spawn(Client.v1().name(name.clone()).connect_with(
            transport,
            async move |cx: ConnectionTo<Agent>| {
                let _ = connection_tx.send(cx.clone());
                // until the script closes the connection (or lets go of every handle to it), or the agent hangs up
                futures_util::future::select(close_rx, std::pin::pin!(cx.incoming_closed())).await;
                Ok(())
            },
        ));
        LinkStart { connection: connection_rx, close: close_tx, name }
    }

    fn closed(&self) -> Option<AcpError> {
        self.close
            .lock()
            .is_none()
            .then(|| AcpError::Closed { reason: "the connection was closed".to_owned() })
    }

    pub(crate) async fn initialize(
        &self,
        request: InitializeRequest,
    ) -> Result<InitializeResponse, AcpError> {
        if let Some(closed) = self.closed() {
            return Err(closed);
        }
        let client_info = request.client_info.unwrap_or_else(|| Implementation {
            name: self.name.clone(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        });
        let initialize: schema::InitializeRequest = to_sdk(&json!({
            "protocol_version": request.protocol_version,
            "client_capabilities": { "fs": { "read_text_file": false, "write_text_file": false }, "terminal": false },
            "client_info": client_info,
        }))?;
        let answer =
            self.cx.send_request(initialize).block_task().await.map_err(|e| failure(&self.cx, &e))?;
        // another version is refused by the SDK's protocol guard (-32600)
        from_sdk(&answer, "the answer to initialize")
    }

    fn session(&self, active: ActiveSession<'static, Agent>, modes: Option<SessionModeState>) -> Session {
        let id = active.session_id().to_string();
        let permissions = Arc::new(Permissions::default());
        let mut sessions = self.sessions.lock();
        sessions.retain(|_, permissions| permissions.strong_count() > 0);
        sessions.insert(id.clone(), Arc::downgrade(&permissions));
        Session {
            id,
            modes,
            connection: self.cx.clone(),
            active: tokio::sync::Mutex::new(active),
            permissions,
        }
    }

    pub(crate) async fn new_session(&self, params: &NewSessionRequest) -> Result<Session, AcpError> {
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
        Ok(self.session(setup.session, response.modes))
    }

    pub(crate) async fn load_session(&self, params: &LoadSessionRequest) -> Result<Session, AcpError> {
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
        // is answered `cancelled`)
        let permissions = Permissions::default();
        while let Some(Ok(message)) = active.read_update().now_or_never() {
            if let SessionMessage::Message(message) = message {
                let _ = message_of(&permissions, message.into_raw());
                cancel_all(&permissions);
            }
        }
        Ok(self.session(active, response.modes))
    }

    pub(crate) fn cancel(&self, session_id: &str) -> Result<(), AcpError> {
        if let Some(closed) = self.closed() {
            return Err(closed);
        }
        if let Some(permissions) = self.sessions.lock().get(session_id).and_then(Weak::upgrade) {
            cancel_all(&permissions);
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

/// A connection the SDK has not handed over yet.
pub(crate) struct LinkStart {
    connection: oneshot::Receiver<ConnectionTo<Agent>>,
    close: oneshot::Sender<()>,
    name: String,
}

impl LinkStart {
    /// The connection, once the SDK runs it.
    pub(crate) async fn ready(self) -> Result<Link, AcpError> {
        let cx = connection(self.connection).await?;
        Ok(Link {
            cx,
            name: self.name,
            close: Mutex::new(Some(self.close)),
            sessions: Mutex::new(HashMap::new()),
        })
    }
}
