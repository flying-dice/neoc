//! The socket behind a script's `WebSocket`: a writing half behind a lock, and a reading task that queues what the
//! peer sends.
//!
//! The reading task belongs to the engine's [`Life`], so a shut-down engine stops it, and to the socket's handles: when
//! the last one goes, the task is stopped and the connection dropped. It reads all the time, so a ping is answered (by
//! tungstenite) while the script is busy elsewhere; the queue is bounded, so a peer that sends faster than the script
//! reads is slowed down by TCP rather than filling memory.

use std::io;
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt as _, StreamExt as _};
use mlua::Lua;
use neoc_net::websocket::{Connected, Socket};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{self, Message as Frame};

use crate::websocket::{Data, Message, MessageKind, WebSocketError};
use neoc_vm_core::lifecycle::Life;

/// How many messages wait for the script before the reading task stops reading.
const QUEUE: usize = 64;
/// The longest close reason RFC 6455 allows: a control frame's 125 bytes less the code's two.
const MAX_REASON: usize = 123;
/// How long the peer has to finish the close handshake (answer this end's close, or end the connection once it has
/// answered the peer's), before this end drops the connection.
const CLOSE_WAIT: std::time::Duration =
    if cfg!(test) { std::time::Duration::from_millis(500) } else { std::time::Duration::from_secs(5) };

/// The writing half, shared by the script's calls and the reading task; `None` once the connection is over (the
/// reading task let go of it).
type Sink = Arc<tokio::sync::Mutex<Option<SplitSink<Socket, Frame>>>>;

type Queue = mpsc::Receiver<Result<Message, WebSocketError>>;

/// A socket taken over as a transport of text lines: what to send them with, and the lines that come. When the
/// lines are dropped (the connection on them ended), the socket is closed.
pub struct Lines {
    /// One text message per line.
    pub outgoing: std::pin::Pin<Box<dyn futures_util::Sink<String, Error = io::Error> + Send>>,
    /// The text messages, until the socket closes. Binary messages are skipped.
    pub incoming: std::pin::Pin<Box<dyn futures_util::Stream<Item = io::Result<String>> + Send>>,
}

/// Closes the socket when the transport that took it over lets go of it (the connection on it ended).
struct CloseOnDrop(Arc<Inner>);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        let inner = Arc::clone(&self.0);
        // no runtime: the engine is going, and the socket with it
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = inner.close(1000, String::new()).await;
            });
        }
    }
}

pub(crate) struct Inner {
    pub(crate) url: String,
    pub(crate) protocol: Option<String>,
    pub(crate) local_address: String,
    sink: Sink,
    /// `None` once a transport took the socket over.
    queue: tokio::sync::Mutex<Option<Queue>>,
    taken: Mutex<bool>,
    /// The peer's close code and reason, once it sent them.
    closed: Arc<Mutex<Option<(u16, String)>>>,
    /// A close frame was sent.
    closing: Mutex<bool>,
    reader: AbortHandle,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

fn closed_error(reason: &str) -> WebSocketError {
    WebSocketError::Closed { reason: reason.to_owned() }
}

fn lost(e: &tungstenite::Error) -> WebSocketError {
    match e {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            closed_error("the socket is closed")
        }
        tungstenite::Error::Io(e) => WebSocketError::ConnectionLost { reason: e.to_string() },
        tungstenite::Error::Capacity(e) => WebSocketError::Protocol { reason: e.to_string() },
        tungstenite::Error::Protocol(e) => WebSocketError::Protocol { reason: e.to_string() },
        other => WebSocketError::ConnectionLost { reason: other.to_string() },
    }
}

/// Read what the peer sends into `queue` until the socket ends, then let go of the connection: both halves are
/// dropped, so the TCP connection closes even while the script still holds the socket.
async fn read(
    mut stream: SplitStream<Socket>,
    sink: Sink,
    queue: mpsc::Sender<Result<Message, WebSocketError>>,
    closed: Arc<Mutex<Option<(u16, String)>>>,
) {
    let peer_closed = read_frames(&mut stream, &queue, &closed).await;
    if peer_closed {
        // tungstenite queued the answer to the peer's close: reading on flushes it, and the stream ends once the
        // handshake is done (or the peer hangs up); a peer that does neither is dropped after a while
        let drain = async { while stream.next().await.is_some() {} };
        let _ = tokio::time::timeout(CLOSE_WAIT, drain).await;
    }
    if let Some(mut sink) = sink.lock().await.take() {
        // flush what is still queued (the close reply); the connection is over either way
        let _ = tokio::time::timeout(CLOSE_WAIT, sink.flush()).await;
    }
    drop(stream);
    let mut closed = closed.lock();
    if closed.is_none() {
        *closed = Some((1006, String::new()));
    }
}

/// Queue what the peer sends until a close frame (its code and reason kept in `closed`: `true`), the end of the
/// stream, an error (queued) or a queue nobody reads.
async fn read_frames(
    stream: &mut SplitStream<Socket>,
    queue: &mpsc::Sender<Result<Message, WebSocketError>>,
    closed: &Mutex<Option<(u16, String)>>,
) -> bool {
    while let Some(frame) = stream.next().await {
        let message = match frame {
            Ok(Frame::Text(text)) => {
                Message { kind: MessageKind::Text, data: Data(text.as_bytes().to_vec()) }
            }
            Ok(Frame::Binary(bytes)) => Message { kind: MessageKind::Binary, data: Data(bytes.to_vec()) },
            Ok(Frame::Close(frame)) => {
                let (code, reason) = frame.map_or((1005, String::new()), |frame| {
                    (u16::from(frame.code), frame.reason.as_str().to_owned())
                });
                *closed.lock() = Some((code, reason));
                return true;
            }
            // tungstenite answers pings itself; pongs and raw frames are not the script's
            Ok(Frame::Ping(_) | Frame::Pong(_) | Frame::Frame(_)) => continue,
            Err(e) => {
                if closed.lock().is_none() {
                    *closed.lock() = Some((1006, String::new()));
                }
                let _ = queue.send(Err(lost(&e))).await;
                return false;
            }
        };
        if queue.send(Ok(message)).await.is_err() {
            return false;
        }
    }
    false
}

impl Inner {
    pub(crate) fn start(lua: &Lua, url: String, connected: Connected) -> Arc<Self> {
        let Connected { socket, protocol, local_address } = connected;
        let (sink, stream) = socket.split();
        let sink: Sink = Arc::new(tokio::sync::Mutex::new(Some(sink)));
        let (tx, rx) = mpsc::channel(QUEUE);
        let closed = Arc::new(Mutex::new(None));
        let reader =
            Life::of(lua).spawn(read(stream, Arc::clone(&sink), tx, Arc::clone(&closed))).abort_handle();
        Arc::new(Self {
            url,
            protocol,
            local_address,
            sink,
            queue: tokio::sync::Mutex::new(Some(rx)),
            taken: Mutex::new(false),
            closed,
            closing: Mutex::new(false),
            reader,
        })
    }

    pub(crate) fn closed(&self) -> Option<(u16, String)> {
        self.closed.lock().clone()
    }

    async fn send(&self, frame: Frame) -> Result<(), WebSocketError> {
        if *self.closing.lock() {
            return Err(closed_error("the socket was closed"));
        }
        if self.closed.lock().is_some() {
            return Err(closed_error("the peer closed the socket"));
        }
        let mut sink = self.sink.lock().await;
        let sink = sink.as_mut().ok_or_else(|| closed_error("the socket is closed"))?;
        sink.send(frame).await.map_err(|e| lost(&e))
    }

    pub(crate) async fn send_text(&self, text: String) -> Result<(), WebSocketError> {
        self.send(Frame::text(text)).await
    }

    pub(crate) async fn send_binary(&self, data: Vec<u8>) -> Result<(), WebSocketError> {
        self.send(Frame::binary(data)).await
    }

    pub(crate) async fn ping(&self, data: Vec<u8>) -> Result<(), WebSocketError> {
        if data.len() > 125 {
            return Err(WebSocketError::InvalidInput {
                reason: format!("a ping carries at most 125 bytes, got {}", data.len()),
            });
        }
        self.send(Frame::Ping(data.into())).await
    }

    pub(crate) async fn receive(&self) -> Result<Option<Message>, WebSocketError> {
        let mut queue = self.queue.lock().await;
        let Some(queue) = queue.as_mut() else {
            return Err(closed_error("the socket was taken over by a transport"));
        };
        queue.recv().await.transpose()
    }

    pub(crate) async fn close(&self, code: u16, reason: String) -> Result<(), WebSocketError> {
        if code != 1000 && !(3000..=4999).contains(&code) {
            return Err(WebSocketError::InvalidInput {
                reason: format!("a close code is 1000 or 3000 to 4999, got {code}"),
            });
        }
        if reason.len() > MAX_REASON {
            return Err(WebSocketError::InvalidInput {
                reason: format!("a close reason is at most {MAX_REASON} bytes, got {}", reason.len()),
            });
        }
        {
            let mut closing = self.closing.lock();
            if *closing {
                return Ok(());
            }
            *closing = true;
        }
        if self.closed.lock().is_some() {
            return Ok(());
        }
        let frame = CloseFrame { code: CloseCode::from(code), reason: reason.into() };
        let sent = {
            let mut sink = self.sink.lock().await;
            let Some(sink) = sink.as_mut() else { return Ok(()) };
            sink.send(Frame::Close(Some(frame))).await
        };
        self.drop_after_close_wait();
        match sent {
            Ok(()) | Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => Ok(()),
            Err(e) => Err(lost(&e)),
        }
    }

    /// A peer that does not answer this end's close within [`CLOSE_WAIT`] (or a reading task stuck behind a full
    /// queue) is cut off: the reading task stops, which ends a `receive` that waits, and both halves are dropped.
    fn drop_after_close_wait(&self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
        let reader = self.reader.clone();
        let sink = Arc::clone(&self.sink);
        let closed = Arc::clone(&self.closed);
        runtime.spawn(async move {
            tokio::time::sleep(CLOSE_WAIT).await;
            if reader.is_finished() {
                return;
            }
            reader.abort();
            drop(sink.lock().await.take());
            let mut closed = closed.lock();
            if closed.is_none() {
                *closed = Some((1006, String::new()));
            }
        });
    }

    pub(crate) fn take_lines(this: &Arc<Self>) -> Option<Lines> {
        let mut taken = this.taken.lock();
        if *taken {
            return None;
        }
        // a script's `receive` that is waiting holds the queue: the socket is not free to take
        let queue = this.queue.try_lock().ok().and_then(|mut queue| queue.take())?;
        *taken = true;
        drop(taken);
        let guard = CloseOnDrop(Arc::clone(this));
        let incoming = futures_util::stream::unfold((queue, guard), |(mut queue, guard)| async move {
            loop {
                match queue.recv().await? {
                    Ok(Message { kind: MessageKind::Text, data }) => {
                        let line = String::from_utf8_lossy(&data.0).into_owned();
                        return Some((Ok(line), (queue, guard)));
                    }
                    Ok(Message { kind: MessageKind::Binary, .. }) => {}
                    Err(e) => return Some((Err(io::Error::other(e.to_string())), (queue, guard))),
                }
            }
        });
        let outgoing = futures_util::sink::unfold(Arc::clone(this), |inner, line: String| async move {
            inner.send_text(line).await.map_err(|e| io::Error::other(e.to_string()))?;
            Ok::<_, io::Error>(inner)
        });
        Some(Lines { outgoing: Box::pin(outgoing), incoming: Box::pin(incoming) })
    }
}
