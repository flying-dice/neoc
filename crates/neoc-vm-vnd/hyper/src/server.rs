//! `vnd:hyper` server path: a server's Listening→Closed state, the accept loop, and the call into the script's
//! handler for each request.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request as HyperRequest, Response};
use hyper_util::rt::TokioIo;
use mlua::{Function, Lua, Value};
use mlua_ir::FromLuaArg;
use parking_lot::Mutex;
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};

use super::collect_headers;
use super::hyper::{Answer, Request, ServerError};
use neoc_vm_core::lifecycle::{Life, idle};
use neoc_vm_std::net::ListenerSlot;
use neoc_vm_std::workers::workers::ShutdownSignal;

/// A server: the script's handler, and whether its accept loop is live.
pub struct Server {
    /// The one function every request is dispatched to (hyper's `service_fn`).
    handler: Function,
    state: Mutex<State>,
}

enum State {
    /// The accept loop is live.
    Listening { addr: SocketAddr, shutdown: Option<oneshot::Sender<()>>, done: watch::Receiver<bool> },
    /// `close()` was called and the loop ended, or the engine shut down.
    Closed,
}

impl Server {
    /// Start accepting on `listener` and dispatching every request to `handler`. The accept loop is registered
    /// with the engine's [`Life`]: shutting the script down aborts it.
    pub(super) fn start(
        lua: &Lua,
        listener: Arc<ListenerSlot>,
        handler: Function,
    ) -> Result<Arc<Self>, ServerError> {
        let addr = listener.local_addr().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotConnected => ServerError::ListenerClosed,
            _ => ServerError::Other { reason: e.to_string() },
        })?;
        Handle::try_current()
            .map_err(|_| ServerError::Other { reason: "no tokio runtime in scope".to_owned() })?;

        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let (done_tx, done) = watch::channel(false);
        let server = Arc::new(Self {
            handler,
            state: Mutex::new(State::Listening { addr, shutdown: Some(shutdown), done }),
        });
        let life = Life::of(lua);
        life.spawn(run(lua.clone(), listener, server.clone(), shutdown_rx, done_tx));
        // The server reports itself closed from the moment the script is shut down (`addr()` says so, `wait()`
        // returns), not only when the aborted accept loop is next dropped; the socket itself is released by the
        // listener's own registration. No lock is held here: on a registry that is already shut down the release
        // runs at once, and it takes the lock itself.
        life.on_shutdown_with(&server, |server| {
            *server.state.lock() = State::Closed;
        });
        Ok(server)
    }

    /// The address the server accepts on; none once it is closed.
    pub(super) fn addr(&self) -> Option<SocketAddr> {
        match &*self.state.lock() {
            State::Listening { addr, .. } => Some(*addr),
            State::Closed => None,
        }
    }

    /// Ask the accept loop to end. It returns at once.
    pub(super) fn close(&self) {
        if let State::Listening { shutdown, .. } = &mut *self.state.lock()
            && let Some(shutdown) = shutdown.take()
        {
            let _ = shutdown.send(());
        }
    }

    /// Wait until the server stops accepting connections: because `close()` was called, because the script is
    /// being shut down (the engine's `life` closes), or because the process-wide shutdown signal fires (the CLI
    /// fires it on Ctrl-C and on SIGTERM, and it stays fired). On the signal this asks the accept loop to end and
    /// waits for it. The wait does not hold the VM (an embedder's turn): the server's own handlers keep running
    /// while a script sits in `server:wait()`.
    pub(super) async fn wait(&self, life: Life) -> mlua::Result<()> {
        let mut done = match &*self.state.lock() {
            State::Listening { done, .. } => done.clone(),
            State::Closed => return Ok(()),
        };
        if *done.borrow() {
            return Ok(());
        }
        let signal = ShutdownSignal::current();
        let signalled = idle(async {
            tokio::select! {
                _ = done.changed() => false,
                () = life.closed() => false,
                () = signal.wait() => true,
            }
        })
        .await?;
        if signalled {
            self.close();
            idle(async {
                let _ = done.changed().await;
            })
            .await?;
        }
        Ok(())
    }
}

async fn run(
    lua: Lua,
    listener: Arc<ListenerSlot>,
    server: Arc<Server>,
    mut shutdown: oneshot::Receiver<()>,
    done: watch::Sender<bool>,
) {
    let signal = ShutdownSignal::current();
    let life = Life::of(&lua);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            () = signal.wait() => break,
            accept = listener.accept() => {
                let stream = match accept {
                    Ok((stream, _)) => stream,
                    // the socket was released (the script was shut down): there is nothing to accept on
                    Err(e) if e.kind() == std::io::ErrorKind::NotConnected => break,
                    // a persistent error (out of descriptors) must not spin
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let server = server.clone();
                let lua = lua.clone();
                life.spawn(async move {
                    let io = TokioIo::new(stream);
                    let service = service_fn(move |req: HyperRequest<Incoming>| {
                        let server = server.clone();
                        let lua = lua.clone();
                        async move { handle(&lua, &server.handler, req).await }
                    });
                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
                });
            }
        }
    }
    // Closed first, then the waiters: `wait()` returns on another thread as soon as `done` is set, and the script
    // it returns to must find the server closed (`addr()`), not still listening. The state held a receiver, so the
    // value is set whether or not anyone is left to read it.
    *server.state.lock() = State::Closed;
    done.send_replace(true);
}

/// Read the request, call the handler with it, and answer what the handler returned. A handler that raises, or
/// returns what is not a reply, answers 500 `handler error`.
async fn handle(
    lua: &Lua,
    handler: &Function,
    req: HyperRequest<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    let headers = collect_headers(req.headers());
    let body = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(_) => return Ok(error_response(500, "body read failed")),
    };

    let handled = neoc_vm_core::panics::caught(async {
        let request = Request::from_parts(method, path, headers, body);
        // the handler is a call into the VM like any other: it waits for its turn (see `lifecycle`)
        let raw: Value = neoc_vm_core::lifecycle::request(lua, handler.call_async(request)).await?;
        Option::<Answer>::from_lua_arg(raw, lua)
            .map_err(|why| mlua::Error::runtime(format!("the handler returned what is not a reply: {why}")))
    })
    .await;
    // a host function the handler reached panicked: that request fails like a handler error, and the connection
    // and the server live on
    let answer: mlua::Result<Option<Answer>> =
        handled.unwrap_or_else(|message| Err(neoc_vm_core::panics::error(&message)));

    match answer {
        Ok(answer) => Ok(response_of(answer)),
        Err(e) => {
            let error = neoc_vm_core::host::error_text(&e);
            tracing::error!(target: "lua::lib::hyper", %error, traceback = %e, "a request handler failed");
            Ok(error_response(500, "handler error"))
        }
    }
}

/// The response a handler's answer describes: a string is a 200 with that body, nothing an empty 200.
fn response_of(answer: Option<Answer>) -> Response<Full<Bytes>> {
    let (status, headers, body) = match answer {
        None => (200, None, Vec::new()),
        Some(Answer::Body(body)) => (200, None, body.0),
        Some(Answer::Reply(reply)) => {
            (reply.status.unwrap_or(200), reply.headers, reply.body.map(|body| body.0).unwrap_or_default())
        }
    };
    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter().flatten() {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder.body(Full::new(Bytes::from(body))).unwrap_or_else(|_| error_response(500, "build failed"))
}

fn error_response(status: u16, text: &'static str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from_static(text.as_bytes())));
    *response.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    response
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::Body;
    use super::super::hyper::Reply;
    use super::*;

    async fn parts(response: Response<Full<Bytes>>) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let status = response.status().as_u16();
        let headers = collect_headers(response.headers());
        let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
        (status, headers, body)
    }

    #[tokio::test]
    async fn a_string_is_a_200_body_nothing_is_an_empty_200_and_a_reply_is_what_it_says() {
        assert_eq!(parts(response_of(None)).await, (200, Vec::new(), Vec::new()));
        let text = response_of(Some(Answer::Body(mlua_ir::Bytes(b"hi \xff".to_vec()))));
        assert_eq!(parts(text).await, (200, Vec::new(), b"hi \xff".to_vec()));
        let empty = response_of(Some(Answer::Reply(Reply::default())));
        assert_eq!(parts(empty).await, (200, Vec::new(), Vec::new()));
        let reply = Reply {
            status: Some(404),
            headers: Some(BTreeMap::from([("content-type".to_owned(), "text/plain".to_owned())])),
            body: Some(Body(b"not found".to_vec())),
        };
        assert_eq!(
            parts(response_of(Some(Answer::Reply(reply)))).await,
            (404, vec![("content-type".to_owned(), "text/plain".to_owned())], b"not found".to_vec())
        );
    }

    /// The accept loop ends on a thread of the runtime, and `wait()` returns as soon as `done` says so: the server
    /// must be `Closed` before that, or a script that calls `addr()` right after `wait()` may still be given the
    /// address. The test holds the state's lock, so the loop cannot close the server: `done` must stay unset
    /// until it can.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_accept_loop_closes_the_server_before_it_wakes_who_waits() {
        use neoc_vm_std::net::net::TcpListener;

        let lua = neoc_vm_core::make_engine().expect("engine");
        let bound: Value =
            lua.load(r#"return (require("std:net").TcpListener.bind("127.0.0.1:0"))"#).eval().expect("bind");
        let listener = TcpListener::from_lua_arg(bound, &lua).expect("a listener");
        let handler = lua.create_function(|_, ()| Ok(())).expect("handler");
        let server = Server::start(&lua, listener.slot(), handler).expect("server");

        let mut done = {
            let mut state = server.state.lock();
            let State::Listening { shutdown, done, .. } = &mut *state else {
                panic!("the server is listening")
            };
            let done = done.clone();
            shutdown.take().expect("not closed yet").send(()).expect("the accept loop runs");
            // long enough for the loop to end and reach the lock
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(!*done.borrow(), "the waiters were woken while the server still said it was listening");
            done
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), done.wait_for(|done| *done))
            .await
            .expect("the accept loop ended")
            .expect("the accept loop said so");
        assert!(server.addr().is_none());
    }

    /// What hyper cannot build a response from (a status outside 100..=999, a header name that is not one) is a
    /// 500, not a panic and not a dropped connection.
    #[tokio::test]
    async fn a_reply_hyper_cannot_build_is_a_500() {
        for reply in [
            Reply { status: Some(99), ..Reply::default() },
            Reply {
                headers: Some(BTreeMap::from([("bad name".to_owned(), "x".to_owned())])),
                ..Reply::default()
            },
        ] {
            let (status, _, body) = parts(response_of(Some(Answer::Reply(reply)))).await;
            assert_eq!((status, body.as_slice()), (500, b"build failed".as_slice()));
        }
    }
}
