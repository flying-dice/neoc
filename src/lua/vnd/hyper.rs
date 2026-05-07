//! HTTP client and server — `vnd:hyper`, backed by hyper.
//!
//! ## Client
//! HTTP-only (no TLS). URLs must be `http://...`. Client functions are
//! async on the Lua side: calls yield the calling coroutine until the
//! response is ready, so other coroutines on the same engine (e.g. a
//! request handler installed via `serve`) keep running concurrently.
//!
//! ```lua
//! local hyper = require("vnd:hyper")
//! local resp, err = hyper.get("http://example.com/")
//! resp.status; resp.headers; resp.body
//! ```
//!
//! ## Server
//! `serve(listener, handler)` mirrors hyper's `service_fn` model: the
//! listener (built by `(require("std:net")).listener`) is consumed by an
//! accept loop and every request is dispatched to a single Lua function.
//! There is no built-in routing — the handler reads `req.method` /
//! `req.path` and decides what to do, exactly like the hyper Rust example.
//!
//! ```lua
//! local net   = require("std:net")
//! local hyper = require("vnd:hyper")
//!
//! local listener = net.listener("127.0.0.1:0")
//! local server = hyper.serve(listener, function(req)
//!     if req.method == "GET" and req.path == "/" then
//!         return { status = 200, body = "hello" }
//!     end
//!     return { status = 404, body = "not found" }
//! end)
//! print("listening on " .. server:addr())
//! server:wait()
//! ```
//!
//! For multi-worker fan-out, wrap the listener in
//! `(require("std:workers")).shared` — every worker attaches to the same
//! fd and races on `accept()`.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request as HyperRequest, Response, Uri};
use hyper_util::rt::TokioIo;
use mlua::{Function, Lua, RegistryKey, Table, UserData, UserDataMethods, Value};
use parking_lot::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::net::Listener;
use crate::lua::std::workers::ShutdownSignal;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(
        "get",
        lua.create_async_function(|lua, url: String| get(lua, url))?,
    )?;
    t.set(
        "post",
        lua.create_async_function(|lua, opts: Table| post(lua, opts))?,
    )?;
    t.set(
        "request",
        lua.create_async_function(|lua, opts: Table| request(lua, opts))?,
    )?;
    t.set("serve", lua.create_function(serve)?)?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// Client — Lua-bound entry points (async)
// ---------------------------------------------------------------------------

#[instrument(name = "hyper.get", target = "luau::lib::hyper", skip_all)]
async fn get(lua: Lua, url: String) -> mlua::Result<(Value, Value)> {
    perform(&lua, "hyper.get", "GET".into(), url, None, Vec::new()).await
}

#[instrument(name = "hyper.post", target = "luau::lib::hyper", skip_all)]
async fn post(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let url: String = match opts.get("url") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.post", e),
    };
    let body = body_from_opts(&opts);
    let headers = match headers_from_opts(&opts) {
        Ok(h) => h,
        Err(e) => return from_err(&lua, "hyper.post", e),
    };
    perform(&lua, "hyper.post", "POST".into(), url, body, headers).await
}

#[instrument(name = "hyper.request", target = "luau::lib::hyper", skip_all)]
async fn request(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let method: String = match opts.get("method") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    let url: String = match opts.get("url") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    let body = body_from_opts(&opts);
    let headers = match headers_from_opts(&opts) {
        Ok(h) => h,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    perform(&lua, "hyper.request", method, url, body, headers).await
}

// ---------------------------------------------------------------------------
// Lua → Rust extraction
// ---------------------------------------------------------------------------

fn body_from_opts(opts: &Table) -> Option<Vec<u8>> {
    let s: Option<mlua::String> = opts.get("body").ok();
    s.map(|v| v.as_bytes().to_vec())
}

fn headers_from_opts(opts: &Table) -> mlua::Result<Vec<(String, String)>> {
    let h: Option<Table> = opts.get("headers").ok();
    match h {
        None => Ok(Vec::new()),
        Some(t) => t.pairs::<String, String>().collect(),
    }
}

// ---------------------------------------------------------------------------
// Client — hyper request
// ---------------------------------------------------------------------------

struct ClientResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn perform(
    lua: &Lua,
    prefix: &str,
    method: String,
    url: String,
    body: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
) -> mlua::Result<(Value, Value)> {
    match do_request(method, url, body, headers).await {
        Ok(resp) => {
            let t = lua.create_table()?;
            t.set("status", resp.status as i64)?;
            let h = lua.create_table()?;
            for (k, v) in resp.headers {
                h.set(k, v)?;
            }
            t.set("headers", h)?;
            t.set("body", lua.create_string(&resp.body)?)?;
            ok(lua, t)
        }
        Err(e) => from_err(lua, prefix, e),
    }
}

async fn do_request(
    method: String,
    url: String,
    body: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
) -> Result<ClientResponse, String> {
    let uri: Uri = url
        .parse()
        .map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;

    if uri.scheme_str() != Some("http") {
        return Err("only http:// URLs are supported".into());
    }

    let host = uri.host().ok_or_else(|| "URL has no host".to_string())?;
    let port = uri.port_u16().unwrap_or(80);
    let authority = format!("{host}:{port}");

    let stream = TcpStream::connect(&authority)
        .await
        .map_err(|e| e.to_string())?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");

    let mut builder = HyperRequest::builder()
        .method(method.as_str())
        .uri(path_and_query)
        .header("host", authority.as_str());

    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    let bytes = body.map(Bytes::from).unwrap_or_default();
    let req = builder.body(Full::new(bytes)).map_err(|e| e.to_string())?;

    let resp = sender.send_request(req).await.map_err(|e| e.to_string())?;

    let status = resp.status().as_u16();
    let resp_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();

    let collected = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| e.to_string())?;
    let body = collected.to_bytes().to_vec();

    Ok(ClientResponse {
        status,
        headers: resp_headers,
        body,
    })
}

// ---------------------------------------------------------------------------
// Server — handle and state machine
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ServerHandle {
    inner: Arc<ServerInner>,
}

struct ServerInner {
    /// The single Lua function registered as the service_fn equivalent.
    /// Stashed in the Lua registry so we can pull it back out for each
    /// request without leaking it through Rust types.
    handler: Arc<RegistryKey>,
    state: Mutex<ServerState>,
}

enum ServerState {
    /// Listener attached, accept loop not yet running.
    Idle {
        listener: Arc<TcpListener>,
        addr: SocketAddr,
    },
    /// `serve()` has run; accept loop is live.
    Listening {
        addr: SocketAddr,
        shutdown: Option<oneshot::Sender<()>>,
        done: tokio::sync::watch::Receiver<bool>,
    },
    /// `:close()` was called or the server task exited.
    Closed,
}

impl ServerHandle {
    fn from_listener(listener: Arc<TcpListener>, handler: Arc<RegistryKey>) -> mlua::Result<Self> {
        let addr = listener.local_addr().map_err(mlua::Error::external)?;
        Ok(Self {
            inner: Arc::new(ServerInner {
                handler,
                state: Mutex::new(ServerState::Idle { listener, addr }),
            }),
        })
    }

    /// Spawn the accept loop on the current tokio runtime. Called once
    /// from `serve`; not exposed to Lua because the only way to obtain a
    /// `ServerHandle` is via `serve`, which always starts it.
    fn start(&self, lua: &Lua) -> mlua::Result<()> {
        let mut state = self.inner.state.lock();
        let (listener, addr) = match std::mem::replace(&mut *state, ServerState::Closed) {
            ServerState::Idle { listener, addr } => (listener, addr),
            ServerState::Listening {
                addr,
                shutdown,
                done,
            } => {
                *state = ServerState::Listening {
                    addr,
                    shutdown,
                    done,
                };
                return Err(mlua::Error::external(
                    "hyper.serve: server already listening",
                ));
            }
            ServerState::Closed => {
                return Err(mlua::Error::external("hyper.serve: server is closed"));
            }
        };

        let handle = Handle::try_current()
            .map_err(|_| mlua::Error::external("hyper.serve: no tokio runtime in scope"))?;

        let (tx, rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = tokio::sync::watch::channel(false);
        let inner = self.inner.clone();
        let lua_handle: Lua = lua.clone();
        handle.spawn(run_server(lua_handle, listener, inner, rx, done_tx));

        *state = ServerState::Listening {
            addr,
            shutdown: Some(tx),
            done: done_rx,
        };
        Ok(())
    }
}

impl UserData for ServerHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("addr", |_, this, _: ()| {
            let state = this.inner.state.lock();
            match &*state {
                ServerState::Idle { addr, .. } => Ok(addr.to_string()),
                ServerState::Listening { addr, .. } => Ok(addr.to_string()),
                ServerState::Closed => Err(mlua::Error::external("hyper.addr: server is closed")),
            }
        });

        methods.add_method("close", |_, this, _: ()| {
            let mut state = this.inner.state.lock();
            if let ServerState::Listening { shutdown, .. } = &mut *state
                && let Some(tx) = shutdown.take()
            {
                let _ = tx.send(());
            }
            Ok(())
        });

        // Block until the server stops accepting connections — either because
        // close() was called, or because the process-wide shutdown signal
        // fires (CLI installs a Ctrl-C handler that calls
        // `workers.shutdown_signal():fire()`). On signal, wait() triggers a
        // local graceful shutdown so the run_server loop drains cleanly.
        methods.add_async_method("wait", |_, this, _: ()| {
            let inner = this.inner.clone();
            async move {
                let mut done = {
                    let state = inner.state.lock();
                    match &*state {
                        ServerState::Listening { done, .. } => done.clone(),
                        ServerState::Closed => return Ok(()),
                        _ => {
                            return Err(mlua::Error::external(
                                "hyper.wait: server is not listening",
                            ));
                        }
                    }
                };
                if *done.borrow() {
                    return Ok(());
                }
                let signal = ShutdownSignal::current();
                tokio::select! {
                    _ = done.changed() => {},
                    _ = signal.wait() => {
                        // Drop the lock before awaiting — clippy
                        // (correctly) flags lock guards held across awaits.
                        {
                            let mut state = inner.state.lock();
                            if let ServerState::Listening { shutdown, .. } = &mut *state
                                && let Some(tx) = shutdown.take()
                            {
                                let _ = tx.send(());
                            }
                        }
                        let _ = done.changed().await;
                    }
                }
                Ok(())
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Server — `lib.hyper.serve`
// ---------------------------------------------------------------------------

#[instrument(name = "hyper.serve", target = "luau::lib::hyper", skip_all)]
fn serve(
    lua: &Lua,
    (listener, handler): (mlua::AnyUserData, Function),
) -> mlua::Result<(Value, Value)> {
    let arc = match listener.borrow::<Listener>() {
        Ok(h) => h.arc(),
        Err(e) => return from_err(lua, "hyper.serve", format!("expected TcpListener: {e}")),
    };
    let key = match lua.create_registry_value(handler) {
        Ok(k) => k,
        Err(e) => return from_err(lua, "hyper.serve", e),
    };
    let server = match ServerHandle::from_listener(arc, Arc::new(key)) {
        Ok(s) => s,
        Err(e) => return from_err(lua, "hyper.serve", e),
    };
    if let Err(e) = server.start(lua) {
        return from_err(lua, "hyper.serve", e);
    }
    ok(lua, server)
}

// ---------------------------------------------------------------------------
// Server — accept + dispatch
// ---------------------------------------------------------------------------

async fn run_server(
    lua: Lua,
    listener: Arc<TcpListener>,
    inner: Arc<ServerInner>,
    mut shutdown_rx: oneshot::Receiver<()>,
    done_tx: tokio::sync::watch::Sender<bool>,
) {
    let signal = ShutdownSignal::current();
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            _ = signal.wait() => break,
            accept = listener.accept() => {
                let stream = match accept {
                    Ok((s, _)) => s,
                    Err(_) => continue,
                };
                let inner = inner.clone();
                let lua = lua.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req: HyperRequest<Incoming>| {
                        let inner = inner.clone();
                        let lua = lua.clone();
                        async move { handle_request(lua, inner, req).await }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        }
    }
    let _ = done_tx.send(true);
    *inner.state.lock() = ServerState::Closed;
}

async fn handle_request(
    lua: Lua,
    inner: Arc<ServerInner>,
    req: HyperRequest<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    invoke_lua_handler(&lua, &inner.handler, method, path, req).await
}

// ---------------------------------------------------------------------------
// Request — userdata handle for an in-flight HTTP request
// ---------------------------------------------------------------------------
//
// Field access for immediate values, methods only where an argument is
// needed. Nothing materialises in Lua until the script asks for it, and
// what comes back is always the most natural type for the question:
//
//   req.method          → "POST"               (Lua string — cheap)
//   req.path            → "/users/42"          (Lua string)
//   req:header("ct")    → "application/json"   (Lua string, or nil; case-insensitive)
//   req.headers         → Object               (boxed; lazy, cached)
//   req.body            → "<raw bytes>"        (Lua string)
//   req.json            → lib:json Object/Array userdata (or scalar Lua native)
//   req.is_json         → bool                 (cheap content-type check)
//
// `.json` and `.headers` materialise plain Lua values on access. If a
// handler reads them in a loop, bind once: `local h = req.headers`. A
// handler that never touches them pays nothing beyond holding the raw
// bytes in `req.body`.

struct RequestInner {
    method: String,
    path: String,
    /// Lower-cased header names paired with original values. Vec rather
    /// than HashMap because typical request header counts (5–15) are
    /// faster to scan than to hash-lookup, and we keep insertion order
    /// for `:headers()` output.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    body_is_json: bool,
}

pub struct Request {
    inner: Arc<RequestInner>,
}

impl Request {
    fn from_parts(
        method: String,
        path: String,
        raw_headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Self {
        let mut headers: Vec<(String, String)> = Vec::with_capacity(raw_headers.len());
        let mut body_is_json = false;
        for (k, v) in raw_headers {
            let lk = k.to_ascii_lowercase();
            if lk == "content-type" && v.to_ascii_lowercase().starts_with("application/json") {
                body_is_json = true;
            }
            headers.push((lk, v));
        }
        Self {
            inner: Arc::new(RequestInner {
                method,
                path,
                headers,
                body,
                body_is_json,
            }),
        }
    }

    fn header_lookup(&self, name: &str) -> Option<&str> {
        for (k, v) in &self.inner.headers {
            if k.eq_ignore_ascii_case(name) {
                return Some(v.as_str());
            }
        }
        None
    }
}

impl UserData for Request {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method", |lua, this| {
            Ok(Value::String(lua.create_string(&this.inner.method)?))
        });
        fields.add_field_method_get("path", |lua, this| {
            Ok(Value::String(lua.create_string(&this.inner.path)?))
        });
        fields.add_field_method_get("body", |lua, this| {
            Ok(Value::String(lua.create_string(&this.inner.body)?))
        });
        fields.add_field_method_get("is_json", |_, this| Ok(this.inner.body_is_json));

        fields.add_field_method_get("headers", |lua, this| {
            let t = lua.create_table_with_capacity(0, this.inner.headers.len())?;
            for (k, v) in &this.inner.headers {
                t.set(k.as_str(), v.as_str())?;
            }
            Ok(Value::Table(t))
        });
        fields.add_field_method_get("json", |lua, this| {
            if this.inner.body.is_empty() {
                return Ok(Value::Nil);
            }
            // Reuse the bridge in vnd:serde_json so all body→handle paths
            // go through the same conversion code.
            crate::lua::vnd::serde_json::parse_to_handle(lua, &this.inner.body).or(Ok(Value::Nil))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("header", |lua, this, name: String| {
            match this.header_lookup(&name) {
                Some(v) => Ok(Value::String(lua.create_string(v)?)),
                None => Ok(Value::Nil),
            }
        });
    }
}

async fn invoke_lua_handler(
    lua: &Lua,
    handler_key: &RegistryKey,
    method: String,
    path: String,
    req: HyperRequest<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let req_headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();
    let body = match req.into_body().collect().await {
        Ok(c) => c.to_bytes().to_vec(),
        Err(_) => return Ok(error_response(500, "body read failed")),
    };

    let result: mlua::Result<HandlerResponse> = async {
        let req_ud = self::Request::from_parts(method, path, req_headers, body);
        let handler: Function = lua.registry_value(handler_key)?;
        let raw: Value = handler.call_async(req_ud).await?;
        decode_handler_response(raw)
    }
    .await;

    match result {
        Ok(resp) => {
            let mut builder = Response::builder().status(resp.status);
            for (k, v) in &resp.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            Ok(builder
                .body(Full::new(Bytes::from(resp.body)))
                .unwrap_or_else(|_| error_response(500, "build failed")))
        }
        Err(e) => {
            tracing::error!(target: "luau::lib::hyper", "handler error: {e}");
            Ok(error_response(500, "handler error"))
        }
    }
}

struct HandlerResponse {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

/// Handlers may return either a plain string (treated as a 200 body) or a
/// table `{status, body, headers}`. Anything else is an error.
fn decode_handler_response(v: Value) -> mlua::Result<HandlerResponse> {
    match v {
        Value::String(s) => Ok(HandlerResponse {
            status: 200,
            body: s.as_bytes().to_vec(),
            headers: Vec::new(),
        }),
        Value::Table(t) => {
            let status: u16 = t.get::<Option<u16>>("status")?.unwrap_or(200);
            let body: Vec<u8> = t
                .get::<Option<mlua::String>>("body")?
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default();
            let headers: Vec<(String, String)> = match t.get::<Option<Table>>("headers")? {
                Some(t) => t
                    .pairs::<String, String>()
                    .collect::<mlua::Result<Vec<_>>>()?,
                None => Vec::new(),
            };
            Ok(HandlerResponse {
                status,
                body,
                headers,
            })
        }
        Value::Nil => Ok(HandlerResponse {
            status: 200,
            body: Vec::new(),
            headers: Vec::new(),
        }),
        other => Err(mlua::Error::external(format!(
            "handler returned {} — expected string or table",
            other.type_name()
        ))),
    }
}

fn error_response(status: u16, msg: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::from_static(msg.as_bytes())))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Benchmarks — hot-path regression matrix.
//
// Three tiers, plus a per-field matrix on the through_lua tier so that
// regressions in eager-materialisation (body/headers/json) show up
// distinct from baseline boundary cost:
//
//   library           — pure hyper service_fn round trip, no mlua.
//   into_out_lua      — Lua handler invoked with a manually-built Request
//                       userdata; no TCP. Pure mlua dispatch cost per req.
//   through_lua/*     — full HTTP round trip with a Lua handler that
//                       touches different parts of the request (no_touch,
//                       path, method, header, body, headers, json). The
//                       contrast between `no_touch_*` and the rest is the
//                       regression signal: if `no_touch_post` ever creeps
//                       toward `body_post`, somebody started eagerly
//                       materialising the body in `Request::from_parts`.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};
    use tokio::runtime::Runtime;

    const BODY: &[u8] = br#"{"id":42,"name":"Alice","email":"alice@example.com","tags":["admin","ops","oncall","gold","silver","bronze"],"meta":{"lang":"en","tz":"UTC","prefs":{"theme":"dark","compact":true,"density":"comfortable"}},"items":[{"sku":"A1","qty":3},{"sku":"B2","qty":1},{"sku":"C3","qty":7},{"sku":"D4","qty":2},{"sku":"E5","qty":5},{"sku":"F6","qty":9},{"sku":"G7","qty":4},{"sku":"H8","qty":6},{"sku":"I9","qty":8},{"sku":"J0","qty":10}],"notes":"Padding so the body has realistic non-trivial size: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. Padding-2: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-3: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-4: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-5: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-6: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-7: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-8: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-9: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-10: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-11: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Padding-12: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."}"#;

    let rt = Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();

    // -- library: pure-Rust hyper service_fn server, no mlua --------------
    let rust_addr = bench_helpers::spawn_rust_baseline(&rt).to_string();

    // -- through_lua: many Lua handlers, one listener each ---------------
    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let lua_no_touch_addr =
        bench_helpers::spawn_lua_server(&lua, r#"function(req) return "hi" end"#);
    let lua_path_addr =
        bench_helpers::spawn_lua_server(&lua, r#"function(req) return req.path end"#);
    let lua_method_addr =
        bench_helpers::spawn_lua_server(&lua, r#"function(req) return req.method end"#);
    let lua_header_addr = bench_helpers::spawn_lua_server(
        &lua,
        r#"function(req) return req:header("x-bench") or "" end"#,
    );
    let lua_body_addr =
        bench_helpers::spawn_lua_server(&lua, r#"function(req) return req.body end"#);
    let lua_headers_obj_addr = bench_helpers::spawn_lua_server(
        &lua,
        r#"function(req) return req.headers["x-bench"] or "" end"#,
    );
    let lua_json_addr = bench_helpers::spawn_lua_server(
        &lua,
        r#"function(req)
            local j = req.json
            if j == nil then return "nil" end
            return tostring(j:get("id"))
        end"#,
    );

    let mut g = c.benchmark_group("hyper");
    g.throughput(Throughput::Elements(1));

    // ---- GET tier (no body) ---------------------------------------------
    for (name, addr) in [
        ("library/get", &rust_addr),
        ("through_lua/no_touch_get", &lua_no_touch_addr),
        ("through_lua/path_get", &lua_path_addr),
        ("through_lua/method_get", &lua_method_addr),
    ] {
        let client = std::cell::RefCell::new(bench_helpers::BenchClient::new(addr.clone()));
        g.bench_function(name, |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut c = client.borrow_mut();
                    let s = c.send(|h| bench_helpers::build_get(black_box(h))).await;
                    assert_eq!(s, 200);
                });
            });
        });
    }

    // ---- GET with custom header (linear scan, no alloc) ------------------
    {
        let client =
            std::cell::RefCell::new(bench_helpers::BenchClient::new(lua_header_addr.clone()));
        g.bench_function("through_lua/header_get", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut c = client.borrow_mut();
                    let s = c
                        .send(|h| bench_helpers::build_get_with_header(black_box(h)))
                        .await;
                    assert_eq!(s, 200);
                });
            });
        });
    }

    // ---- POST tier (body materialisation) -------------------------------
    g.throughput(Throughput::Bytes(BODY.len() as u64));
    for (name, addr) in [
        ("library/post", &rust_addr),
        ("through_lua/no_touch_post", &lua_no_touch_addr),
        ("through_lua/body_post", &lua_body_addr),
        ("through_lua/headers_obj_post", &lua_headers_obj_addr),
        ("through_lua/json_post", &lua_json_addr),
    ] {
        let client = std::cell::RefCell::new(bench_helpers::BenchClient::new(addr.clone()));
        g.bench_function(name, |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut c = client.borrow_mut();
                    let s = c
                        .send(|h| bench_helpers::build_post_json(black_box(h), BODY))
                        .await;
                    assert_eq!(s, 200);
                });
            });
        });
    }

    // ---- into_out_lua: invoke handler directly, bypass HTTP --------------
    // Spawns a "no-op" service_fn on its own listener but the bench drives
    // the registered Lua handler in-process by calling it via the Rust
    // Function reference. Pure dispatch cost per request, no TCP, no body
    // collection — isolates the `Request::from_parts` + handler invoke.
    g.throughput(Throughput::Elements(1));
    let handler_fn: Function = lua
        .load(r#"return function(req) return req.method end"#)
        .eval()
        .expect("inline handler");
    let req_headers = vec![
        ("host".to_string(), "localhost".to_string()),
        ("user-agent".to_string(), "bench".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ];
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let req = Request::from_parts(
                "GET".to_string(),
                "/".to_string(),
                req_headers.clone(),
                Vec::new(),
            );
            let _: Value = handler_fn.call(black_box(req)).unwrap();
        });
    });

    g.finish();
}

#[cfg(feature = "bench")]
mod bench_helpers {
    use super::*;
    use mlua::ObjectLike;
    use tokio::net::TcpListener;
    use tokio::runtime::Runtime;

    pub fn spawn_rust_baseline(rt: &Runtime) -> SocketAddr {
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let listener = Arc::new(listener);

        rt.spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = hyper::service::service_fn(|_req: HyperRequest<Incoming>| async {
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"hi"))))
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });

        addr
    }

    pub fn spawn_lua_server(lua: &Lua, handler_src: &str) -> String {
        let listener_fn: Function = lua
            .load(r#"return (require("std:net")).listener"#)
            .eval()
            .expect("require std:net");
        let serve_fn: Function = lua
            .load(r#"return (require("vnd:hyper")).serve"#)
            .eval()
            .expect("require vnd:hyper");

        let (listener_v, lerr): (Value, Value) =
            listener_fn.call("127.0.0.1:0").expect("listener call");
        assert!(lerr.is_nil(), "listener error: {lerr:?}");

        let handler: Function = lua
            .load(handler_src)
            .eval()
            .unwrap_or_else(|e| panic!("compile handler {handler_src:?}: {e}"));

        let (server_v, serr): (Value, Value) = serve_fn.call((listener_v, handler)).expect("serve");
        assert!(serr.is_nil(), "serve error: {serr:?}");
        let server_ud: mlua::AnyUserData = server_v.as_userdata().expect("server ud").clone();
        server_ud.call_method::<String>("addr", ()).expect("addr")
    }

    /// Reused HTTP/1.1 keep-alive client. Connection-per-iter exhausts
    /// localhost ephemeral ports under criterion's iter rates; sharing one
    /// keep-alive sender across iterations sidesteps that and isolates the
    /// per-request path from TCP setup cost. Reconnects once on send
    /// failure to recover from server-side connection drops mid-bench.
    pub struct BenchClient {
        addr: String,
        sender: Option<hyper::client::conn::http1::SendRequest<Full<Bytes>>>,
    }

    impl BenchClient {
        pub fn new(addr: String) -> Self {
            Self { addr, sender: None }
        }

        async fn connect(&mut self) {
            let stream = TcpStream::connect(&self.addr).await.expect("connect");
            let io = TokioIo::new(stream);
            let (sender, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io)
                .await
                .expect("handshake");
            tokio::spawn(async move {
                let _ = conn.await;
            });
            self.sender = Some(sender);
        }

        pub async fn send<F>(&mut self, build: F) -> u16
        where
            F: Fn(&str) -> HyperRequest<Full<Bytes>>,
        {
            for _ in 0..2 {
                if self.sender.is_none() {
                    self.connect().await;
                }
                let req = build(&self.addr);
                match self.sender.as_mut().unwrap().send_request(req).await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        let _ = resp.into_body().collect().await.expect("collect");
                        return status;
                    }
                    Err(_) => {
                        self.sender = None;
                    }
                }
            }
            panic!("send failed twice for {}", self.addr);
        }
    }

    pub fn build_get(host: &str) -> HyperRequest<Full<Bytes>> {
        HyperRequest::builder()
            .method("GET")
            .uri("/")
            .header("host", host)
            .body(Full::new(Bytes::new()))
            .expect("build GET")
    }

    pub fn build_get_with_header(host: &str) -> HyperRequest<Full<Bytes>> {
        HyperRequest::builder()
            .method("GET")
            .uri("/")
            .header("host", host)
            .header("x-bench", "ping")
            .body(Full::new(Bytes::new()))
            .expect("build GET+hdr")
    }

    pub fn build_post_json(host: &str, body: &'static [u8]) -> HyperRequest<Full<Bytes>> {
        HyperRequest::builder()
            .method("POST")
            .uri("/")
            .header("host", host)
            .header("content-type", "application/json")
            .header("content-length", body.len().to_string())
            .header("x-bench", "ping")
            .body(Full::new(Bytes::from_static(body)))
            .expect("build POST")
    }
}
