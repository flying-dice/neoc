//! `vnd:hyper` client path: one HTTP/1.1 exchange behind `get`, `post` and `request`. The server half lives in
//! `super::server`, the script's surface in `super`.
//!
//! `http://` and `https://` both work. TLS is rustls over the `ring` provider: the certificate is checked by the
//! platform verifier (the operating system's trust store). A service with its own CA or a self-signed certificate
//! passes `ca_pem = "<PEM>"` in the options: then exactly those certificates are trusted for every hop of that
//! request.
//!
//! What a request does when the caller sets nothing is decided in NEOC-259, after `URLSession` and Swift's
//! async-http-client: it gives up after 10 s of connecting and after 60 s without data, follows up to 16 redirects
//! (`redirect`), and reads at most 64 MiB of body (`exchange`). Each has an option to change it. A failure is
//! `(nil, err)` with a typed error of a stable kind (`error`).

use mlua::Lua;

use crate::lifecycle::Life;

mod error;
mod exchange;
mod idle;
mod options;
mod redirect;
mod tls;

#[cfg(test)]
mod tests;

pub use error::{HyperError, TimeLimit};
pub(super) use exchange::Response;
// what a module that reads a response as a stream needs (`lib:agui`): the same request, connection and errors
pub(crate) use exchange::Streamed;
pub(crate) use options::Prepared;
pub use options::RequestOptions;

/// Send `method url` with `body` as `opts` says, and read the response. The connections it opens belong to the
/// engine's [`Life`]: a shut-down engine closes them.
pub(super) async fn send(
    lua: &Lua,
    method: &str,
    url: String,
    body: Option<Vec<u8>>,
    opts: RequestOptions,
) -> Result<Response, HyperError> {
    let prepared = options::Prepared::new(method, url, body, opts)?;
    exchange::run(prepared, Life::of(lua)).await
}
