//! Hand-rolled JSON-RPC 2.0 peer over one WebSocket: both sides may send requests, responses and
//! notifications. Why not jsonrpsee: its server API is request/response + subscriptions; it has no
//! way for the server to issue a *request* to a connected client and await the answer.

use async_trait::async_trait;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{Sink, SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{
    mpsc::{self, error::TrySendError},
    oneshot, OwnedSemaphorePermit, Semaphore,
};
use tokio_util::sync::CancellationToken;
use vos_domain::{
    protocol::{RpcError, RpcId, RpcMessage},
    ui::node::limits::MAX_MESSAGE_BYTES,
    Device,
};

/// How many requests one connection may have in flight (queued or running) at once. The next one is answered
/// at once with [`BUSY`], so a client that floods `/rpc` slows only itself.
pub const MAX_IN_FLIGHT: usize = 64;

/// The JSON-RPC server error for a connection over [`MAX_IN_FLIGHT`].
pub const BUSY: i32 = -32000;

/// How many messages may wait to be written to one client. A client whose connection has stalled without closing (a
/// sleeping tablet, a dead Wi-Fi link) stops taking messages, and everything the host sends it queues here: past this
/// many the host closes the connection instead of growing its memory with every broadcast. The client reconnects (its
/// shell re-reads what it shows), so what is lost is what was queued and what the host sends while it is away:
/// notifications are not kept for a client that is not connected. A client that is merely slow never gets near it: it
/// is a bound on a stalled connection, not on a burst (a full list of app windows is a few dozen messages).
pub const MAX_OUTBOUND: usize = 512;

/// How long the socket may take to accept one message before the connection is closed. A write that has made no
/// progress for this long is a stalled connection (the operating system's own send buffer is full and nothing is
/// reading), not a slow one.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// The last connection id handed out.
static LAST_CONNECTION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum RpcCallError {
    #[error("peer returned error {0:?}")]
    Remote(RpcError),
    #[error("connection closed")]
    Closed,
    #[error("timed out")]
    Timeout,
}

type Pending = HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>;

/// The queue of what is to be written to one client, and the way to hang up on it. Bounded by [`MAX_OUTBOUND`]: a
/// message that does not fit means the client cannot keep up, and the connection is closed (never grown, never
/// silently dropped: a client that lost a message and carried on would show stale state with nothing to tell it).
#[derive(Clone)]
struct Outbound {
    tx: mpsc::Sender<RpcMessage>,
    closed: CancellationToken,
}

impl Outbound {
    /// Queue a message. `Closed` when the connection is over, or when this very message found the queue full and
    /// ended it.
    fn send(&self, message: RpcMessage) -> Result<(), RpcCallError> {
        match self.tx.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                tracing::warn!(
                    limit = MAX_OUTBOUND,
                    "a client cannot keep up with what the host sends it: closing its connection so it reconnects"
                );
                self.closed.cancel();
                Err(RpcCallError::Closed)
            }
            Err(TrySendError::Closed(_)) => Err(RpcCallError::Closed),
        }
    }
}

/// The sending half, cloneable; handed to the handler so it can push notifications / call the client.
#[derive(Clone)]
pub struct RpcPeer {
    out: Outbound,
    pending: Arc<Mutex<Pending>>,
    next_id: Arc<AtomicI64>,
    /// This connection's id: from a counter, so it is never reused while the host runs.
    connection: u64,
    device: Option<Device>,
}

impl RpcPeer {
    /// The paired device on the other end (`None` only when the router is mounted without a session guard).
    pub fn device(&self) -> Option<&Device> {
        self.device.as_ref()
    }

    /// An id of this connection: unique for as long as the host runs, so state keyed by it (views) can never
    /// be inherited by a later connection.
    pub fn connection_id(&self) -> u64 {
        self.connection
    }

    /// Do both handles belong to the same socket?
    pub fn is_same_connection(&self, other: &RpcPeer) -> bool {
        self.connection == other.connection
    }

    /// Hang up on this client (its device was revoked). In-flight requests end with `Closed`.
    pub fn close(&self) {
        self.out.closed.cancel();
    }

    /// Queue a notification for this client. `Closed` when the connection is over, which includes a client that could
    /// not keep up (see [`MAX_OUTBOUND`]): the connection is closed and the client reconnects.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcCallError> {
        self.out.send(RpcMessage::notification(method, params))
    }

    /// Server -> client request; resolves when the client answers (or times out / disconnects).
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RpcCallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        if self.out.send(RpcMessage::request(id, method, params)).is_err() {
            self.pending.lock().remove(&id);
            return Err(RpcCallError::Closed);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(r)) => r.map_err(RpcCallError::Remote),
            Ok(Err(_)) => Err(RpcCallError::Closed),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(RpcCallError::Timeout)
            }
        }
    }
}

#[async_trait]
pub trait RpcHandler: Send + Sync + 'static {
    /// A client request. Runs in its own task, so it may `peer.request(..).await` without deadlock.
    async fn request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError>;
    async fn notification(&self, _peer: &RpcPeer, _method: &str, _params: Value) {}
    /// Called once per connection (register the peer in the host's client list / `EventSink`).
    async fn connected(&self, _peer: RpcPeer) {}
    /// Called once when the connection ended, however it ended.
    async fn disconnected(&self, _peer: &RpcPeer) {}
}

pub fn rpc_router(handler: Arc<dyn RpcHandler>) -> Router {
    Router::new().route("/rpc", get(upgrade)).with_state(handler)
}

async fn upgrade(
    ws: WebSocketUpgrade,
    State(h): State<Arc<dyn RpcHandler>>,
    device: Option<Extension<Device>>,
) -> Response {
    let device = device.map(|Extension(d)| d);
    ws.max_message_size(MAX_MESSAGE_BYTES).on_upgrade(move |socket| serve_socket(socket, h, device))
}

async fn serve_socket(socket: WebSocket, handler: Arc<dyn RpcHandler>, device: Option<Device>) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::channel::<RpcMessage>(MAX_OUTBOUND);
    let closed = CancellationToken::new();
    let out = Outbound { tx, closed: closed.clone() };
    let peer = RpcPeer {
        out: out.clone(),
        pending: Arc::default(),
        next_id: Arc::new(AtomicI64::new(1)),
        connection: LAST_CONNECTION.fetch_add(1, Ordering::Relaxed) + 1,
        device,
    };
    // single writer task: everything outbound goes through the channel
    let writer = tokio::spawn(pump(rx, sink, WRITE_TIMEOUT, closed));

    handler.connected(peer.clone()).await;

    // The view protocol is ordered: a person's keystrokes are `view.event`s with increasing `seq`, and the host
    // drops one that arrives after a later one as a replay. Those requests run one at a time, in arrival
    // order; everything else (app management, anything that may call back into the client) runs concurrently.
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let (ordered_tx, mut ordered_rx) = mpsc::unbounded_channel::<(Job, OwnedSemaphorePermit)>();
    let ordered = {
        let (h, p) = (handler.clone(), peer.clone());
        tokio::spawn(async move {
            while let Some((job, permit)) = ordered_rx.recv().await {
                let reply = answer(&*h, &p, job).await;
                drop(permit);
                if p.out.send(reply).is_err() {
                    break;
                }
            }
        })
    };

    loop {
        let frame = tokio::select! {
            () = peer.out.closed.cancelled() => break,
            next = stream.next() => match next {
                Some(Ok(frame)) => frame,
                _ => break,
            },
        };
        let Message::Text(text) = frame else {
            if matches!(frame, Message::Close(_)) {
                break;
            }
            continue;
        };
        match serde_json::from_str::<RpcMessage>(&text) {
            Err(_) => {
                let _ = out.send(RpcMessage::err(None, -32700, "parse error"));
            }
            Ok(RpcMessage::Request { id, method, params, .. }) => {
                let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                    let busy =
                        format!("too many requests in flight on this connection (at most {MAX_IN_FLIGHT})");
                    let _ = out.send(RpcMessage::err(Some(id), BUSY, &busy));
                    continue;
                };
                let job = (id, method, params);
                if is_ordered(&job.1) {
                    let _ = ordered_tx.send((job, permit)); // one at a time, in the order sent
                } else {
                    let (h, p) = (handler.clone(), peer.clone());
                    tokio::spawn(async move {
                        let reply = answer(&*h, &p, job).await;
                        drop(permit);
                        p.out.send(reply).ok()
                    });
                }
            }
            Ok(RpcMessage::Notification { method, params, .. }) => {
                // in order, before the next frame is read: `client.state` is FIFO ahead of a later event. A panic
                // in it is logged and the connection reads on (a notification has no reply to carry an error)
                let outcome =
                    vos_application::panics::caught(handler.notification(&peer, &method, params)).await;
                if let Err(message) = outcome {
                    tracing::error!(%method, %message, "handling a notification panicked");
                }
            }
            Ok(RpcMessage::Response { id: RpcId::Num(id), result, .. }) => {
                if let Some(w) = peer.pending.lock().remove(&id) {
                    let _ = w.send(Ok(result));
                }
            }
            Ok(RpcMessage::Error { id: Some(RpcId::Num(id)), error, .. }) => {
                if let Some(w) = peer.pending.lock().remove(&id) {
                    let _ = w.send(Err(error));
                }
            }
            Ok(_) => {}
        }
    }
    peer.pending.lock().clear(); // wakes every in-flight server->client request with Closed
    handler.disconnected(&peer).await;
    ordered.abort();
    writer.abort();
}

/// The single writer of one connection: everything outbound goes through its queue, one message at a time. It ends the
/// connection (`closed`) when the socket fails or stops taking messages for `timeout`, however it ended, so the read
/// side stops too: a connection that stalls without closing would otherwise be read from forever.
async fn pump<S: Sink<Message> + Unpin>(
    mut rx: mpsc::Receiver<RpcMessage>,
    mut sink: S,
    timeout: Duration,
    closed: CancellationToken,
) {
    while let Some(message) = rx.recv().await {
        let Ok(text) = serde_json::to_string(&message) else { continue };
        match tokio::time::timeout(timeout, sink.send(Message::text(text))).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => break,
            Err(_) => {
                tracing::warn!(?timeout, "a client's socket stopped taking messages: closing its connection");
                break;
            }
        }
    }
    closed.cancel();
}

/// Requests that must be handled in the order the client sent them (the view protocol).
fn is_ordered(method: &str) -> bool {
    ["view.", "row.", "state.", "source."].iter().any(|p| method.starts_with(p))
}

/// A request taken off the socket: its id, method and params.
type Job = (RpcId, String, Value);

/// Answer one request. A panic in the handler answers it with an internal error (`-32603`) instead of ending the
/// task that runs it: the request gets its reply, and the connection's ordered task goes on serving the next
/// `view.event` (a window that stopped answering would look alive and not be).
async fn answer(handler: &dyn RpcHandler, peer: &RpcPeer, (id, method, params): Job) -> RpcMessage {
    match vos_application::panics::caught(handler.request(peer, &method, params)).await {
        Ok(Ok(v)) => RpcMessage::ok(id, v),
        Ok(Err(e)) => {
            RpcMessage::Error { jsonrpc: vos_domain::protocol::Version::V2, id: Some(id), error: e }
        }
        Err(message) => {
            tracing::error!(%method, %message, "answering a request panicked");
            RpcMessage::err(
                Some(id),
                -32603,
                &format!("internal error: the host panicked answering `{method}`"),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    /// A sink that never accepts a message: the connection of a client that stopped reading.
    struct Stalled;
    impl Sink<Message> for Stalled {
        type Error = axum::Error;
        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
        fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
            Ok(())
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
    }

    fn note() -> RpcMessage {
        RpcMessage::notification("n", Value::Null)
    }

    #[tokio::test]
    async fn the_writer_closes_a_connection_whose_socket_stops_taking_messages() {
        let (tx, rx) = mpsc::channel(4);
        let closed = CancellationToken::new();
        tx.try_send(note()).unwrap();
        let writer = tokio::spawn(pump(rx, Stalled, Duration::from_millis(50), closed.clone()));
        tokio::time::timeout(Duration::from_secs(5), closed.cancelled())
            .await
            .expect("closed after the timeout");
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn a_full_queue_closes_the_connection_and_every_later_send_says_closed() {
        let (tx, _rx) = mpsc::channel(2);
        let out = Outbound { tx, closed: CancellationToken::new() };
        assert!(out.send(note()).is_ok());
        assert!(out.send(note()).is_ok());
        assert!(!out.closed.is_cancelled());
        assert!(matches!(out.send(note()), Err(RpcCallError::Closed)));
        assert!(out.closed.is_cancelled(), "the message that did not fit hung up on the client");
        assert!(matches!(out.send(note()), Err(RpcCallError::Closed)));
    }

    #[tokio::test]
    async fn a_connection_that_has_ended_says_closed_without_hanging_up_again() {
        let (tx, rx) = mpsc::channel(2);
        drop(rx);
        let out = Outbound { tx, closed: CancellationToken::new() };
        assert!(matches!(out.send(note()), Err(RpcCallError::Closed)));
    }
}
