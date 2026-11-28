//! The network clients under neoc's modules: HTTP/1.1 requests and WebSocket connections, over TCP and TLS.
//!
//! No Lua and nothing of vos: a module (`vnd:hyper`, `lib:http`, `lib:websocket`) reads its script's arguments,
//! hands the values here, and gives the result back in its own types. What every client shares is decided once:
//!
//! - **TLS** is rustls over the `ring` provider; the certificate is checked by the platform verifier (the operating
//!   system's trust store), or by exactly the certificates of a `ca_pem` the caller gives (`tls`).
//! - **Connecting** goes through hyper-util's `HttpConnector`, which races the addresses a name resolves to (Happy
//!   Eyeballs, RFC 8305), within a connect timeout that covers the TLS handshake too.
//! - **An HTTP request** ([`send`], [`fetch`]) has the limits NEOC-259 decided, after `URLSession` and Swift's
//!   async-http-client (`options`): 10 s to connect, 60 s without data, 16 redirects, 64 MiB of body read whole. Its
//!   body can be streamed both ways ([`RequestBody::Stream`], [`Body::chunk`]).
//! - **A WebSocket** ([`websocket::connect`]) is tungstenite over the same connection, after RFC 6455.
//! - **A failure** is a [`NetError`] of a stable kind.
//!
//! The tasks a client starts (a connection's driver) are started with the caller's [`Spawner`], so an engine that
//! registers them closes the connections when it shuts down.

mod error;
mod exchange;
mod idle;
mod options;
mod redirect;
mod tls;
pub mod websocket;

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::FutureExt as _;
use tokio::task::AbortHandle;

pub use error::{NetError, TimeLimit, chain};
pub use exchange::{Body, FullResponse, Response, fetch, send};
pub use options::{
    ClientOptions, DEFAULT_CONNECT, DEFAULT_IDLE, DEFAULT_MAX_BODY, DEFAULT_REDIRECTS, Prepared, RequestBody,
};

/// Starts a task the client needs (a connection's driver) and gives the handle that stops it. An engine registers
/// the task, so that shutting the engine down stops it.
pub type Spawner = Arc<dyn Fn(Pin<Box<dyn Future<Output = ()> + Send>>) -> AbortHandle + Send + Sync>;

/// A spawner of plain tokio tasks, for a caller with nothing to register them with (a test).
pub fn tokio_spawner() -> Spawner {
    Arc::new(|future| tokio::spawn(future).abort_handle())
}

/// A `HeaderMap` as a script has it: one entry per name (lower case). A header sent more than once is one value, the
/// values joined with `", "` in the order they came, as the Fetch standard's `Headers.get` does, except `set-cookie`,
/// whose values are joined with `"\n"`: a cookie may hold a comma (`Expires`), a field value never holds a line feed,
/// so splitting on `"\n"` gives each cookie back. A value that is not UTF-8 is decoded with replacement characters.
/// Nothing is dropped.
pub fn collect_headers(headers: &hyper::HeaderMap) -> Vec<(String, String)> {
    headers
        .keys()
        .map(|name| {
            let values: Vec<_> =
                headers.get_all(name).iter().map(|v| String::from_utf8_lossy(v.as_bytes())).collect();
            let separator = if name == hyper::header::SET_COOKIE { "\n" } else { ", " };
            (name.as_str().to_owned(), values.join(separator))
        })
        .collect()
}

/// Run `future`; a panic in it ends it with the panic's message.
async fn caught<F: Future>(future: F) -> Result<F::Output, String> {
    AssertUnwindSafe(future).catch_unwind().await.map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with no message".to_owned())
    })
}
