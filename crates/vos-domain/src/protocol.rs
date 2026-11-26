//! The `/rpc` socket: the JSON-RPC 2.0 envelope, the handshake (`session.hello`), what the host tells a shell
//! without being asked (`window.open`, `notification.deliver`), the error codes and the method table of those.
//!
//! The app-management methods (`app.*`, `agents.*`, `host.devices.*`) and the `app.event` notification are plain
//! rows in [`crate::api`].

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Num(i64),
    Str(String),
}

/// Any inbound/outbound frame on the socket. Order matters for `untagged`: a Request has `method`+`id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcMessage {
    Request {
        jsonrpc: Version,
        id: RpcId,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Notification {
        jsonrpc: Version,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Response {
        jsonrpc: Version,
        id: RpcId,
        result: Value,
    },
    Error {
        jsonrpc: Version,
        id: Option<RpcId>,
        error: RpcError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Version {
    #[serde(rename = "2.0")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcMessage {
    pub fn request(id: i64, method: &str, params: Value) -> Self {
        Self::Request { jsonrpc: Version::V2, id: RpcId::Num(id), method: method.into(), params }
    }
    pub fn notification(method: &str, params: Value) -> Self {
        Self::Notification { jsonrpc: Version::V2, method: method.into(), params }
    }
    pub fn ok(id: RpcId, result: Value) -> Self {
        Self::Response { jsonrpc: Version::V2, id, result }
    }
    pub fn err(id: Option<RpcId>, code: i32, message: &str) -> Self {
        Self::Error {
            jsonrpc: Version::V2,
            id,
            error: RpcError { code, message: message.into(), data: None },
        }
    }
}

/// Applies the standard derives, and the codegen one behind the feature.
macro_rules! wire {
    ($($item:item)*) => {
        $(
            #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
            #[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
            $item
        )*
    };
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
        /// The session a reconnecting client had. The host reads nothing from it: a welcome is never `resumed`.
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

    /// Params of a method that takes none: `{}`.
    #[serde(deny_unknown_fields)]
    pub struct Empty {}
}

// ---------------------------------------------------------------------------------------------------------
// Host notifications
// ---------------------------------------------------------------------------------------------------------

wire! {
    /// `window.open`: an app asked for its window (`app.open_window`, e.g. from an agent's tool call). The shell
    /// opens the app's window, or brings its open one forward.
    pub struct WindowOpen {
        pub app: String,
        /// The window kind (`"main"`).
        pub window: String,
        /// What the app passed to `app.open_window` for its page.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub params: Option<Map<String, Value>>,
    }

    /// `notification.deliver`: an app asked to tell the person something (`app.notify`). Sent to every connected
    /// client; one that is not connected never gets it (nothing is queued). `app` is the sender's id, set by the
    /// host, never by the app, so a client can name the sender from its own app list.
    ///
    /// `title` and `body` are plain text written by the app (or an agent through one): show them as text, never as
    /// markup. The host enforces their limits in `app.notify` before it sends, so a client can rely on them.
    pub struct AppNotification {
        pub app: String,
        /// 1 to 200 characters, not blank.
        pub title: String,
        /// Up to 2000 characters.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub body: Option<String>,
    }
}

/// A notification from the host.
#[derive(Clone, Debug, PartialEq)]
pub enum HostNotification {
    WindowOpen(WindowOpen),
    Notify(AppNotification),
}

impl HostNotification {
    /// The method name and params object.
    #[must_use]
    pub fn to_rpc(&self) -> (&'static str, Value) {
        fn value<T: Serialize>(params: &T) -> Value {
            serde_json::to_value(params).unwrap_or(Value::Null)
        }
        match self {
            Self::WindowOpen(p) => ("window.open", value(p)),
            Self::Notify(p) => ("notification.deliver", value(p)),
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
            "window.open" => Self::WindowOpen(parse(method, params)?),
            "notification.deliver" => Self::Notify(parse(method, params)?),
            other => return Err(ParseError::UnknownMethod(other.to_owned())),
        })
    }
}

// ---------------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------------

/// JSON-RPC error codes the protocol uses.
pub mod codes {
    /// The server will not take another request now (too many on the connection, too many calls of one app running): nothing was
    /// started, and asking again later is allowed.
    pub const BUSY: i32 = -32000;
    /// An app handler failed; the message is safe to show.
    pub const APP_ERROR: i32 = -32010;
    /// The client's `proto` is not served by this host.
    pub const UNSUPPORTED_PROTO: i32 = -32012;
    /// An answer to a request on the bus that no longer waits: another handler answered first, or it was cancelled.
    pub const EXPIRED: i32 = -32013;
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

/// Params arrive as a by-name object; an absent `params` is an empty one. Anything else (an array included) is
/// left to the typed parse, which refuses it.
///
/// # Errors
/// [`ParseError::InvalidParams`] when the params are not the shape of `T`.
pub fn parse<T: serde::de::DeserializeOwned>(method: &str, params: Value) -> Result<T, ParseError> {
    let params = match params {
        Value::Null => Value::Object(Map::new()),
        other => other,
    };
    serde_json::from_value(params)
        .map_err(|e| ParseError::InvalidParams { method: method.to_owned(), message: e.to_string() })
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

/// The session methods and the host's notifications to a shell: the source of the generated `Methods` table.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec {
        name: "session.hello",
        direction: Direction::ClientToHost,
        kind: MethodKind::Request,
        params: "SessionHello",
        result: Some("SessionWelcome"),
        doc: "Handshake, version negotiation, formatting environment.",
    },
    MethodSpec {
        name: "session.bye",
        direction: Direction::ClientToHost,
        kind: MethodKind::Notification,
        params: "Empty",
        result: None,
        doc: "The client is going away.",
    },
    MethodSpec {
        name: "window.open",
        direction: Direction::HostToClient,
        kind: MethodKind::Notification,
        params: "WindowOpen",
        result: None,
        doc: "An app asked for its window (`app.open_window`): open it, or bring it forward.",
    },
    MethodSpec {
        name: "notification.deliver",
        direction: Direction::HostToClient,
        kind: MethodKind::Notification,
        params: "AppNotification",
        result: None,
        doc: "An app's notification (`app.notify`): the sender, a title and an optional body.",
    },
];

/// The method called `name`.
#[must_use]
pub fn method(name: &str) -> Option<&'static MethodSpec> {
    METHODS.iter().find(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(
        value: &T,
        expected: &Value,
    ) {
        let wire = serde_json::to_value(value).unwrap();
        assert_eq!(&wire, expected);
        assert_eq!(&serde_json::from_value::<T>(wire).unwrap(), value);
    }

    #[test]
    fn frames_are_classified() {
        let p = |v: Value| serde_json::from_value::<RpcMessage>(v).unwrap();
        assert!(matches!(p(json!({"jsonrpc":"2.0","id":1,"method":"m"})), RpcMessage::Request { .. }));
        assert!(matches!(
            p(json!({"jsonrpc":"2.0","method":"m","params":[1]})),
            RpcMessage::Notification { .. }
        ));
        assert!(matches!(p(json!({"jsonrpc":"2.0","id":"a","result":1})), RpcMessage::Response { .. }));
        assert!(matches!(
            p(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}})),
            RpcMessage::Error { .. }
        ));
        assert!(serde_json::from_value::<RpcMessage>(json!({"jsonrpc":"1.0","id":1,"method":"m"})).is_err());
    }

    #[test]
    fn hello_and_welcome() {
        let hello = SessionHello {
            proto: 1,
            kit: "1.0".into(),
            locale: "en-GB".into(),
            time_zone: "Europe/London".into(),
            features: vec!["offline".into()],
            resume: Some(SessionResume {
                session_id: "s1".into(),
                views: vec![ResumeView { view_id: "v1".into(), rev: 4 }],
            }),
        };
        round_trip(
            &hello,
            &json!({
                "proto": 1, "kit": "1.0", "locale": "en-GB", "timeZone": "Europe/London", "features": ["offline"],
                "resume": { "sessionId": "s1", "views": [{ "viewId": "v1", "rev": 4 }] }
            }),
        );
        round_trip(
            &SessionWelcome {
                proto: 1,
                kit: "1.0".into(),
                host_version: "0.1.0".into(),
                session_id: "s1".into(),
                resumed: false,
            },
            &json!({ "proto": 1, "kit": "1.0", "hostVersion": "0.1.0", "sessionId": "s1", "resumed": false }),
        );
    }

    #[test]
    fn window_open_carries_the_app_the_window_kind_and_optional_params() {
        round_trip(
            &WindowOpen { app: "calculator".into(), window: "main".into(), params: None },
            &json!({ "app": "calculator", "window": "main" }),
        );
        let note = HostNotification::WindowOpen(WindowOpen {
            app: "notes".into(),
            window: "main".into(),
            params: json!({ "noteId": 12 }).as_object().cloned(),
        });
        let frame = serde_json::to_value(note.to_message()).unwrap();
        assert_eq!(frame["method"], "window.open");
        assert_eq!(frame["params"]["params"]["noteId"], 12);
    }

    #[test]
    fn an_app_notification_is_the_sender_a_title_and_an_optional_body() {
        round_trip(
            &AppNotification { app: "notes".into(), title: "Note added".into(), body: None },
            &json!({ "app": "notes", "title": "Note added" }),
        );
        let note = HostNotification::Notify(AppNotification {
            app: "notes".into(),
            title: "Reminder".into(),
            body: Some("Call mum".into()),
        });
        let frame = serde_json::to_value(note.to_message()).unwrap();
        assert_eq!(
            frame,
            json!({ "jsonrpc": "2.0", "method": "notification.deliver", "params": { "app": "notes", "title": "Reminder", "body": "Call mum" } })
        );
        assert_eq!(
            HostNotification::from_rpc("notification.deliver", frame["params"].clone()).unwrap(),
            note
        );
        // a client cannot read a notification without its sender or its title
        for params in [json!({ "title": "x" }), json!({ "app": "notes" })] {
            let e = HostNotification::from_rpc("notification.deliver", params).unwrap_err();
            assert_eq!(e.to_rpc_error().code, codes::INVALID_PARAMS);
        }
    }

    #[test]
    fn the_method_table_has_unique_namespaced_names() {
        let mut seen = std::collections::HashSet::new();
        for m in METHODS {
            assert!(seen.insert(m.name), "duplicate {}", m.name);
            assert!(m.name.contains('.'), "{} is not namespaced", m.name);
            assert_eq!(m.result.is_some(), m.kind == MethodKind::Request, "{}", m.name);
        }
        assert_eq!(METHODS.iter().filter(|m| m.kind == MethodKind::Request).count(), 1);
        assert_eq!(method("window.open").map(|m| m.direction), Some(Direction::HostToClient));
        assert!(method("nope").is_none());
    }

    #[test]
    fn every_method_in_the_table_decodes_as_its_params_type() {
        for m in METHODS {
            match m.name {
                "session.hello" => {
                    let params = json!({ "proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC" });
                    parse::<SessionHello>(m.name, params).unwrap();
                }
                "session.bye" => {
                    parse::<Empty>(m.name, Value::Null).unwrap();
                }
                "window.open" => {
                    let note =
                        HostNotification::from_rpc(m.name, json!({ "app": "calculator", "window": "main" }));
                    assert_eq!(note.unwrap().to_rpc().0, m.name);
                }
                "notification.deliver" => {
                    let note =
                        HostNotification::from_rpc(m.name, json!({ "app": "notes", "title": "Note added" }));
                    assert_eq!(note.unwrap().to_rpc().0, m.name);
                }
                other => panic!("no sample for {other}"),
            }
        }
    }

    #[test]
    fn params_are_a_by_name_object_and_an_array_is_refused() {
        let hello = json!({ "proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC" });
        assert_eq!(parse::<SessionHello>("session.hello", hello.clone()).unwrap().proto, 1);
        let wrapped = parse::<SessionHello>("session.hello", json!([hello])).unwrap_err();
        assert_eq!(wrapped.to_rpc_error().code, codes::INVALID_PARAMS);
        assert!(wrapped.to_string().contains("session.hello"));
        let unknown = HostNotification::from_rpc("view.explode", json!({})).unwrap_err();
        assert_eq!(unknown.to_rpc_error().code, codes::METHOD_NOT_FOUND);
    }
}
