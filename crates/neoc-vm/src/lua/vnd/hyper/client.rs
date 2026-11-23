//! `vnd:hyper` client path: the `get`, `post` and `request` entry points over one HTTP/1.1 exchange. The server and
//! request halves live in `super::server` and `super`.
//!
//! `http://` and `https://` both work. TLS is rustls over the `ring` provider: the certificate is checked by the
//! platform verifier (the operating system's trust store). A service with its own CA or a self-signed certificate
//! passes `ca_pem = "<PEM>"` in the options: then exactly those certificates are trusted for every hop of that
//! request.
//!
//! What a request does when the caller sets nothing is decided in NEOC-259, after `URLSession` and Swift's
//! async-http-client: it gives up after 10 s of connecting and after 60 s without data, follows up to 16 redirects
//! (`redirect`), and reads at most 64 MiB of body (`exchange`). Each has an option to change it. A failure is
//! `(nil, "hyper.<fn>: <kind>: <message>")` with a stable kind (`error`).

use std::future::Future;

use mlua::Lua;
use tracing::Instrument;

use crate::lua::facade::args::{Checked, Strict};
use crate::lua::facade::{Bytes, DeclType, Map};
use crate::lua::lifecycle::Life;
use crate::lua::sandbox::{Fallible, from_err, ok};

mod error;
mod exchange;
mod idle;
mod options;
mod redirect;
mod tls;

#[cfg(test)]
mod tests;

pub(super) use error::{kind_of, message_of};
use options::Prepared;
pub(super) use options::{GetOptions, PostOptions, RequestOptions};

/// What a client function hands a script.
#[neoc_macros::lua_record(into)]
pub(super) struct Response {
    status: u16,
    headers: Headers,
    body: Bytes,
    /// The URL the response came from: the last one of the redirect chain.
    url: String,
}

/// The headers of a response: one value under each lower-case name.
pub(super) struct Headers(Vec<(String, String)>);

impl mlua::IntoLua for Headers {
    fn into_lua(self, lua: &Lua) -> mlua::Result<mlua::Value> {
        let headers = lua.create_table()?;
        for (name, value) in self.0 {
            headers.set(name, value)?;
        }
        Ok(mlua::Value::Table(headers))
    }
}

impl DeclType for Headers {
    fn decl_type() -> String {
        Map::<String, String>::decl_type()
    }
}

// Each entry point's options are read in the synchronous part of the call (`Checked`), before it builds the future
// that performs the request, and raw: reading them runs no script code, and the future holds no script table.

/// Perform an HTTP or HTTPS GET request.
#[neoc_macros::lua_fn(doc_with = crate::lua::vnd::hyper::client_doc)]
pub(super) fn get(
    lua: Lua,
    (url, opts): (String, Checked<Option<Strict<GetOptions>>>),
) -> impl Future<Output = mlua::Result<Fallible<Response>>> + Send + 'static {
    let prepared = opts
        .take::<error::HyperError>()
        .and_then(|opts| Prepared::get(url, opts.map(|opts| opts.0)));
    send(lua, "hyper.get", prepared)
}

/// Perform an HTTP or HTTPS POST request given `{ url, body?, ... }`; `body` is a string or a buffer.
#[neoc_macros::lua_fn(doc_with = crate::lua::vnd::hyper::client_doc)]
pub(super) fn post(
    lua: Lua,
    opts: Checked<Strict<PostOptions>>,
) -> impl Future<Output = mlua::Result<Fallible<Response>>> + Send + 'static {
    let prepared = opts
        .take::<error::HyperError>()
        .and_then(|opts| Prepared::post(&lua, opts.0));
    send(lua, "hyper.post", prepared)
}

/// Perform an HTTP or HTTPS request given `{ method, url, body?, ... }`; `body` is a string or a buffer. A standard method in any case is the standard method (`put` is `PUT`); any other is sent as written.
#[neoc_macros::lua_fn(doc_with = crate::lua::vnd::hyper::client_doc)]
pub(super) fn request(
    lua: Lua,
    opts: Checked<Strict<RequestOptions>>,
) -> impl Future<Output = mlua::Result<Fallible<Response>>> + Send + 'static {
    let prepared = opts
        .take::<error::HyperError>()
        .and_then(|opts| Prepared::request(&lua, opts.0));
    send(lua, "hyper.request", prepared)
}

/// The future of a prepared request, or the `(nil, err)` of options that could not be read. Holds no script table.
/// It runs inside a span named after the entry point (`hyper.get`, `hyper.post`, `hyper.request`: a span name is a
/// literal, so each is written out), at target `lua::lib::hyper`, as the `#[instrument]` it replaces opened.
fn send(
    lua: Lua,
    prefix: &'static str,
    prepared: Result<Prepared, error::HyperError>,
) -> impl Future<Output = mlua::Result<Fallible<Response>>> + Send + 'static {
    let span = match prefix {
        "hyper.get" => tracing::info_span!(target: "lua::lib::hyper", "hyper.get"),
        "hyper.post" => tracing::info_span!(target: "lua::lib::hyper", "hyper.post"),
        _ => tracing::info_span!(target: "lua::lib::hyper", "hyper.request"),
    };
    // taken here, not in the future: the connections the request opens are registered with the engine's `Life`
    let life = Life::of(&lua);
    async move {
        let performed = match prepared {
            Ok(prepared) => exchange::run(prepared, life).await,
            Err(e) => Err(e),
        };
        match performed {
            Ok(resp) => ok(Response {
                status: resp.status,
                headers: Headers(resp.headers),
                body: Bytes(resp.body),
                url: resp.url,
            }),
            Err(e) => from_err(prefix, e),
        }
    }
    .instrument(span)
}
