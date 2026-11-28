//! `lib:websocket`: a WebSocket client (RFC 6455), modelled on Swift's `URLSessionWebSocketTask` and Bun's
//! `WebSocket`.
//!
//! ```lua
//! local websocket = require("lib:websocket")
//! local ws = assert(websocket.connect("wss://example.com/socket", { protocols = { "chat" } }))
//! assert(ws:send("hello"))                 -- a text message
//! assert(ws:send_binary("\0\1\2"))         -- a binary message
//! local msg = assert(ws:receive())         -- { kind = "text" | "binary", data = "..." }
//! for msg in ws:messages():items() do print(msg.kind, msg.data) end
//! ws:close(1000, "done")
//! ```
//!
//! The API's rules:
//! - `connect` waits until the upgrade is done: there is no "connecting" state to watch. A connection that cannot
//!   be made is `(nil, err)` with a typed `WebSocketError` (`connect`, `tls`, `timed_out`, `status` for an upgrade
//!   answered with an HTTP status, ...).
//! - `receive` waits for the next message: `(message, nil)`; `(nil, nil)` once the socket is closed, with
//!   `close_code` and `close_reason` saying how; or `(nil, err)` when the connection failed. Messages queue while
//!   nobody reads them, up to a bound; then the socket stops reading, and the peer waits.
//! - Pings are answered for the script; `ping` sends one.
//! - `close(code?, reason?)` sends a close frame (1000 when no code is given; a code of 1000 or 3000 to 4999, a
//!   reason of at most 123 bytes) and returns; the peer's answer ends `receive`. A socket the script lets go of, or
//!   whose engine shuts down, is closed.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

mod socket;

pub use socket::Lines;

#[mlua_ir::module("lib:websocket")]
pub mod websocket {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use mlua::Lua;
    use mlua_ir::{Bytes, LuaError, Object, Options, Shape, Stream};
    use neoc_net::NetError;
    use serde::{Deserialize, Serialize};

    use super::socket::{Inner, Lines};

    /// Which time limit a `timed_out` connection passed.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum TimeLimit {
        /// The TCP connect, the TLS handshake and the upgrade took longer than `connect_timeout`.
        ConnectTimeout,
        /// The operating system gave up on the connection by a limit of its own.
        System,
    }

    /// Why a connection or a call failed. `reason` is for a person or a log.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum WebSocketError {
        /// An option, a header, a close code or a reason was refused before anything was sent.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// The URL is not a `ws` or `wss` URL with a host.
        #[error("{reason}")]
        InvalidUrl {
            /// What is wrong with it.
            reason: String,
        },
        /// No connection could be made: the name did not resolve, or the host refused or could not be reached.
        #[error("{reason}")]
        Connect {
            /// What the transport said.
            reason: String,
        },
        /// The TLS handshake failed, usually over the certificate.
        #[error("{reason}")]
        Tls {
            /// What the handshake said.
            reason: String,
        },
        /// A time limit passed.
        #[error("{reason}")]
        TimedOut {
            /// Which limit.
            limit: TimeLimit,
            /// The limit and how long it was.
            reason: String,
        },
        /// The server answered the upgrade with an HTTP status instead (`401`, `403`, `404`).
        #[error("the server answered {code}{}", said(.body))]
        Status {
            /// The status code.
            code: u16,
            /// The start of the response's body, as text.
            body: String,
        },
        /// The peer broke the protocol: an upgrade or a frame that is not what RFC 6455 says, or a message over
        /// 64 MiB.
        #[error("{reason}")]
        Protocol {
            /// What is wrong.
            reason: String,
        },
        /// The connection failed after it was made.
        #[error("{reason}")]
        ConnectionLost {
            /// What the transport said.
            reason: String,
        },
        /// The socket is closed (by `close`, by the peer, or by the engine shutting down), or a transport took it
        /// over (`vnd:agent_client_protocol`).
        #[error("{reason}")]
        Closed {
            /// Why.
            reason: String,
        },
        /// Anything else, reported and never hidden.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    fn said(body: &str) -> String {
        if body.is_empty() { String::new() } else { format!(": {body}") }
    }

    impl From<NetError> for WebSocketError {
        fn from(error: NetError) -> Self {
            match error {
                NetError::InvalidInput { reason } => Self::InvalidInput { reason },
                NetError::InvalidUrl { reason } => Self::InvalidUrl { reason },
                NetError::Connect { reason } => Self::Connect { reason },
                NetError::Tls { reason } => Self::Tls { reason },
                NetError::TimedOut { limit, reason } => Self::TimedOut {
                    limit: match limit {
                        neoc_net::TimeLimit::ConnectTimeout => TimeLimit::ConnectTimeout,
                        _ => TimeLimit::System,
                    },
                    reason,
                },
                NetError::Status { code, body } => Self::Status { code, body },
                NetError::Protocol { reason } | NetError::BadResponse { reason } => Self::Protocol { reason },
                NetError::ConnectionLost { reason } => Self::ConnectionLost { reason },
                other @ (NetError::TooManyRedirects { .. }
                | NetError::TooLarge { .. }
                | NetError::Other { .. }) => Self::Other { reason: other.to_string() },
            }
        }
    }

    /// How `connect` connects.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    #[serde(default, deny_unknown_fields)]
    pub struct ConnectOptions {
        /// Headers to add to the upgrade request (`Authorization`, `Origin`); no `Origin` is sent unless given. The
        /// upgrade's own headers (`Upgrade`, `Connection`, `Sec-WebSocket-*`) are refused.
        pub headers: BTreeMap<String, String>,
        /// The subprotocols to offer, in order of preference; `protocol` is the one the server chose.
        pub protocols: Vec<String>,
        /// PEM certificates to trust instead of the platform's.
        pub ca_pem: Option<String>,
        /// Seconds the TCP connect, the TLS handshake and the upgrade may take together.
        pub connect_timeout: f64,
    }

    impl Default for ConnectOptions {
        fn default() -> Self {
            Self {
                headers: BTreeMap::new(),
                protocols: Vec::new(),
                ca_pem: None,
                connect_timeout: neoc_net::DEFAULT_CONNECT.as_secs_f64(),
            }
        }
    }

    /// What a message holds.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum MessageKind {
        /// Text: UTF-8.
        Text,
        /// Bytes.
        Binary,
    }

    /// A message the peer sent.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
    pub struct Message {
        /// Text or bytes.
        pub kind: MessageKind,
        /// The message: a string whatever its bytes.
        pub data: Data,
    }

    /// The bytes of a message as a field of a record: a Lua string, whatever its bytes. Declared `string`.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct Data(pub Vec<u8>);

    impl Shape for Data {
        fn shape(_: &mut mlua_ir::Registry) -> mlua_ir::ir::Ty {
            mlua_ir::ir::Ty::String
        }
    }

    impl Serialize for Data {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_bytes(&self.0)
        }
    }

    impl<'de> Deserialize<'de> for Data {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let bytes: serde_bytes_like::Bytes = Deserialize::deserialize(deserializer)?;
            Ok(Data(bytes.0))
        }
    }

    mod serde_bytes_like {
        /// What a string or a byte string deserializes to.
        pub struct Bytes(pub Vec<u8>);

        impl<'de> serde::Deserialize<'de> for Bytes {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Visitor;

                impl serde::de::Visitor<'_> for Visitor {
                    type Value = Bytes;

                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("a string")
                    }

                    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Bytes, E> {
                        Ok(Bytes(text.as_bytes().to_vec()))
                    }

                    fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Bytes, E> {
                        Ok(Bytes(bytes.to_vec()))
                    }
                }

                deserializer.deserialize_any(Visitor)
            }
        }
    }

    /// Connect to `url` (`ws://` or `wss://`) and complete the upgrade.
    ///
    /// ```luau
    /// local ws, err = websocket.connect("ws://127.0.0.1:1/socket", { connect_timeout = 2 })
    /// if ws == nil then
    ///     assert(err ~= nil and (err.kind == "connect" or err.kind == "timed_out"))
    /// end
    /// ```
    #[host("Wait")]
    pub async fn connect(lua: Lua, url: String, opts: ConnectOptions) -> Result<WebSocket, WebSocketError> {
        let connect_timeout = Duration::try_from_secs_f64(opts.connect_timeout)
            .ok()
            .filter(|d| !d.is_zero())
            .ok_or_else(|| WebSocketError::InvalidInput {
                reason: format!(
                    "connect_timeout must be a positive, finite number of seconds, got {}",
                    opts.connect_timeout
                ),
            })?;
        let options = neoc_net::websocket::Options {
            headers: opts.headers,
            protocols: opts.protocols,
            ca_pem: opts.ca_pem,
            connect_timeout,
        };
        let connected = neoc_net::websocket::connect(&url, &options).await?;
        Ok(WebSocket { inner: Inner::start(&lua, url, connected) })
    }

    /// An open WebSocket. Copies of the handle are the same socket; when the last one goes, the socket closes.
    #[derive(Clone, Object)]
    pub struct WebSocket {
        inner: Arc<Inner>,
    }

    impl WebSocket {
        /// Take the socket over as a transport of text lines, one per text message (`vnd:agent_client_protocol`
        /// hands it to the ACP SDK this way). From then on the script's `receive` gives `closed`; `send` and
        /// `close` still work, and the socket closes when the transport lets go of the lines. `None` when a
        /// transport has it already, or the script's `receive` is waiting.
        pub fn take_lines(&self) -> Option<Lines> {
            Inner::take_lines(&self.inner)
        }

        /// The IP address of this end of the connection.
        pub fn local_address_of(&self) -> String {
            self.inner.local_address.clone()
        }
    }

    #[mlua_ir::object]
    impl WebSocket {
        /// The URL connected to.
        #[get]
        pub fn url(&self) -> String {
            self.inner.url.clone()
        }

        /// The subprotocol the server chose; `nil` when it chose none.
        #[get]
        pub fn protocol(&self) -> Option<String> {
            self.inner.protocol.clone()
        }

        /// The IP address of this end of the connection: the address this machine has on the way to the peer.
        #[get]
        pub fn local_address(&self) -> String {
            self.inner.local_address.clone()
        }

        /// The close code the peer sent (1000 for a normal close, 1005 when its close frame had none); `nil` while
        /// the socket is open.
        #[get]
        pub fn close_code(&self) -> Option<u16> {
            self.inner.closed().map(|(code, _)| code)
        }

        /// The reason the peer gave with its close; `nil` while the socket is open.
        #[get]
        pub fn close_reason(&self) -> Option<String> {
            self.inner.closed().map(|(_, reason)| reason)
        }

        /// Send `text` as a text message. Waits while the peer is not reading.
        pub async fn send(&self, text: String) -> Result<bool, WebSocketError> {
            self.inner.send_text(text).await.map(|()| true)
        }

        /// Send `data` as a binary message. Waits while the peer is not reading.
        pub async fn send_binary(&self, data: Bytes) -> Result<bool, WebSocketError> {
            self.inner.send_binary(data.0).await.map(|()| true)
        }

        /// Send a ping (with up to 125 bytes of `data`); the peer's pong is not given to the script.
        pub async fn ping(&self, data: Option<Bytes>) -> Result<bool, WebSocketError> {
            self.inner.ping(data.map(|data| data.0).unwrap_or_default()).await.map(|()| true)
        }

        /// The next message: `(message, nil)`; `(nil, nil)` once the socket is closed; or `(nil, err)` when the
        /// connection failed.
        pub async fn receive(&self) -> Result<Option<Message>, WebSocketError> {
            self.inner.receive().await
        }

        /// The messages, read as they come: `next()` or `items()`, ending when the socket closes.
        pub fn messages(&self) -> Stream<Message, WebSocketError> {
            let inner = Arc::clone(&self.inner);
            Stream::new(futures_util::stream::unfold(Some(inner), |inner| async move {
                let inner = inner?;
                match inner.receive().await {
                    Ok(Some(message)) => Some((Ok(message), Some(inner))),
                    Ok(None) => None,
                    Err(e) => Some((Err(e), None)),
                }
            }))
        }

        /// Close the socket: send a close frame with `code` (1000 when not given; 1000, or 3000 to 4999) and
        /// `reason` (at most 123 bytes), and return. `receive` gives what the peer sent before its answer, then
        /// `nil`. Closing again does nothing.
        pub async fn close(&self, code: Option<u16>, reason: Option<String>) -> Result<bool, WebSocketError> {
            self.inner.close(code.unwrap_or(1000), reason.unwrap_or_default()).await.map(|()| true)
        }
    }
}

#[cfg(test)]
mod tests;
