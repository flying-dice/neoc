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
use futures_util::{SinkExt, StreamExt};
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
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
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

/// The sending half, cloneable; handed to the handler so it can push notifications / call the client.
#[derive(Clone)]
pub struct RpcPeer {
    tx: mpsc::UnboundedSender<RpcMessage>,
    pending: Arc<Mutex<Pending>>,
    next_id: Arc<AtomicI64>,
    /// This connection's id: from a counter, so it is never reused while the host runs.
    connection: u64,
    device: Option<Device>,
    closed: CancellationToken,
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
        self.closed.cancel();
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcCallError> {
        self.tx.send(RpcMessage::notification(method, params)).map_err(|_| RpcCallError::Closed)
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
        if self.tx.send(RpcMessage::request(id, method, params)).is_err() {
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
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<RpcMessage>();
    let peer = RpcPeer {
        tx: tx.clone(),
        pending: Arc::default(),
        next_id: Arc::new(AtomicI64::new(1)),
        connection: LAST_CONNECTION.fetch_add(1, Ordering::Relaxed) + 1,
        device,
        closed: CancellationToken::new(),
    };
    // single writer task: everything outbound goes through the channel
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&m) else { continue };
            if sink.send(Message::text(text)).await.is_err() {
                break;
            }
        }
    });

    handler.connected(peer.clone()).await;

    // The view protocol is ordered: a person's keystrokes are `view.event`s with increasing `seq`, and the host
    // drops one that arrives after a later one as a replay. Those requests run one at a time, in arrival
    // order; everything else (app management, anything that may call back into the client) runs concurrently.
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let (ordered_tx, mut ordered_rx) = mpsc::unbounded_channel::<(Job, OwnedSemaphorePermit)>();
    let ordered = {
        let (h, p, tx) = (handler.clone(), peer.clone(), tx.clone());
        tokio::spawn(async move {
            while let Some((job, permit)) = ordered_rx.recv().await {
                let reply = answer(&*h, &p, job).await;
                drop(permit);
                if tx.send(reply).is_err() {
                    break;
                }
            }
        })
    };

    loop {
        let frame = tokio::select! {
            () = peer.closed.cancelled() => break,
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
                let _ = tx.send(RpcMessage::err(None, -32700, "parse error"));
            }
            Ok(RpcMessage::Request { id, method, params, .. }) => {
                let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                    let busy =
                        format!("too many requests in flight on this connection (at most {MAX_IN_FLIGHT})");
                    let _ = tx.send(RpcMessage::err(Some(id), BUSY, &busy));
                    continue;
                };
                let job = (id, method, params);
                if is_ordered(&job.1) {
                    let _ = ordered_tx.send((job, permit)); // one at a time, in the order sent
                } else {
                    let (h, p, tx) = (handler.clone(), peer.clone(), tx.clone());
                    tokio::spawn(async move {
                        let reply = answer(&*h, &p, job).await;
                        drop(permit);
                        tx.send(reply).ok()
                    });
                }
            }
            Ok(RpcMessage::Notification { method, params, .. }) => {
                // in order, before the next frame is read: `client.state` is FIFO ahead of a later event
                handler.notification(&peer, &method, params).await;
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

/// Requests that must be handled in the order the client sent them (the view protocol).
fn is_ordered(method: &str) -> bool {
    ["view.", "row.", "state.", "source."].iter().any(|p| method.starts_with(p))
}

/// A request taken off the socket: its id, method and params.
type Job = (RpcId, String, Value);

async fn answer(handler: &dyn RpcHandler, peer: &RpcPeer, (id, method, params): Job) -> RpcMessage {
    match handler.request(peer, &method, params).await {
        Ok(v) => RpcMessage::ok(id, v),
        Err(e) => RpcMessage::Error { jsonrpc: vos_domain::protocol::Version::V2, id: Some(id), error: e },
    }
}
