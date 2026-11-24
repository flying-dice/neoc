//! HTTP client and server: `vnd:hyper`, backed by hyper.
//!
//! ## Client
//! `http://` and `https://` (rustls; see `client`). Client functions are async on the Lua side: a call yields the
//! calling coroutine until the response is ready, so other coroutines on the same engine (a request handler
//! installed with `serve`, say) keep running meanwhile.
//!
//! ```lua
//! local hyper = require("vnd:hyper")
//! local resp, err = hyper.get("http://example.com/")
//! if not resp then
//!     if err.kind == "timed_out" then ... end
//!     return
//! end
//! print(resp.status, resp.headers["content-type"], resp.body, resp.url)
//! ```
//!
//! What a request does when the options say nothing (NEOC-259, after `URLSession` and Swift's async-http-client):
//!
//! - **It does not hang.** It gives up after 10 s of connecting (TCP and the TLS handshake) and after 60 s
//!   without data in either direction, reset whenever data moves (`connect_timeout`, `idle_timeout`). A server
//!   that answers one byte every 59 s is never idle: `timeout` bounds the whole request, redirects and body
//!   included, and has no default.
//! - **It follows redirects**, 16 at most, with the rewriting and cross-origin rules of async-http-client
//!   (`follow_redirects`, `max_redirects`; see `client::redirect`). Credentials do not follow a redirect to another
//!   origin. Nothing else is checked about where a redirect goes: it may lead from `https` to `http`, to a host on
//!   the local network or to the machine itself, because neoc has no permissions; an app that fetches a URL
//!   someone else chose and must not reach those checks `resp.url` against its own rule, or sets
//!   `follow_redirects = false` and follows the `Location` itself.
//! - **It does not fill the memory.** It reads at most 64 MiB of body (`max_body`) and fails with `too_large`
//!   past that, a declared length first. It never decompresses: it sends no `Accept-Encoding` and returns the bytes
//!   as they came, so the cap is on what was read from the socket. A script that decodes a body (`lib:compress`)
//!   bounds the decoded size itself.
//! - **It says what failed**: a typed error whose `kind` is stable across platforms.
//!
//! ## Server
//! `serve(listener, handler)` mirrors hyper's `service_fn` model: an accept loop takes connections from the
//! listener (built by `(require("std:net")).TcpListener.bind`) and every request is dispatched to a single Lua
//! function. There is no built-in routing: the handler reads `req.method` / `req.path` and decides what to do,
//! exactly like the hyper Rust example.
//!
//! ```lua
//! local net   = require("std:net")
//! local hyper = require("vnd:hyper")
//!
//! local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
//! local server = assert(hyper.serve(listener, function(req)
//!     if req.method == "GET" and req.path == "/" then
//!         return { status = 200, body = "hello" }
//!     end
//!     return { status = 404, body = "not found" }
//! end))
//! print("listening on " .. assert(server:addr()))
//! server:wait()
//! ```
//!
//! For multi-worker fan-out, share the listener with `std:thread` workers: every worker serves on the same socket
//! and races on `accept()`.
//!
//! Ported from the old VM's `vnd:hyper`. What changed on purpose:
//! - A failed request is a typed `HyperError` (`err.kind`, and what the kind carries: `limit` for `timed_out`,
//!   `max_redirects`, `max_body`, `reason`); `error_kind` and `error_message` are gone.
//! - `post(url, body?, opts?)` and `request(method, url, body?, opts?)` take the URL, the method and the body as
//!   arguments (they were keys of one table), so all three functions take the same options.
//! - An option that is misspelt or of the wrong type is refused as a wrong argument, with its place (it was a
//!   returned `invalid_input`). What a type cannot say (a timeout of 0, PEM without a certificate, a bad header
//!   name) still is `invalid_input`.
//! - `follow_redirects` is a boolean; the limit is `max_redirects` (one option held either).
//! - A body is a string. A buffer or a number is refused (`buffer.tostring`, `tostring`), and so is a header value
//!   that is a number.
//! - A handler answers with a string, a `Reply` table (`status`, `headers`, `body`, and no other key) or nothing.
//! - `serve` returns a typed `ServerError`, and `Server:addr()` returns `(nil, err)` once the server is closed (it
//!   raised). The server type is `Server` (it was `ServerHandle`).
//! - `Request.json` is `lib:json`'s typed node (an `Object`, an `Array` or a scalar).

use std::collections::BTreeMap;

use mlua_ir::ir::Ty;
use mlua_ir::{Registry, Shape};

pub mod client;
mod server;

/// The bytes of a body as a field of a record: a Lua string, whatever its bytes (a `String` field would refuse
/// what is not UTF-8). Declared `string`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Body(pub Vec<u8>);

impl Shape for Body {
    fn shape(_: &mut Registry) -> Ty {
        Ty::String
    }
}

impl serde::Serialize for Body {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Body {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl serde::de::Visitor<'_> for BodyVisitor {
            type Value = Body;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Body, E> {
                Ok(Body(text.as_bytes().to_vec()))
            }

            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Body, E> {
                Ok(Body(bytes.to_vec()))
            }
        }

        deserializer.deserialize_any(BodyVisitor)
    }
}

/// A request's or a response's `HeaderMap` as the Lua surface has it: one entry per name (lower case). A header
/// sent more than once is one value, the values joined with `", "` in the order they came, as the Fetch standard's
/// `Headers.get` does, except `set-cookie`, whose values are joined with `"\n"`: a cookie may hold a comma
/// (`Expires`), a field value never holds a line feed, so splitting on `"\n"` gives each cookie back. A value that
/// is not UTF-8 is decoded with replacement characters. Nothing is dropped. Shared by the server and the client.
pub(crate) fn collect_headers(headers: &::hyper::HeaderMap) -> Vec<(String, String)> {
    headers
        .keys()
        .map(|name| {
            let values: Vec<_> =
                headers.get_all(name).iter().map(|v| String::from_utf8_lossy(v.as_bytes())).collect();
            let separator = if name == ::hyper::header::SET_COOKIE { "\n" } else { ", " };
            (name.as_str().to_owned(), values.join(separator))
        })
        .collect()
}

/// What a request a handler is given holds. Nothing is made a Lua value until the handler asks for it.
struct RequestParts {
    method: String,
    path: String,
    /// Lower-cased header names with their values, in the order they came. A list, not a map: the handful of
    /// headers a request has is faster to scan than to hash.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    body_is_json: bool,
}

impl RequestParts {
    fn new(method: String, path: String, raw_headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        let mut headers: Vec<(String, String)> = Vec::with_capacity(raw_headers.len());
        let mut body_is_json = false;
        for (name, value) in raw_headers {
            let name = name.to_ascii_lowercase();
            if name == "content-type" && value.to_ascii_lowercase().starts_with("application/json") {
                body_is_json = true;
            }
            headers.push((name, value));
        }
        Self { method, path, headers, body, body_is_json }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn headers(&self) -> BTreeMap<String, String> {
        self.headers.iter().cloned().collect()
    }
}

#[mlua_ir::module("vnd:hyper")]
pub mod hyper {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{Bytes, Callback, LuaError, Object, OneOf, Shape};
    use serde::{Deserialize, Serialize};

    use super::client::{self, HyperError, RequestOptions};
    use super::{Body, RequestParts, server};
    use crate::lifecycle::Life;
    use crate::modules::lib::json::json::Node;
    use crate::modules::std::net::net::TcpListener;

    /// What a client function hands a script: the last response of the redirect chain, its body read in full. A
    /// 4xx or 5xx status is a response, not an error.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct Response {
        /// The status code.
        pub status: u16,
        /// One value under each lower-case name. A header sent more than once is joined with `", "`, `set-cookie`
        /// with `"\n"` so each cookie can be split back out, and bytes that are not UTF-8 become U+FFFD.
        pub headers: BTreeMap<String, String>,
        /// The body, as the bytes that came: never decompressed.
        pub body: Body,
        /// The URL the response came from: the last one of the redirect chain.
        pub url: String,
    }

    impl From<client::Response> for Response {
        fn from(response: client::Response) -> Self {
            Self {
                status: response.status,
                headers: response.headers.into_iter().collect(),
                body: Body(response.body),
                url: response.url,
            }
        }
    }

    /// Perform an HTTP or HTTPS GET request.
    ///
    /// ```luau
    /// local resp, err = hyper.get("http://127.0.0.1:1/", { connect_timeout = 2, headers = { accept = "text/plain" } })
    /// if resp == nil then
    ///     assert(err ~= nil and (err.kind == "connect" or err.kind == "timed_out"))
    /// end
    /// ```
    #[host("Wait")]
    pub async fn get(lua: Lua, url: String, opts: RequestOptions) -> Result<Response, HyperError> {
        client::send(&lua, "GET", url, None, opts).await.map(Response::from)
    }

    /// Perform an HTTP or HTTPS POST request with `body`. With no body it says `Content-Length: 0`.
    ///
    /// ```luau
    /// local resp, err = hyper.post("http://127.0.0.1:1/items", '{"name":"ada"}', {
    ///     headers = { ["content-type"] = "application/json" },
    /// })
    /// ```
    #[host("Wait")]
    pub async fn post(
        lua: Lua,
        url: String,
        body: Option<Bytes>,
        opts: RequestOptions,
    ) -> Result<Response, HyperError> {
        client::send(&lua, "POST", url, body.map(|body| body.0), opts).await.map(Response::from)
    }

    /// Perform an HTTP or HTTPS request with `method`. A standard method in any case is the standard method (`put`
    /// is `PUT`); any other is sent as written. A method that is not a valid method name is `invalid_input`.
    ///
    /// ```luau
    /// local resp, err = hyper.request("DELETE", "http://127.0.0.1:1/items/1", nil, { timeout = 5 })
    /// ```
    #[host("Wait")]
    pub async fn request(
        lua: Lua,
        method: String,
        url: String,
        body: Option<Bytes>,
        opts: RequestOptions,
    ) -> Result<Response, HyperError> {
        client::send(&lua, &method, url, body.map(|body| body.0), opts).await.map(Response::from)
    }

    /// A request a server's handler is given.
    #[derive(Clone, Object)]
    pub struct Request {
        parts: Arc<RequestParts>,
    }

    impl Request {
        /// A request of `method` to `path` with these headers (any case) and this body.
        #[must_use]
        pub(crate) fn from_parts(
            method: String,
            path: String,
            headers: Vec<(String, String)>,
            body: Vec<u8>,
        ) -> Self {
            Self { parts: Arc::new(RequestParts::new(method, path, headers, body)) }
        }
    }

    #[mlua_ir::object]
    impl Request {
        /// The HTTP method (`GET`).
        #[get]
        pub fn method(&self) -> String {
            self.parts.method.clone()
        }

        /// The request path, without the query string.
        #[get]
        pub fn path(&self) -> String {
            self.parts.path.clone()
        }

        /// The raw request body.
        #[get]
        pub fn body(&self) -> Bytes {
            Bytes(self.parts.body.clone())
        }

        /// Whether the `Content-Type` header is `application/json`.
        #[get]
        pub fn is_json(&self) -> bool {
            self.parts.body_is_json
        }

        /// The request headers by name, names in lower case. A header sent more than once is one value joined with
        /// `", "` (`set-cookie` with `"\n"`), and a value that is not UTF-8 has its bad bytes replaced with U+FFFD.
        /// The table is built on every read: a handler that reads several binds it once.
        #[get]
        pub fn headers(&self) -> BTreeMap<String, String> {
            self.parts.headers()
        }

        /// The body parsed as JSON: a `lib:json` handle for an object or an array, a Lua value for a string, a
        /// number or a boolean. `nil` when the body is empty, is not JSON, or is `null`. Parsed on every read.
        #[get]
        pub fn json(&self) -> Option<Node> {
            if self.parts.body.is_empty() {
                return None;
            }
            serde_json::from_slice(&self.parts.body).ok().and_then(Node::root)
        }

        /// A request header by name, in any case; `nil` when the request has none.
        pub fn header(&self, name: String) -> Option<String> {
            self.parts.header(&name).map(str::to_owned)
        }
    }

    /// A handler's answer, when it is more than a body.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct Reply {
        /// The status code; 200 when not given.
        pub status: Option<u16>,
        /// Headers to send, by name.
        pub headers: Option<BTreeMap<String, String>>,
        /// The body; empty when not given.
        pub body: Option<Body>,
    }

    /// What a handler returns: a body (a 200 with it), or a `Reply`.
    #[derive(Debug, Clone, OneOf)]
    pub enum Answer {
        /// The body of a 200 response.
        Body(Bytes),
        /// The status, the headers and the body.
        Reply(Reply),
    }

    /// Why a server could not be started, or has no address to give.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ServerError {
        /// The listener is closed: its engine shut down, or stopped its background work.
        #[error("the listener is closed")]
        ListenerClosed,
        /// The server is closed: `close()` was called, or its engine shut down.
        #[error("the server is closed")]
        Closed,
        /// Any other failure.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    /// A running server. Copies of the handle are the same server.
    #[derive(Clone, Object)]
    pub struct Server {
        server: Arc<server::Server>,
    }

    #[mlua_ir::object]
    impl Server {
        /// The local address the server accepts on, as `"127.0.0.1:8080"`.
        pub fn addr(&self) -> Result<String, ServerError> {
            self.server.addr().map(|addr| addr.to_string()).ok_or(ServerError::Closed)
        }

        /// Ask the server to stop accepting connections; returns at once. `wait()` returns when it has.
        pub fn close(&self) {
            self.server.close();
        }

        /// Wait until the server stops accepting connections: `close()` was called, the process was asked to stop
        /// (Ctrl-C), or an embedder shut the script down. The handlers keep running meanwhile. At once when the
        /// server has already stopped.
        #[host("Mixed")]
        pub async fn wait(&self, lua: Lua) -> mlua::Result<()> {
            self.server.wait(Life::of(&lua)).await
        }
    }

    /// Start accepting HTTP/1.1 connections on `listener` and call `handler` for every request. The handler
    /// returns a body (a 200 with it), a `Reply`, or nothing (an empty 200); it may wait (call the client, sleep)
    /// and other requests are served meanwhile. A handler that raises, or returns anything else, answers 500
    /// `handler error`, and the server serves on.
    ///
    /// ```luau
    /// local net = require("std:net")
    /// local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
    /// local server = assert(hyper.serve(listener, function(req)
    ///     if req.method == "GET" and req.path == "/" then
    ///         return "hello"
    ///     end
    ///     return { status = 404, body = "not found" }
    /// end))
    /// local resp = assert(hyper.get("http://" .. assert(server:addr()) .. "/"))
    /// assert(resp.status == 200 and resp.body == "hello")
    /// server:close()
    /// server:wait()
    /// ```
    pub fn serve(
        lua: &Lua,
        listener: TcpListener,
        handler: Callback<fn(req: Request) -> Option<Answer>>,
    ) -> Result<Server, ServerError> {
        // the handler is run by the server, not through the callback: it may wait, as a call of its own
        server::Server::start(lua, listener.slot(), handler.into_function()).map(|server| Server { server })
    }
}

#[cfg(test)]
mod tests;
