//! The view protocol (spec §2.2 to §2.7): JSON-RPC 2.0 over the `/rpc` WebSocket, by-name params objects.
//!
//! Three message classes keep the common path cheap: `view.tree` (a full tree, on open and resync), `view.ops`
//! (keyed structural ops, versioned by `base`/`rev`) and `data.patch` (per-binding value updates). Values change
//! far more often than structure, so typing, agent writes and list refreshes never rebuild the tree.
//!
//! * Client to host **requests**: [`ClientRequest`]. **Notifications**: [`ClientNotification`].
//! * Host to client **notifications**: [`HostNotification`].
//! * [`METHODS`] is the full method table, the source of the generated reference.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::node::Handler;
use super::node::Node;
use super::source::{ClientDecl, SourceDecl};
use crate::protocol::{RpcError, RpcMessage};

/// Applies the standard derives, and the codegen ones behind the feature.
macro_rules! wire {
    ($($item:item)*) => {
        $(
            #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
            #[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
            $item
        )*
    };
}

// ---------------------------------------------------------------------------------------------------------
// Ops (spec §2.2)
// ---------------------------------------------------------------------------------------------------------

wire! {
    /// A keyed structural op. Paths are key paths from the window root (`main/list/@row/item`).
    #[serde(tag = "op", rename_all = "camelCase")]
    pub enum Op {
        /// Change props. A `null` value removes the prop (back to its default).
        Set {
            path: String,
            #[cfg_attr(feature = "codegen", ts(as = "BTreeMap<String, Value>"))]
            props: BTreeMap<String, Value>,
        },
        /// Change which events the node reports. A `null` removes the handler.
        On {
            path: String,
            on: BTreeMap<String, Option<Handler>>,
        },
        /// Insert `node` into `parent`'s default slot (or `slot`), before the sibling `before` (`null` appends).
        Insert {
            parent: String,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[cfg_attr(feature = "codegen", ts(optional))]
            slot: Option<String>,
            before: Option<String>,
            node: Node,
        },
        /// Remove a node. Idempotent.
        Remove { path: String },
        /// Reorder among siblings: place the node before `before` (`null` = last).
        Move { path: String, before: Option<String> },
        /// Same key, new type.
        Replace { path: String, node: Node },
    }
}

// ---------------------------------------------------------------------------------------------------------
// Data patches and sources on the wire (spec §2.3, §3.3)
// ---------------------------------------------------------------------------------------------------------

wire! {
    /// A server-state value with its revision.
    #[serde(deny_unknown_fields)]
    pub struct StateValue {
        /// The value.
        pub v: Value,
        /// The revision.
        pub r: u32,
    }

    /// A window of a source's data: values, never structure.
    #[serde(tag = "kind", rename_all = "camelCase")]
    pub enum SourcePatch {
        /// `keys` is the whole window in order; `rows` only the rows whose content changed; `drop` the rows that
        /// left the window. Inserts, removals and moves are all "a different key list": a patch is idempotent.
        Rows {
            total: u32,
            from: u32,
            keys: Vec<Value>,
            rows: BTreeMap<String, Value>,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            #[cfg_attr(feature = "codegen", ts(as = "Option<Vec<Value>>", optional))]
            drop: Vec<Value>,
        },
        Row {
            rev: u32,
            row: Option<Value>,
        },
        Value { value: Value },
        /// A text document edit.
        #[serde(rename_all = "camelCase")]
        Text {
            rev: u32,
            length: u32,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[cfg_attr(feature = "codegen", ts(optional))]
            value: Option<String>,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            #[cfg_attr(feature = "codegen", ts(as = "Option<Vec<TextSplice>>", optional))]
            splices: Vec<TextSplice>,
        },
        /// Chat rows plus streaming appends.
        Thread {
            rows: BTreeMap<String, Value>,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            #[cfg_attr(feature = "codegen", ts(as = "Option<Vec<ThreadAppend>>", optional))]
            append: Vec<ThreadAppend>,
        },
    }

    /// Replace `del` characters at `at` with `ins`.
    pub struct TextSplice {
        pub at: u32,
        pub del: u32,
        pub ins: String,
        pub rev: u32,
    }

    /// Append streamed text to a part of a message.
    pub struct ThreadAppend {
        pub id: String,
        /// `parts.0.text`
        pub path: String,
        pub text: String,
    }
}

// ---------------------------------------------------------------------------------------------------------
// session.*
// ---------------------------------------------------------------------------------------------------------

wire! {
    /// `session.hello` params.
    #[serde(rename_all = "camelCase")]
    pub struct SessionHello {
        pub proto: u32,
        /// The client's kit version, `"1.0"`.
        pub kit: String,
        pub locale: String,
        pub time_zone: String,
        #[serde(default)]
        pub features: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub resume: Option<SessionResume>,
    }

    #[serde(rename_all = "camelCase")]
    pub struct SessionResume {
        pub session_id: String,
        pub views: Vec<ResumeView>,
    }

    #[serde(rename_all = "camelCase")]
    pub struct ResumeView {
        pub view_id: String,
        pub rev: u32,
    }

    /// `session.hello` result.
    #[serde(rename_all = "camelCase")]
    pub struct SessionWelcome {
        pub proto: u32,
        pub kit: String,
        pub host_version: String,
        pub session_id: String,
        pub resumed: bool,
    }
}

// ---------------------------------------------------------------------------------------------------------
// view.* requests
// ---------------------------------------------------------------------------------------------------------

wire! {
    pub struct Size {
        pub w: u32,
        pub h: u32,
    }

    /// `view.open` params.
    #[serde(rename_all = "camelCase")]
    pub struct ViewOpen {
        pub app: String,
        /// The window kind (`"main"`).
        pub window: String,
        /// The shell's id for the window, kept so a reconnect maps back.
        pub window_id: String,
        pub size: Size,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub params: Option<Map<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub resume: Option<ViewResume>,
    }

    #[serde(rename_all = "camelCase")]
    pub struct ViewResume {
        pub view_id: String,
        pub rev: u32,
        /// The client's persisted client-managed values.
        pub client: Map<String, Value>,
        /// Writes the client made but never saw acked: replayed with their `baseRev`.
        #[serde(default)]
        pub drafts: Vec<Draft>,
    }

    /// An unacked two-way write.
    #[serde(tag = "kind", rename_all = "camelCase")]
    pub enum Draft {
        #[serde(rename_all = "camelCase")]
        State { path: String, value: Value, base_rev: u32 },
        #[serde(rename_all = "camelCase")]
        Row { source: String, key: Value, field: String, value: Value, base_rev: u32 },
    }

    /// `view.open` / `view.sync` result: one round trip to a complete window.
    #[serde(rename_all = "camelCase")]
    pub struct ViewSnapshot {
        pub view_id: String,
        pub proto: u32,
        pub kit: String,
        pub rev: u32,
        /// Declared client-managed state.
        pub client_decls: BTreeMap<String, ClientDecl>,
        /// Current client-managed values (restored from the host's `_ui_state`).
        pub client: Map<String, Value>,
        /// App and window state referenced by the tree, with revisions.
        pub state: BTreeMap<String, StateValue>,
        pub sources: BTreeMap<String, SourceDecl>,
        pub tree: Node,
        /// The first data window of each source.
        pub data: BTreeMap<String, SourcePatch>,
    }

    /// `view.sync` and `view.close` params.
    #[serde(rename_all = "camelCase")]
    pub struct ViewRef {
        pub view_id: String,
    }

    /// `view.event` params: a user event.
    #[serde(rename_all = "camelCase")]
    pub struct ViewEvent {
        pub view_id: String,
        /// The tree revision the user saw.
        pub rev: u32,
        /// Per-view monotonic: dedupes on reconnect.
        pub seq: u32,
        pub path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub row_key: Option<Value>,
        pub event: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub payload: Option<Value>,
    }

    /// `view.event` result. `ok: false, reason: "stale"` when the node no longer exists.
    pub struct EventAck {
        pub ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub reason: Option<String>,
    }

    /// `state.set` params: a two-way write to a server state path.
    #[serde(rename_all = "camelCase")]
    pub struct StateSet {
        pub view_id: String,
        pub path: String,
        pub value: Value,
        pub base_rev: u32,
    }

    /// `row.set` params: a two-way write to a field of a bound row.
    #[serde(rename_all = "camelCase")]
    pub struct RowSet {
        pub view_id: String,
        pub source: String,
        pub key: Value,
        pub field: String,
        pub value: Value,
        pub base_rev: u32,
    }

    /// What the host did with a two-way write: the client needs it to tell a merge from a refusal.
    #[serde(rename_all = "camelCase")]
    pub enum WriteResolution {
        /// Based on the current revision: written as sent.
        Accepted,
        /// Based on an older revision; the edits composed and nothing was lost.
        Merged,
        /// The human's value replaced a newer one; the replaced value is kept (see `undo`).
        Replaced,
        /// A strict source refused a stale write: the client adopts `value` and shows the error state.
        Rejected,
    }

    /// What a client needs to offer Undo for a write that replaced something: the lost value and the kept
    /// revision it can ask the host to restore (`row.restore`).
    #[serde(rename_all = "camelCase")]
    pub struct Undo {
        pub revision_id: u32,
        /// The value that was replaced (the agent's text, the previous scalar).
        pub lost: Value,
    }

    /// The ack of a write: the authoritative value (it may differ from what was sent after a merge).
    pub struct WriteAck {
        pub rev: u32,
        pub value: Value,
        pub resolution: WriteResolution,
        /// `true` when the write overlapped a newer change (the human won, or the write was rejected).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub conflict: Option<bool>,
        /// Present when `resolution` is `replaced`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub undo: Option<Undo>,
    }

    /// `row.restore` params: Undo, put a kept revision back (the human wins; what it replaces is kept in turn).
    #[serde(rename_all = "camelCase")]
    pub struct RowRestore {
        pub view_id: String,
        pub source: String,
        pub key: Value,
        pub revision_id: u32,
    }

    pub struct Range {
        pub from: u32,
        pub to: u32,
    }

    /// `source.set` params: change params and/or the visible window.
    #[serde(rename_all = "camelCase")]
    pub struct SourceSet {
        pub view_id: String,
        pub source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub params: Option<Map<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub range: Option<Range>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub anchor: Option<String>,
    }
}

// ---------------------------------------------------------------------------------------------------------
// Client notifications
// ---------------------------------------------------------------------------------------------------------

wire! {
    /// `client.state`: the mirror of synced and persisted client entries (FIFO ahead of any later event).
    #[serde(rename_all = "camelCase")]
    pub struct ClientState {
        pub view_id: String,
        pub set: Map<String, Value>,
    }

    /// `view.resize`: debounced, sent only if the app reads `w.env.*` in render.
    #[serde(rename_all = "camelCase")]
    pub struct ViewResize {
        pub view_id: String,
        pub size: Size,
        pub size_class: String,
    }

    /// `view.visibility`: a hidden view pauses pushes; on show the host sends one coalesced catch-up.
    #[serde(rename_all = "camelCase")]
    pub struct ViewVisibility {
        pub view_id: String,
        pub visible: bool,
    }
}

// ---------------------------------------------------------------------------------------------------------
// Host notifications
// ---------------------------------------------------------------------------------------------------------

wire! {
    /// `view.tree`: a full replace.
    #[serde(rename_all = "camelCase")]
    pub struct ViewTree {
        pub view_id: String,
        pub rev: u32,
        pub tree: Node,
    }

    /// `view.ops`: `base` must equal the client's current `rev`; a gap means `view.sync`.
    #[serde(rename_all = "camelCase")]
    pub struct ViewOps {
        pub view_id: String,
        pub base: u32,
        pub rev: u32,
        pub ops: Vec<Op>,
    }

    /// `data.patch`: per-binding values and row windows.
    #[serde(rename_all = "camelCase")]
    pub struct DataPatch {
        pub view_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub state: Option<BTreeMap<String, StateValue>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub sources: Option<BTreeMap<String, SourcePatch>>,
        /// Sources a re-render started to use (they were not in the snapshot): declared here, with their first
        /// window in `sources`, before the ops that reference them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub declare: Option<BTreeMap<String, SourceDecl>>,
    }

    /// `client.set`: the app drives client-managed state (open this note, push this route).
    #[serde(rename_all = "camelCase")]
    pub struct ClientSet {
        pub view_id: String,
        pub path: String,
        pub value: Value,
        /// Overrides the user's value even on a focused text control.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub force: Option<bool>,
    }

    /// An imperative one-shot.
    #[serde(tag = "cmd", content = "args", rename_all = "camelCase")]
    pub enum Command {
        Toast(Toast),
        Focus { key: String },
        ScrollTo {
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[cfg_attr(feature = "codegen", ts(optional))]
            key: Option<String>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[cfg_attr(feature = "codegen", ts(optional))]
            row: Option<Value>,
        },
        Announce { text: String },
        Copy { text: String },
        OpenUrl { url: String },
        Haptic,
        CloseWindow,
    }

    pub struct Toast {
        pub text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub icon: Option<String>,
        /// `info`, `success` or `error`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub kind: Option<String>,
        /// An action button. Tapping it sends `view.event` with `path` `$toast/<id>` and `event` `tap`: the host
        /// keeps the app's `onTap` closure for that id and runs it once.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub action: Option<ToastAction>,
        /// Milliseconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub duration: Option<u32>,
    }

    /// The action button of a toast.
    pub struct ToastAction {
        /// The button's label.
        pub title: String,
        /// Names the host's closure: the event path is `$toast/<id>`.
        pub id: String,
    }

    /// `view.command` params: `{viewId, cmd, args}`.
    #[serde(rename_all = "camelCase")]
    pub struct ViewCommand {
        pub view_id: String,
        #[serde(flatten)]
        pub command: Command,
    }

    /// `view.error`: a render or validation failure; the last good tree stays.
    #[serde(rename_all = "camelCase")]
    pub struct ViewError {
        pub view_id: String,
        pub message: String,
        /// Dev builds: the Luau trace.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub dev: Option<String>,
    }

    /// `view.closed`: the instance is gone (app crash, uninstall).
    #[serde(rename_all = "camelCase")]
    pub struct ViewClosed {
        pub view_id: String,
        pub reason: String,
    }

    /// `window.open`: an app asked for a window (`app:openWindow`, e.g. from an agent's tool call). Not tied to
    /// a view: the shell opens the window, or brings the app's open one forward, then does `view.open`.
    pub struct WindowOpen {
        pub app: String,
        /// The window kind (`"main"`).
        pub window: String,
        /// The deep link `onOpen(w, params)` receives.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub params: Option<Map<String, Value>>,
    }

    /// `theme.set`: session-wide, not per view.
    #[serde(rename_all = "camelCase")]
    pub struct ThemeSet {
        pub scheme: String,
        pub tint: String,
        pub text_scale: f64,
        pub reduce_motion: bool,
        pub reduce_transparency: bool,
        pub increase_contrast: bool,
    }
}

// ---------------------------------------------------------------------------------------------------------
// The method table
// ---------------------------------------------------------------------------------------------------------

/// Who sends a method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    ClientToHost,
    HostToClient,
}

/// Request (has a result) or notification (fire and forget).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Request,
    Notification,
}

/// One row of the method table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: &'static str,
    pub direction: Direction,
    pub kind: MethodKind,
    /// The Rust/TS type of the params object.
    pub params: &'static str,
    /// The type of the result, for requests.
    pub result: Option<&'static str>,
    pub doc: &'static str,
}

const fn request(
    name: &'static str,
    params: &'static str,
    result: &'static str,
    doc: &'static str,
) -> MethodSpec {
    MethodSpec {
        name,
        direction: Direction::ClientToHost,
        kind: MethodKind::Request,
        params,
        result: Some(result),
        doc,
    }
}
const fn from_client(name: &'static str, params: &'static str, doc: &'static str) -> MethodSpec {
    MethodSpec {
        name,
        direction: Direction::ClientToHost,
        kind: MethodKind::Notification,
        params,
        result: None,
        doc,
    }
}
const fn from_host(name: &'static str, params: &'static str, doc: &'static str) -> MethodSpec {
    MethodSpec {
        name,
        direction: Direction::HostToClient,
        kind: MethodKind::Notification,
        params,
        result: None,
        doc,
    }
}

/// Every method of the view protocol (spec §2.6).
pub const METHODS: &[MethodSpec] = &[
    request(
        "session.hello",
        "SessionHello",
        "SessionWelcome",
        "Handshake, version negotiation, formatting environment.",
    ),
    request(
        "view.open",
        "ViewOpen",
        "ViewSnapshot",
        "Create or resume a view: tree, state, client state, declarations and the first data.",
    ),
    request("view.sync", "ViewRef", "ViewSnapshot", "Full resync, after a gap or a failed op."),
    request(
        "view.event",
        "ViewEvent",
        "EventAck",
        "A user event. The ack arrives after every op and patch it caused.",
    ),
    request("state.set", "StateSet", "WriteAck", "Two-way write to a server state path."),
    request("row.set", "RowSet", "WriteAck", "Two-way write to a field of a bound row."),
    request("row.restore", "RowRestore", "WriteAck", "Undo: restore a kept revision of a bound field."),
    request("source.set", "SourceSet", "SourcePatch", "Change a source's params and/or its visible window."),
    request("view.close", "ViewRef", "null", "Flush drafts, then destroy the instance."),
    from_client("client.state", "ClientState", "Mirror of synced and persisted client entries."),
    from_client("view.resize", "ViewResize", "The window was resized (only if the app reads `w.env.*`)."),
    from_client("view.visibility", "ViewVisibility", "The window was hidden or shown."),
    from_client("session.bye", "Empty", "The client is going away."),
    from_host("view.tree", "ViewTree", "A full tree replace."),
    from_host("view.ops", "ViewOps", "Keyed structural ops."),
    from_host("data.patch", "DataPatch", "Per-binding values and row windows."),
    from_host("client.set", "ClientSet", "The app drives client-managed state."),
    from_host(
        "view.command",
        "ViewCommand",
        "Imperative one-shots: toast, focus, scrollTo, announce, copy, openUrl, haptic, closeWindow.",
    ),
    from_host("view.error", "ViewError", "A render or validation failure; the last good tree stays."),
    from_host("view.closed", "ViewClosed", "The instance is gone."),
    from_host("theme.set", "ThemeSet", "The host theme."),
    from_host(
        "window.open",
        "WindowOpen",
        "An app asked for a window (`app:openWindow`): open it, or bring it forward.",
    ),
];

/// The method called `name`.
#[must_use]
pub fn method(name: &str) -> Option<&'static MethodSpec> {
    METHODS.iter().find(|m| m.name == name)
}

wire! {
    /// Params of a method that takes none: `{}`.
    #[serde(deny_unknown_fields)]
    pub struct Empty {}
}

/// JSON-RPC error codes the protocol uses.
pub mod codes {
    /// An app handler failed; the message is safe to show.
    pub const APP_ERROR: i32 = -32010;
    /// The view does not exist (closed, or the host restarted).
    pub const NO_SUCH_VIEW: i32 = -32011;
    /// The client's `proto` is not served by this host.
    pub const UNSUPPORTED_PROTO: i32 = -32012;
    /// The tree an app rendered failed validation.
    pub const INVALID_TREE: i32 = -32013;
    /// JSON-RPC: the method does not exist.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// JSON-RPC: the params are malformed.
    pub const INVALID_PARAMS: i32 = -32602;
}

/// A frame that is not a well-formed call of a known method.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ParseError {
    #[error("unknown method `{0}`")]
    UnknownMethod(String),
    #[error("invalid params for `{method}`: {message}")]
    InvalidParams { method: String, message: String },
}

impl ParseError {
    /// The JSON-RPC error to answer with.
    #[must_use]
    pub fn to_rpc_error(&self) -> RpcError {
        let code = match self {
            Self::UnknownMethod(_) => codes::METHOD_NOT_FOUND,
            Self::InvalidParams { .. } => codes::INVALID_PARAMS,
        };
        RpcError { code, message: self.to_string(), data: None }
    }
}

/// Params arrive as a by-name object; a one-element array wrapping that object (the SDK's positional
/// convention) is accepted too.
fn params_object(params: Value) -> Value {
    match params {
        Value::Array(mut items) if items.len() == 1 => items.remove(0),
        Value::Null => Value::Object(Map::new()),
        other => other,
    }
}

fn parse<T: serde::de::DeserializeOwned>(method: &str, params: Value) -> Result<T, ParseError> {
    serde_json::from_value(params_object(params))
        .map_err(|e| ParseError::InvalidParams { method: method.to_owned(), message: e.to_string() })
}

/// A request from the client.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientRequest {
    Hello(SessionHello),
    ViewOpen(ViewOpen),
    ViewSync(ViewRef),
    ViewEvent(ViewEvent),
    StateSet(StateSet),
    RowSet(RowSet),
    RowRestore(RowRestore),
    SourceSet(SourceSet),
    ViewClose(ViewRef),
}

impl ClientRequest {
    /// Decode a request frame's method and params.
    ///
    /// # Errors
    /// [`ParseError`] for an unknown method or params of the wrong shape.
    pub fn from_rpc(method: &str, params: Value) -> Result<Self, ParseError> {
        Ok(match method {
            "session.hello" => Self::Hello(parse(method, params)?),
            "view.open" => Self::ViewOpen(parse(method, params)?),
            "view.sync" => Self::ViewSync(parse(method, params)?),
            "view.event" => Self::ViewEvent(parse(method, params)?),
            "state.set" => Self::StateSet(parse(method, params)?),
            "row.set" => Self::RowSet(parse(method, params)?),
            "row.restore" => Self::RowRestore(parse(method, params)?),
            "source.set" => Self::SourceSet(parse(method, params)?),
            "view.close" => Self::ViewClose(parse(method, params)?),
            other => return Err(ParseError::UnknownMethod(other.to_owned())),
        })
    }

    #[must_use]
    pub const fn method(&self) -> &'static str {
        match self {
            Self::Hello(_) => "session.hello",
            Self::ViewOpen(_) => "view.open",
            Self::ViewSync(_) => "view.sync",
            Self::ViewEvent(_) => "view.event",
            Self::StateSet(_) => "state.set",
            Self::RowSet(_) => "row.set",
            Self::RowRestore(_) => "row.restore",
            Self::SourceSet(_) => "source.set",
            Self::ViewClose(_) => "view.close",
        }
    }
}

/// A notification from the client.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientNotification {
    ClientState(ClientState),
    Resize(ViewResize),
    Visibility(ViewVisibility),
    Bye,
}

impl ClientNotification {
    /// Decode a notification frame's method and params.
    ///
    /// # Errors
    /// [`ParseError`] for an unknown method or params of the wrong shape.
    pub fn from_rpc(method: &str, params: Value) -> Result<Self, ParseError> {
        Ok(match method {
            "client.state" => Self::ClientState(parse(method, params)?),
            "view.resize" => Self::Resize(parse(method, params)?),
            "view.visibility" => Self::Visibility(parse(method, params)?),
            "session.bye" => Self::Bye,
            other => return Err(ParseError::UnknownMethod(other.to_owned())),
        })
    }
}

/// A notification from the host.
#[derive(Clone, Debug, PartialEq)]
pub enum HostNotification {
    Tree(ViewTree),
    Ops(ViewOps),
    Data(DataPatch),
    ClientSet(ClientSet),
    Command(ViewCommand),
    Error(ViewError),
    Closed(ViewClosed),
    Theme(ThemeSet),
    WindowOpen(WindowOpen),
}

impl HostNotification {
    /// The method name and params object.
    #[must_use]
    pub fn to_rpc(&self) -> (&'static str, Value) {
        fn value<T: Serialize>(params: &T) -> Value {
            serde_json::to_value(params).unwrap_or(Value::Null)
        }
        match self {
            Self::Tree(p) => ("view.tree", value(p)),
            Self::Ops(p) => ("view.ops", value(p)),
            Self::Data(p) => ("data.patch", value(p)),
            Self::ClientSet(p) => ("client.set", value(p)),
            Self::Command(p) => ("view.command", value(p)),
            Self::Error(p) => ("view.error", value(p)),
            Self::Closed(p) => ("view.closed", value(p)),
            Self::Theme(p) => ("theme.set", value(p)),
            Self::WindowOpen(p) => ("window.open", value(p)),
        }
    }

    /// The JSON-RPC frame.
    #[must_use]
    pub fn to_message(&self) -> RpcMessage {
        let (method, params) = self.to_rpc();
        RpcMessage::notification(method, params)
    }

    /// Decode a notification frame (the client's side of the wire, and tests).
    ///
    /// # Errors
    /// [`ParseError`] for an unknown method or params of the wrong shape.
    pub fn from_rpc(method: &str, params: Value) -> Result<Self, ParseError> {
        Ok(match method {
            "view.tree" => Self::Tree(parse(method, params)?),
            "view.ops" => Self::Ops(parse(method, params)?),
            "data.patch" => Self::Data(parse(method, params)?),
            "client.set" => Self::ClientSet(parse(method, params)?),
            "view.command" => Self::Command(parse(method, params)?),
            "view.error" => Self::Error(parse(method, params)?),
            "view.closed" => Self::Closed(parse(method, params)?),
            "theme.set" => Self::Theme(parse(method, params)?),
            "window.open" => Self::WindowOpen(parse(method, params)?),
            other => return Err(ParseError::UnknownMethod(other.to_owned())),
        })
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
