//! `vnd:hyper` server path — the `ServerHandle` userdata and its Idle→Listening
//! →Closed state machine, the `serve` entry point, and the accept/dispatch
//! loop. Per-request handling lives in `super::invoke_lua_handler`.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request as HyperRequest, Response};
use hyper_util::rt::TokioIo;
use mlua::{Function, Lua, RegistryKey, UserData, UserDataMethods, Value};
use parking_lot::Mutex;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::net::Listener;
use crate::lua::std::workers::ShutdownSignal;

use super::invoke_lua_handler;

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
    // TODO: clean-code - 0.6 - KISS: replace-with-Closed-then-restore-on-Listening dance is harder than peeking; check state first, only consume on Idle.
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
pub(super) fn serve(
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
