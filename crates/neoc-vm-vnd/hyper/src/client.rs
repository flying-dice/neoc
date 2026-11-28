//! `vnd:hyper` client path: one HTTP/1.1 exchange behind `get`, `post` and `request`, made by `neoc-net`. The server
//! half lives in `super::server`, the script's surface in `super`.
//!
//! `http://` and `https://` both work. TLS is rustls over the `ring` provider: the certificate is checked by the
//! platform verifier (the operating system's trust store). A service with its own CA or a self-signed certificate
//! passes `ca_pem = "<PEM>"` in the options: then exactly those certificates are trusted for every hop of that
//! request.
//!
//! What a request does when the caller sets nothing is decided in NEOC-259, after `URLSession` and Swift's
//! async-http-client: it gives up after 10 s of connecting and after 60 s without data, follows up to 16 redirects,
//! and reads at most 64 MiB of body. Each has an option to change it. A failure is `(nil, err)` with a typed error of
//! a stable kind.
//!
//! Every failed request returns `(nil, err)` where `err` is `{ kind, message, op, <the kind's fields> }` and
//! `tostring(err)` is `vnd:hyper.<function>: <kind>: <message>` (`vnd:hyper.get: timed_out: no data for 60s`). The
//! `kind` is the same on every platform, so a script branches on it (`err.kind == "timed_out"`); the message is for
//! a log, and a script must not parse it.

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use mlua::Lua;
use mlua_ir::{LuaError, Options, Shape};
use neoc_net::{ClientOptions, NetError, Prepared, RequestBody, Spawner};
use serde::{Deserialize, Serialize};

use neoc_vm_core::lifecycle::Life;

#[cfg(test)]
mod tests;

pub(super) use neoc_net::FullResponse as Response;

/// Which time limit a `timed_out` request passed: the option that sets it, or the operating system's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "vnd:hyper")]
pub enum TimeLimit {
    /// Connecting (the name lookup, the TCP connect and the TLS handshake) took longer than `connect_timeout`.
    ConnectTimeout,
    /// No data moved in either direction for `idle_timeout`.
    IdleTimeout,
    /// The whole request, redirects and body included, took longer than `timeout`.
    Timeout,
    /// The operating system gave up on the connection by a limit of its own.
    System,
}

impl From<neoc_net::TimeLimit> for TimeLimit {
    fn from(limit: neoc_net::TimeLimit) -> Self {
        match limit {
            neoc_net::TimeLimit::ConnectTimeout => Self::ConnectTimeout,
            neoc_net::TimeLimit::IdleTimeout => Self::IdleTimeout,
            neoc_net::TimeLimit::Timeout => Self::Timeout,
            neoc_net::TimeLimit::System => Self::System,
        }
    }
}

/// Why a request failed. The kind means the same on every platform; `reason` is this module's or the
/// transport's wording, for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[shape(module = "vnd:hyper")]
pub enum HyperError {
    /// An option, a header, the method or the body was refused before anything was sent.
    #[error("{reason}")]
    InvalidInput {
        /// What was refused, and why.
        reason: String,
    },
    /// The URL (or a redirect's `Location`) is not an `http` or `https` URL with a host.
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
    /// A time limit passed: connecting, no data arriving, or the whole request.
    #[error("{reason}")]
    TimedOut {
        /// Which limit.
        limit: TimeLimit,
        /// The limit and how long it was.
        reason: String,
    },
    /// More redirects than `max_redirects` allows.
    #[error("more than {max_redirects} redirects")]
    TooManyRedirects {
        /// The limit that was passed.
        max_redirects: u32,
    },
    /// The response body is longer than `max_body`.
    #[error("the response body is longer than max_body ({max_body} bytes)")]
    TooLarge {
        /// The limit that was passed, in bytes.
        max_body: u64,
    },
    /// The connection ended or failed after it was made, before the response was complete.
    #[error("{reason}")]
    ConnectionLost {
        /// What the transport said.
        reason: String,
    },
    /// The server answered with something that is not valid HTTP.
    #[error("{reason}")]
    BadResponse {
        /// What the parser said.
        reason: String,
    },
    /// Anything else, reported and never hidden.
    #[error("{reason}")]
    Other {
        /// What went wrong.
        reason: String,
    },
}

impl From<NetError> for HyperError {
    fn from(error: NetError) -> Self {
        match error {
            NetError::InvalidInput { reason } => Self::InvalidInput { reason },
            NetError::InvalidUrl { reason } => Self::InvalidUrl { reason },
            NetError::Connect { reason } => Self::Connect { reason },
            NetError::Tls { reason } => Self::Tls { reason },
            NetError::TimedOut { limit, reason } => Self::TimedOut { limit: limit.into(), reason },
            NetError::TooManyRedirects { max_redirects } => Self::TooManyRedirects { max_redirects },
            NetError::TooLarge { max_body } => Self::TooLarge { max_body },
            NetError::ConnectionLost { reason } => Self::ConnectionLost { reason },
            NetError::BadResponse { reason } => Self::BadResponse { reason },
            // a WebSocket's kinds: never what an HTTP request fails with
            other @ (NetError::Status { .. } | NetError::Protocol { .. } | NetError::Other { .. }) => {
                Self::Other { reason: other.to_string() }
            }
        }
    }
}

/// What a request sends besides its method, URL and body, and the limits it keeps to.
#[derive(Debug, Clone, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:hyper")]
pub struct RequestOptions {
    /// Headers to send, by name. A `Content-Length` that is not the body's length is refused, a `Host` replaces
    /// the built-in one, and a name given twice in any case or a `Transfer-Encoding` is refused: the module frames
    /// the body.
    pub headers: BTreeMap<String, String>,
    /// PEM certificates to trust instead of the platform's, on every hop.
    pub ca_pem: Option<String>,
    /// Seconds the TCP connect and the TLS handshake may take.
    pub connect_timeout: f64,
    /// Seconds without data moving in either direction; reset whenever any does, so a server that trickles never
    /// trips it.
    pub idle_timeout: f64,
    /// Seconds the whole request may take, redirects and body included, which is what bounds a trickle. No limit
    /// when not given.
    pub timeout: Option<f64>,
    /// Follow 301, 302, 303, 307 and 308; `false` returns the 3xx. A 303 becomes a GET, a POST becomes a GET on 301
    /// and 302, 307 and 308 keep method and body. A hop to another origin drops `Authorization`, `Cookie`,
    /// `Proxy-Authorization`, `Origin` and a caller-set `Host`, and only those: a credential in a header of your
    /// own (`X-Api-Key`) goes to every hop. Nothing refuses a redirect to a host on the local network or from
    /// `https` to `http`: check the response's `url`, or follow redirects yourself.
    pub follow_redirects: bool,
    /// The most redirects to follow; one more is a `too_many_redirects` error.
    pub max_redirects: u32,
    /// The most bytes of response body to read; a longer one fails with `too_large`. The module never
    /// decompresses, so this bounds the bytes on the wire.
    pub max_body: u64,
}

impl Default for RequestOptions {
    fn default() -> Self {
        let net = ClientOptions::default();
        Self {
            headers: net.headers,
            ca_pem: net.ca_pem,
            connect_timeout: net.connect_timeout,
            idle_timeout: net.idle_timeout,
            timeout: net.timeout,
            follow_redirects: net.follow_redirects,
            max_redirects: net.max_redirects,
            max_body: net.max_body,
        }
    }
}

impl From<RequestOptions> for ClientOptions {
    fn from(opts: RequestOptions) -> Self {
        Self {
            headers: opts.headers,
            ca_pem: opts.ca_pem,
            connect_timeout: opts.connect_timeout,
            idle_timeout: opts.idle_timeout,
            timeout: opts.timeout,
            follow_redirects: opts.follow_redirects,
            max_redirects: opts.max_redirects,
            max_body: opts.max_body,
        }
    }
}

/// The spawner of the engine's [`Life`]: a shut-down engine closes the connections.
pub(crate) fn spawner(lua: &Lua) -> Spawner {
    let life = Life::of(lua);
    Arc::new(move |future| life.spawn(future).abort_handle())
}

/// Send `method url` with `body` as `opts` says, and read the response. The connections it opens belong to the
/// engine's [`Life`]: a shut-down engine closes them.
pub(super) async fn send(
    lua: &Lua,
    method: &str,
    url: String,
    body: Option<Vec<u8>>,
    opts: RequestOptions,
) -> Result<Response, HyperError> {
    let body = body.map_or(RequestBody::Empty, |body| RequestBody::Full(Bytes::from(body)));
    let prepared = Prepared::new(method, url, body, &opts.into())?;
    Ok(neoc_net::fetch(prepared, &spawner(lua)).await?)
}
