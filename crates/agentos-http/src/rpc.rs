//! Hand-rolled JSON-RPC 2.0 peer over one WebSocket: both sides may send requests, responses and
//! notifications. Why not jsonrpsee: its server API is request/response + subscriptions; it has no
//! way for the server to issue a *request* to a connected client and await the answer.

use agentos_domain::{
    protocol::{RpcError, RpcId, RpcMessage},
    Device,
};
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
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

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
    device: Option<Device>,
    closed: CancellationToken,
}

impl RpcPeer {
    /// The paired device on the other end (`None` only when the router is mounted without a session guard).
    pub fn device(&self) -> Option<&Device> {
        self.device.as_ref()
    }

    /// An id of this connection, unique among the live ones.
    pub fn connection_id(&self) -> u64 {
        Arc::as_ptr(&self.next_id) as usize as u64
    }

    /// Do both handles belong to the same socket?
    pub fn is_same_connection(&self, other: &RpcPeer) -> bool {
        Arc::ptr_eq(&self.next_id, &other.next_id)
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
    ws.max_message_size(1 << 20).on_upgrade(move |socket| serve_socket(socket, h, device))
}

async fn serve_socket(socket: WebSocket, handler: Arc<dyn RpcHandler>, device: Option<Device>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<RpcMessage>();
    let peer = RpcPeer {
        tx: tx.clone(),
        pending: Arc::default(),
        next_id: Arc::new(AtomicI64::new(1)),
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
    let (ordered_tx, mut ordered_rx) = mpsc::unbounded_channel::<(RpcId, String, Value)>();
    let ordered = {
        let (h, p, tx) = (handler.clone(), peer.clone(), tx.clone());
        tokio::spawn(async move {
            while let Some(job) = ordered_rx.recv().await {
                if tx.send(answer(&*h, &p, job).await).is_err() {
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
                let job = (id, method, params);
                if is_ordered(&job.1) {
                    let _ = ordered_tx.send(job); // one at a time, in the order sent
                } else {
                    let (h, p, tx) = (handler.clone(), peer.clone(), tx.clone());
                    tokio::spawn(async move { tx.send(answer(&*h, &p, job).await).ok() });
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

async fn answer(
    handler: &dyn RpcHandler,
    peer: &RpcPeer,
    (id, method, params): (RpcId, String, Value),
) -> RpcMessage {
    match handler.request(peer, &method, params).await {
        Ok(v) => RpcMessage::ok(id, v),
        Err(e) => {
            RpcMessage::Error { jsonrpc: agentos_domain::protocol::Version::V2, id: Some(id), error: e }
        }
    }
}
