//! The backend's side of the one RPC bus (`vos:rpc`, the global `RPC`), with the same four functions an app's page
//! has:
//!
//! ```text
//! RPC.send_request(app, method, params?)    a request to an app (this one or another): every handler registered
//!                                           for the method, a backend's or an open page's, may answer; the first does
//! RPC.on_request(method, fn)                serve one of this app's methods on the bus: fn(params, ctx)
//! RPC.send_notification(name, params?)      one of this app's notifications, to whoever listens; nothing waits
//! RPC.on_notification(app, name, fn)        hear an app's notification (this one's too): fn(params, ctx)
//! ```
//!
//! Only an app serves its own methods and sends its own notifications. A `_name` is private: another app's private
//! method can still be called (the host logs it), but a private notification never leaves its app.

use super::{context::Context, json::Json, misc::EventData, shared};
use crate::vm::{Effect, Handler};
use mlua_ir::{Callback, LuaError, Options, Registry, Shape, Unknown};
use neoc_vm::mlua;
use neoc_vm::mlua::Lua;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use vos_application::ports::HandlerInfo;
use vos_application::PortError;
use vos_domain::AppId;

mlua_ir::unknown_reason!(
    /// Why a request's params have no declared shape here.
    RequestParams = "the params of the app's method: their shape is its contract's"
);
mlua_ir::unknown_reason!(
    /// Why a request's result has no declared shape here.
    RequestResult = "what the handler answers: its shape is the contract's"
);
mlua_ir::unknown_reason!(
    /// Why a notification's or a handled request's params have no declared shape here.
    MessageParams = "the params of the method or notification: their shape is the contract's"
);
mlua_ir::unknown_reason!(
    /// Why a handler's answer has no declared shape here.
    HandlerResult = "what the handler answers: its shape is the contract's"
);

mlua_ir::unknown_reason!(
    /// Why a JSON Schema has no declared shape here.
    SchemaWords = "a JSON Schema: its keywords are the schema's own"
);

/// The JSON Schema of a handler's params.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InputSchema(Value);

impl Shape for InputSchema {
    fn shape(reg: &mut Registry) -> mlua_ir::ir::Ty {
        BTreeMap::<String, Unknown<SchemaWords>>::shape(reg)
    }
}

/// What a handler says about itself when it registers: an agent is shown it when the method is public (a method
/// without a leading `_` is an agent tool, `<app>__<method in snake case>`, answered on the bus).
#[derive(Clone, Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vos:rpc")]
pub struct About {
    /// The name a person is shown.
    pub title: Option<String>,
    /// What it does: an agent decides from it when to call it.
    pub description: Option<String>,
    /// The JSON Schema of its params.
    pub input_schema: Option<InputSchema>,
    /// It changes nothing: an agent's call runs with every write refused.
    pub read_only: bool,
}

/// The named params a request is sent with.
pub type Params = Json<BTreeMap<String, Unknown<RequestParams>>>;

/// A handler of one of this app's methods: `fn(params, ctx)`, answering with the result.
pub type RequestHandler =
    Callback<fn(params: Json<Unknown<MessageParams>>, ctx: Context) -> Json<Unknown<HandlerResult>>>;

/// A listener of a notification: `fn(params, ctx)`, where `ctx.actor` is the app that sent it.
pub type Listener = Callback<fn(params: Json<Unknown<MessageParams>>, ctx: Context)>;

/// Why a request gave no result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[shape(module = "vos:rpc")]
pub enum RequestError {
    /// The app is not running, or nothing on the bus handles the method.
    #[error("{text}")]
    NotFound {
        #[serde(skip)]
        text: String,
    },
    /// The handler refused the params, raised, or said it could not (`ctx:fail`).
    #[error("{text}")]
    Failed {
        #[serde(skip)]
        text: String,
    },
    /// The host could not run it.
    #[error("{text}")]
    Internal {
        #[serde(skip)]
        text: String,
    },
}

impl From<PortError> for RequestError {
    fn from(error: PortError) -> Self {
        match error {
            PortError::NotFound(_) => Self::NotFound { text: error.to_string() },
            PortError::Rpc { code: -32601, message, .. } => Self::NotFound { text: message },
            PortError::Rpc { message, .. } => Self::Failed { text: message },
            PortError::Rejected(_) => Self::Failed { text: error.to_string() },
            PortError::Internal(_) => Self::Internal { text: error.to_string() },
        }
    }
}

/// The longest method name (the backend's own limit).
const MAX_METHOD: usize = 128;
/// The longest notification name (as `app.emit` takes).
const MAX_NAME: usize = 64;

fn app_id(what: &str, id: &str) -> mlua::Result<AppId> {
    AppId::parse(id).map_err(|_| mlua::Error::runtime(format!("{what}: `{id}` is not an app id")))
}

fn method_name(what: &str, method: &str) -> mlua::Result<()> {
    if method.is_empty() || method.len() > MAX_METHOD || method.contains('/') {
        return Err(mlua::Error::runtime(format!(
            "{what}: a method name is 1 to {MAX_METHOD} characters, without `/`"
        )));
    }
    Ok(())
}

pub(super) async fn send_request(
    lua: Lua,
    app: String,
    method: String,
    params: Option<Params>,
) -> mlua::Result<Result<Json<Unknown<RequestResult>>, RequestError>> {
    let sh = shared(&lua)?;
    // a handler may write, so a `readOnly` tool may not ask for one
    sh.writable("RPC.send_request")?;
    let to = app_id("RPC.send_request", &app)?;
    method_name("RPC.send_request", &method)?;
    let params = match params.map(|params| params.value) {
        Some(params @ Value::Object(_)) => params,
        _ => json!({}),
    };
    let peers = sh.peers.clone();
    let from = sh.app.clone();
    let out = super::host(&sh, async { Ok(peers.request(&from, &to, &method, params).await) }).await?;
    Ok(out.map(Json::new).map_err(RequestError::from))
}

pub(super) fn on_request(
    lua: &Lua,
    method: String,
    handler: RequestHandler,
    about: Option<About>,
) -> mlua::Result<()> {
    let sh = shared(lua)?;
    // serving a method changes the app's wiring, as `app.tool` does
    sh.writable("RPC.on_request")?;
    method_name("RPC.on_request", &method)?;
    let mut handlers = sh.rpc_handlers.lock();
    if handlers.contains_key(&method) {
        return Err(mlua::Error::runtime(format!(
            "RPC.on_request: `{method}` already has a handler in this backend"
        )));
    }
    let about = about.unwrap_or_default();
    let info = HandlerInfo {
        method: method.clone(),
        title: about.title,
        description: about.description,
        input_schema: about.input_schema.map(|s| s.0),
        read_only: about.read_only,
    };
    handlers.insert(method, Handler::Registered(handler.into_function(), info));
    drop(handlers);
    // the app's agent tools follow its handlers
    sh.push_effect(Effect::HandlersChanged)
}

pub(super) fn send_notification(lua: &Lua, name: String, params: Option<EventData>) -> mlua::Result<()> {
    // the same as `app.emit`: the runtime takes it to the bus when the call ends or pauses
    super::misc::emit(&*shared(lua)?, name, params)
}

pub(super) fn on_notification(lua: &Lua, app: String, name: String, listener: Listener) -> mlua::Result<()> {
    let sh = shared(lua)?;
    // listening changes the app's wiring, as `app.tool` does
    sh.writable("RPC.on_notification")?;
    let from = app_id("RPC.on_notification", &app)?;
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
    if !valid {
        return Err(mlua::Error::runtime(format!(
            "RPC.on_notification: a notification name is 1 to {MAX_NAME} letters, digits, . _ : or -"
        )));
    }
    if name.starts_with("host:") || (name.starts_with('_') && from != sh.app) {
        return Err(mlua::Error::runtime(format!(
            "RPC.on_notification: `{name}` never leaves {from}: a `_name` is its own, a `host:` name the host's"
        )));
    }
    sh.listeners.lock().entry((from.to_string(), name)).or_default().push(listener.into_function());
    Ok(())
}
