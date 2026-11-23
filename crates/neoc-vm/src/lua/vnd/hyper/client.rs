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

use mlua::{Lua, Table};
use tracing::Instrument;

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

// Each entry point reads its options table in the synchronous part of the call, before it builds the future that
// performs the request. A script table can run script code when it is read (a `__index`), which can call the same
// entry point again: read inside the future, every level would stack another polled future on the C stack (about
// 20 KiB a level in release, about 150 KiB in debug, with Luau's C-call limit at 200 levels), and read here each
// level is a few hundred bytes. `thread.spawn` reads its options the same way (`prepare_spawn`).

pub(super) fn get(
    lua: Lua,
    (url, opts): (String, Option<Table>),
) -> impl Future<Output = mlua::Result<Fallible<Table>>> + Send + 'static {
    send(lua, "hyper.get", Prepared::get(url, opts.as_ref()))
}

pub(super) fn post(
    lua: Lua,
    opts: Table,
) -> impl Future<Output = mlua::Result<Fallible<Table>>> + Send + 'static {
    send(lua, "hyper.post", Prepared::post(&opts))
}

pub(super) fn request(
    lua: Lua,
    opts: Table,
) -> impl Future<Output = mlua::Result<Fallible<Table>>> + Send + 'static {
    send(lua, "hyper.request", Prepared::request(&opts))
}

/// The future of a prepared request, or the `(nil, err)` of options that could not be read. Holds no script table.
/// It runs inside a span named after the entry point (`hyper.get`, `hyper.post`, `hyper.request`: a span name is a
/// literal, so each is written out), at target `lua::lib::hyper`, as the `#[instrument]` it replaces opened.
fn send(
    lua: Lua,
    prefix: &'static str,
    prepared: Result<Prepared, error::HyperError>,
) -> impl Future<Output = mlua::Result<Fallible<Table>>> + Send + 'static {
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
            Ok(resp) => {
                let t = lua.create_table()?;
                t.set("status", i64::from(resp.status))?;
                let h = lua.create_table()?;
                for (k, v) in resp.headers {
                    h.set(k, v)?;
                }
                t.set("headers", h)?;
                t.set("body", lua.create_string(&resp.body)?)?;
                t.set("url", resp.url)?;
                ok(t)
            }
            Err(e) => from_err(prefix, e),
        }
    }
    .instrument(span)
}
