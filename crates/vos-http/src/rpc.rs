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
        atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
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
pub const BUSY: i32 = vos_domain::ui::protocol::codes::BUSY;

/// How many bytes may wait to be written to one client. A client whose connection has stalled without closing (a
/// sleeping tablet, a dead Wi-Fi link) stops taking messages, and everything the host sends it queues here: past this
/// the host closes the connection instead of growing its memory with every broadcast, so a stalled client holds at
/// most this much (plus the one message the socket is writing). The client reconnects (its shell re-reads what it
/// shows), so what is lost is what was queued and what the host sends while it is away: notifications are not kept
/// for a client that is not connected.
///
/// The bound is bytes, not a count, and is far above any burst a client that reads will see, so a client that keeps up
/// is never closed for one. The biggest bursts are opening an app whose view is large (one view tree is at most about
/// a mebibyte, [`MAX_MESSAGE_BYTES`]) beside a few windows whose live sources refresh at once, or an agent streaming
/// into several chats: a few mebibytes. 32 MiB is ten times that, and small enough for a host on a home server to hold
/// for one stalled tablet. A message is taken unless more than this is already waiting, whatever its own size, and it
/// stops counting when the writer takes it, so a reader is not closed for one message that is big in itself; it is
/// closed for more than this waiting to be taken (a burst of more than this, sent faster than the writer can take it,
/// is a stall as far as the host can tell). The host holds at most this much, plus the last message that was taken
/// while it was under the limit (any size), plus the one being written.
pub const MAX_QUEUED_BYTES: usize = 32 << 20;

/// How long the socket may go without taking a message before the connection is closed, however small it is.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// The slowest rate at which a message still counts as moving: the time allowed for a message is [`WRITE_TIMEOUT`] plus
/// its size at this rate, so a large message on a slow link (a 1 MiB view tree at 128 kbit/s takes about a minute) is
/// not cut off while it is still going out. A link slower than this for the whole message is as good as stalled.
pub const MIN_WRITE_BYTES_PER_SEC: u64 = 16 * 1024;

/// The time the socket gets to take a message of `len` bytes.
fn write_timeout(len: usize) -> Duration {
    WRITE_TIMEOUT
        + Duration::from_millis(u64::try_from(len).unwrap_or(u64::MAX) * 1000 / MIN_WRITE_BYTES_PER_SEC)
}

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
    /// The message could not be written as JSON, so it was not sent (a bug in the host, logged where it happened).
    #[error("message could not be serialised: {0}")]
    Unserialisable(String),
}

type Pending = HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>;

/// The queue of what is to be written to one client, and the way to hang up on it. Messages are serialised when they
/// are queued, and the bytes waiting are counted: past [`MAX_QUEUED_BYTES`] the client cannot keep up, and the
/// connection is closed (never grown, never silently dropped: a client that lost a message and carried on would show
/// stale state with nothing to tell it). Every message to a client goes through [`Outbound::send`]: there is no other
/// way in, so the bound holds for all of them.
#[derive(Clone)]
struct Outbound {
    tx: mpsc::UnboundedSender<String>,
    /// Bytes of the messages waiting in the queue. The writer takes a message off this count when it takes it from the
    /// queue, so the one it is writing does not count against what may wait behind it: a message that is big in
    /// itself (bigger than [`MAX_QUEUED_BYTES`]) is not what closes a client that is reading. What the host holds for
    /// a client is this count plus that one message.
    queued: Arc<AtomicUsize>,
    closed: CancellationToken,
    /// Set by the first hang-up the host chose (not a revoke or the client leaving): the log line is for that one.
    dropped: Arc<AtomicBool>,
    /// For the log line: which connection, and which paired device.
    connection: u64,
    device: Option<Arc<str>>,
}

impl Outbound {
    /// Queue a message. `Closed` when the connection is over, or when this very message found more than the byte limit
    /// already waiting and ended it (the message's own size is not what is judged: a big one is taken, and counts
    /// against what follows only until the writer takes it); `Unserialisable` when it cannot be written as JSON (it is
    /// then not sent).
    fn send(&self, message: &RpcMessage) -> Result<(), RpcCallError> {
        if self.closed.is_cancelled() {
            return Err(RpcCallError::Closed);
        }
        self.send_text(serialise(message)?)
    }

    /// Queue a message that is already serialised (one message for many clients is serialised once).
    fn send_text(&self, text: String) -> Result<(), RpcCallError> {
        if self.closed.is_cancelled() {
            return Err(RpcCallError::Closed);
        }
        let len = text.len();
        let before = self.queued.fetch_add(len, Ordering::SeqCst);
        if before > MAX_QUEUED_BYTES {
            self.queued.fetch_sub(len, Ordering::SeqCst);
            self.hang_up("too much is waiting to be sent to it", before);
            return Err(RpcCallError::Closed);
        }
        if self.tx.send(text).is_err() {
            self.queued.fetch_sub(len, Ordering::SeqCst);
            return Err(RpcCallError::Closed);
        }
        Ok(())
    }

    /// Close the connection because the client cannot keep up. Logs once, on the transition: the next sends find it
    /// closed. Returns whether this call was the one that closed it.
    fn hang_up(&self, why: &str, queued_bytes: usize) -> bool {
        let first = !self.dropped.swap(true, Ordering::SeqCst);
        if first {
            tracing::warn!(
                connection = self.connection,
                device = self.device.as_deref().unwrap_or("none"),
                queued_bytes,
                limit = MAX_QUEUED_BYTES,
                why,
                "closing a client's connection so it reconnects: it cannot keep up with what the host sends it"
            );
        }
        self.closed.cancel();
        first
    }
}

fn serialise(message: &RpcMessage) -> Result<String, RpcCallError> {
    serde_json::to_string(message).map_err(|e| {
        tracing::error!(error = %e, "a message for a client could not be serialised: not sent");
        RpcCallError::Unserialisable(e.to_string())
    })
}

/// A notification serialised once, to be sent to many clients ([`RpcPeer::send_prepared`]).
#[derive(Clone)]
pub struct PreparedNotification(Arc<str>);

impl PreparedNotification {
    /// `Unserialisable` when it cannot be written as JSON (logged; nothing is sent to anyone).
    pub fn new(method: &str, params: Value) -> Result<Self, RpcCallError> {
        serialise(&RpcMessage::notification(method, params)).map(|text| Self(text.into()))
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

    /// Bytes of messages waiting to be written to this client (not the one being written). It is how a test, or a
    /// gauge, tells that the writer has taken what was sent.
    pub fn queued_bytes(&self) -> usize {
        self.out.queued.load(Ordering::SeqCst)
    }

    /// Queue a notification that was serialised once for many clients, through the same bounded queue as every other message.
    pub fn send_prepared(&self, message: &PreparedNotification) -> Result<(), RpcCallError> {
        self.out.send_text(message.0.to_string())
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
    /// not keep up (see [`MAX_QUEUED_BYTES`]): the connection is closed and the client reconnects.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcCallError> {
        self.out.send(&RpcMessage::notification(method, params))
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
        if self.out.send(&RpcMessage::request(id, method, params)).is_err() {
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
    let (tx, rx) = mpsc::unbounded_channel::<String>(); // unbounded in messages, bounded in bytes: `Outbound::send`
    let connection = LAST_CONNECTION.fetch_add(1, Ordering::Relaxed) + 1;
    let out = Outbound {
        tx,
        queued: Arc::default(),
        closed: CancellationToken::new(),
        dropped: Arc::default(),
        connection,
        device: device.as_ref().map(|d| Arc::from(d.id.0.as_str())),
    };
    let peer = RpcPeer {
        out: out.clone(),
        pending: Arc::default(),
        next_id: Arc::new(AtomicI64::new(1)),
        connection,
        device,
    };
    // single writer task: everything outbound goes through the queue
    let writer = tokio::spawn(pump(rx, sink, out.clone(), write_timeout));

    handler.connected(peer.clone()).await;

    // The view protocol is ordered: a person's keystrokes are `view.event`s with increasing `seq`, and the host
    // drops one that arrives after a later one as a replay. Those requests run one at a time, in arrival
    // order; everything else (app management, anything that may call back into the client) runs concurrently.
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    // The concurrent requests that are running, by id: `notifications/cancelled` ends one (see [`CANCELLED`]), and the connection
    // closing ends them all, because each holds a child of the connection's own token.
    let running: Arc<Mutex<HashMap<String, CancellationToken>>> = Arc::default();
    let (ordered_tx, mut ordered_rx) = mpsc::unbounded_channel::<(Job, OwnedSemaphorePermit)>();
    let ordered = {
        let (h, p) = (handler.clone(), peer.clone());
        tokio::spawn(async move {
            while let Some((job, permit)) = ordered_rx.recv().await {
                let reply = answer(&*h, &p, job).await;
                drop(permit);
                if p.out.send(&reply).is_err() {
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
                let _ = out.send(&RpcMessage::err(None, -32700, "parse error"));
            }
            Ok(RpcMessage::Request { id, method, params, .. }) => {
                let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                    let busy =
                        format!("too many requests in flight on this connection (at most {MAX_IN_FLIGHT})");
                    let _ = out.send(&RpcMessage::err(Some(id), BUSY, &busy));
                    continue;
                };
                let job = (id, method, params);
                if is_ordered(&job.1) {
                    let _ = ordered_tx.send((job, permit)); // one at a time, in the order sent
                } else {
                    spawn_request(&handler, &peer, &running, job, permit);
                }
            }
            Ok(RpcMessage::Notification { method, params, .. }) if method == CANCELLED => {
                // MCP's cancellation: `{ requestId, reason? }`. Unknown, finished or malformed ones are ignored (it is a notification:
                // there is nobody to tell), and a request that is ordered (the view protocol) is not cancellable.
                let id =
                    params.get("requestId").and_then(|v| serde_json::from_value::<RpcId>(v.clone()).ok());
                if let Some(token) = id.and_then(|id| running.lock().get(&id_key(&id)).cloned()) {
                    token.cancel();
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
    // the connection is over, however it ended: what its requests still run (handlers waiting on something) is ended with it, and
    // gives back what it holds (the pump's own end does this when it is the writer that stops; a reader that stops first must too)
    peer.out.closed.cancel();
    peer.pending.lock().clear(); // wakes every in-flight server->client request with Closed
    handler.disconnected(&peer).await;
    ordered.abort();
    writer.abort();
}

/// The single writer of one connection: everything outbound goes through its queue, one message at a time. It gives the
/// socket `timeout_for(len)` to take each message, and ends the connection when the socket fails or stops taking
/// messages, however it ended, so the read side stops too: a connection that stalls without closing would otherwise be
/// read from for ever.
async fn pump<S: Sink<Message> + Unpin>(
    mut rx: mpsc::UnboundedReceiver<String>,
    mut sink: S,
    out: Outbound,
    timeout_for: impl Fn(usize) -> Duration,
) {
    while let Some(text) = rx.recv().await {
        let len = text.len();
        // taken: the message being written is no longer waiting, so it does not count against what follows it
        out.queued.fetch_sub(len, Ordering::SeqCst);
        let outcome = tokio::time::timeout(timeout_for(len), sink.send(Message::text(text))).await;
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(_)) => break,
            Err(_) => {
                out.hang_up("its socket stopped taking messages", out.queued.load(Ordering::SeqCst));
                break;
            }
        }
    }
    out.closed.cancel();
}

/// Run one concurrent request on its own task, registered in `running` so that `notifications/cancelled` (or the connection ending)
/// can end it. Dropping the answer drops the handler's call with it, and what it holds (the permit here, an app's place in
/// `AppService`): a cancelled request frees them, and gets no reply (the specification's rule).
fn spawn_request(
    handler: &Arc<dyn RpcHandler>,
    peer: &RpcPeer,
    running: &Arc<Mutex<HashMap<String, CancellationToken>>>,
    job: Job,
    permit: OwnedSemaphorePermit,
) {
    let (handler, peer, running) = (handler.clone(), peer.clone(), running.clone());
    let token = peer.out.closed.child_token();
    let key = id_key(&job.0);
    running.lock().insert(key.clone(), token.clone());
    tokio::spawn(async move {
        let reply = tokio::select! {
            () = token.cancelled() => None,
            reply = answer(&*handler, &peer, job) => Some(reply),
        };
        running.lock().remove(&key);
        drop(permit);
        if let Some(reply) = reply {
            peer.out.send(&reply).ok();
        }
    });
}

/// MCP's cancellation notification (basic/utilities/cancellation): a client that no longer wants a running request says so, and
/// the receiver stops it, frees what it holds and sends no response.
pub const CANCELLED: &str = "notifications/cancelled";

/// A request id as a map key: a number and a string of the same digits are different ids.
fn id_key(id: &RpcId) -> String {
    match id {
        RpcId::Num(n) => format!("n{n}"),
        RpcId::Str(s) => format!("s{s}"),
    }
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

/// A peer with nothing behind it but its queue, for the tests of what sends to peers (the hub).
#[cfg(test)]
pub(crate) fn test_peer(connection: u64) -> (RpcPeer, mpsc::UnboundedReceiver<String>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let out = Outbound {
        tx,
        queued: Arc::default(),
        closed: CancellationToken::new(),
        dropped: Arc::default(),
        connection,
        device: None,
    };
    let peer = RpcPeer {
        out,
        pending: Arc::default(),
        next_id: Arc::new(AtomicI64::new(1)),
        connection,
        device: None,
    };
    (peer, rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    use tracing_subscriber::fmt::MakeWriter;

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

    /// Collects what `tracing` writes, so a test can count log lines.
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);
    impl io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Lines {
        type Writer = Lines;
        fn make_writer(&'a self) -> Lines {
            self.clone()
        }
    }
    impl Lines {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock()).into_owned()
        }
    }

    fn outbound() -> (Outbound, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let out = Outbound {
            tx,
            queued: Arc::default(),
            closed: CancellationToken::new(),
            dropped: Arc::default(),
            connection: 7,
            device: Some(Arc::from("device-1")),
        };
        (out, rx)
    }

    /// A notification of about `size` bytes once written as JSON.
    fn note(size: usize) -> RpcMessage {
        RpcMessage::notification("n", serde_json::json!({ "pad": "x".repeat(size) }))
    }

    #[tokio::test]
    async fn the_writer_closes_a_connection_whose_socket_stops_taking_messages() {
        let (out, rx) = outbound();
        out.send(&note(10)).unwrap();
        let writer = tokio::spawn(pump(rx, Stalled, out.clone(), |_| Duration::from_millis(50)));
        tokio::time::timeout(Duration::from_secs(5), out.closed.cancelled())
            .await
            .expect("closed after the timeout");
        writer.await.unwrap();
        assert!(
            out.dropped.load(Ordering::SeqCst),
            "the host chose this hang-up, so it is the one that is logged"
        );
    }

    #[test]
    fn a_message_gets_the_base_time_plus_what_its_size_needs_at_the_slowest_moving_rate() {
        assert_eq!(write_timeout(0), WRITE_TIMEOUT);
        let mebibyte = write_timeout(1 << 20).checked_sub(WRITE_TIMEOUT).unwrap();
        // 1 MiB at 16 KiB/s is 64 s: a 128 kbit/s link is not cut off in the middle of a view tree
        assert_eq!(mebibyte, Duration::from_secs(64));
    }

    #[test]
    fn a_queue_over_the_byte_limit_closes_the_connection_at_the_limit_and_no_later() {
        let (out, _rx) = outbound(); // nothing reads: the queue only grows
        let size = 64 * 1024;
        let mut accepted = 0_usize;
        let refused = loop {
            match out.send(&note(size)) {
                Ok(()) => accepted = out.queued.load(Ordering::SeqCst),
                Err(e) => break e,
            }
            assert!(
                accepted <= MAX_QUEUED_BYTES + size + 64,
                "the bound did not hold: {accepted} bytes queued"
            );
        };
        assert!(matches!(refused, RpcCallError::Closed));
        // it was refused once more than the limit was waiting, so the queue was just over it, by less than a message
        assert!(accepted > MAX_QUEUED_BYTES, "closed early, at {accepted} bytes");
        assert!(out.closed.is_cancelled());
        assert!(matches!(out.send(&note(1)), Err(RpcCallError::Closed)));
        // what was refused is not counted as waiting
        assert_eq!(out.queued.load(Ordering::SeqCst), accepted);
    }

    /// What the writer takes stops counting at once, so a message bigger than the limit that is being written does not
    /// close the client that is reading it when the next message comes.
    #[tokio::test]
    async fn a_message_bigger_than_the_limit_does_not_close_a_reader_when_others_follow_it() {
        let (out, rx) = outbound();
        let writer = tokio::spawn(pump(rx, futures_util::sink::drain::<Message>(), out.clone(), |_| {
            Duration::from_secs(60)
        }));
        for _ in 0..3 {
            out.send(&note(MAX_QUEUED_BYTES + 1)).unwrap();
            // the writer takes it (the connection is reading), as it does between a view tree and what follows it
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            while out.queued.load(Ordering::SeqCst) > 0 {
                assert!(tokio::time::Instant::now() < deadline, "the writer never took the big message");
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            for _ in 0..50 {
                out.send(&note(100)).unwrap();
            }
        }
        assert!(!out.closed.is_cancelled());
        assert!(!out.dropped.load(Ordering::SeqCst));
        writer.abort();
    }

    #[test]
    fn a_message_is_taken_whatever_its_size_unless_more_than_the_limit_is_already_waiting() {
        let (out, _rx) = outbound();
        out.send(&note(100)).unwrap(); // a little is waiting: a big message is still taken
        assert!(out.send(&note(MAX_QUEUED_BYTES + 1)).is_ok());
        assert!(!out.closed.is_cancelled());
        // but not on top of that
        assert!(matches!(out.send(&note(1)), Err(RpcCallError::Closed)));
    }

    #[test]
    fn a_client_that_reads_is_never_closed_for_a_burst_far_larger_than_any_count() {
        let (out, mut rx) = outbound();
        // 100 000 messages sent with no pause: the queue is deep, but it is a few mebibytes
        for _ in 0..100_000 {
            out.send(&note(20)).unwrap();
        }
        assert!(!out.closed.is_cancelled());
        let mut read = 0;
        while let Ok(text) = rx.try_recv() {
            out.queued.fetch_sub(text.len(), Ordering::SeqCst);
            read += 1;
        }
        assert_eq!(read, 100_000);
        assert_eq!(out.queued.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_hang_up_is_logged_once_with_the_connection_and_the_device_however_many_sends_follow() {
        let lines = Lines::default();
        let subscriber = tracing_subscriber::fmt().with_writer(lines.clone()).with_ansi(false).finish();
        tracing::subscriber::with_default(subscriber, || {
            let (out, _rx) = outbound();
            for _ in 0..2000 {
                let _ = out.send(&note(64 * 1024));
            }
            assert!(!out.hang_up("again", 0), "already closed: not the one that closed it");
        });
        let text = lines.text();
        assert_eq!(text.matches("closing a client's connection").count(), 1, "{text}");
        assert!(text.contains("connection=7"), "{text}");
        assert!(text.contains(r#"device="device-1""#), "{text}");
    }

    #[tokio::test]
    async fn a_notification_serialised_once_goes_through_the_same_bounded_queue() {
        let (out, mut rx) = outbound();
        let once = PreparedNotification::new("n", serde_json::json!({ "pad": "x".repeat(10) })).unwrap();
        let (a, b) = (out.clone(), out.clone());
        a.send_text(once.0.to_string()).unwrap();
        b.send_text(once.0.to_string()).unwrap();
        assert_eq!(rx.recv().await.unwrap(), serde_json::to_string(&note(10)).unwrap());
        assert_eq!(rx.recv().await.unwrap(), rx_text_of(&once));
        // and the bound holds for it: one more message than the queue may hold ends the connection
        out.send(&note(MAX_QUEUED_BYTES + 1)).unwrap();
        let big = PreparedNotification::new("n", serde_json::json!({ "pad": "y" })).unwrap();
        assert!(matches!(out.send_text(big.0.to_string()), Err(RpcCallError::Closed)));
        assert!(out.closed.is_cancelled());
    }

    fn rx_text_of(n: &PreparedNotification) -> String {
        n.0.to_string()
    }
}
