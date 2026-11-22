//! The idle timeout, measured on the socket.
//!
//! `idle_timeout` is `URLSession`'s `timeoutIntervalForRequest`: how long a request waits for data before it gives
//! up, reset whenever data arrives. [`IdleIo`] wraps the connection's socket (below TLS, so the handshake counts too)
//! and measures exactly that: the timer is armed when a read or a write finds the socket not ready, and cleared when
//! any bytes move in either direction. So it covers every phase with one rule: a server that never answers, a body
//! that stops half way, a peer that stops reading an upload, and (the point of measuring here) a slow upload that
//! is still making progress is not cut off, as it would be by a timeout around the request as a whole.
//!
//! Nothing runs while the connection is not being polled, so a response body the script has not asked for yet does
//! not time out for that reason: it is the server's silence that is timed, not the script's pace.
//!
//! What it does not do: a peer that sends one byte just under every `idle_timeout` keeps resetting it for ever.
//! That is what the whole-request `timeout` option is for.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

/// How far ahead a deadline is put when adding the timeout to now overflows (30 years: effectively never).
const FAR: Duration = Duration::from_secs(60 * 60 * 24 * 365 * 30);

/// The idle timeout of one connection, and whether it fired, shared with whoever classifies the error the
/// connection ends with: hyper may report a timed-out socket as a closed connection or a cancelled request, and
/// this says what it really was.
#[derive(Clone)]
pub(super) struct Fired {
    flag: Arc<AtomicBool>,
    idle: Duration,
}

impl Fired {
    pub(super) fn new(idle: Duration) -> Self {
        Self {
            flag: Arc::default(),
            idle,
        }
    }

    /// The idle timeout, if it has fired.
    pub(super) fn after(&self) -> Option<Duration> {
        self.flag.load(Ordering::SeqCst).then_some(self.idle)
    }

    fn set(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

/// What a request that hit the idle timeout says, whatever hyper made of it.
pub(super) fn message(idle: Duration) -> String {
    format!("no data for {}s", idle.as_secs_f64())
}

/// A socket whose reads and writes fail with `TimedOut` after `idle` without progress.
pub(super) struct IdleIo<T> {
    inner: T,
    idle: Duration,
    timer: Pin<Box<Sleep>>,
    armed: bool,
    fired: Fired,
}

impl<T> IdleIo<T> {
    pub(super) fn new(inner: T, fired: Fired) -> Self {
        let idle = fired.idle;
        Self {
            inner,
            idle,
            timer: Box::pin(tokio::time::sleep(idle)),
            armed: false,
            fired,
        }
    }

    /// The socket was not ready: arm the timer if it is not, and fail if it has passed.
    fn wait<R>(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<R>> {
        if !self.armed {
            let deadline = Instant::now()
                .checked_add(self.idle)
                .unwrap_or_else(|| Instant::now() + FAR);
            self.timer.as_mut().reset(deadline);
            self.armed = true;
        }
        match self.timer.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.fired.set();
                Poll::Ready(Err(timed_out(self.idle)))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn expired(&self) -> Option<io::Error> {
        self.fired.after().map(timed_out)
    }
}

fn timed_out(idle: Duration) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, message(idle))
}

impl<T: AsyncRead + Unpin> AsyncRead for IdleIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Some(e) = this.expired() {
            return Poll::Ready(Err(e));
        }
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(r) => {
                this.armed = false;
                Poll::Ready(r)
            }
            Poll::Pending => this.wait(cx),
        }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for IdleIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Some(e) = this.expired() {
            return Poll::Ready(Err(e));
        }
        match Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(r) => {
                this.armed = false;
                Poll::Ready(r)
            }
            Poll::Pending => this.wait(cx),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Some(e) = this.expired() {
            return Poll::Ready(Err(e));
        }
        match Pin::new(&mut this.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(r) => {
                this.armed = false;
                Poll::Ready(r)
            }
            Poll::Pending => this.wait(cx),
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Ready(r) => Poll::Ready(r),
            Poll::Pending => this.wait(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_shutdown(cx) {
            Poll::Ready(r) => Poll::Ready(r),
            Poll::Pending => this.wait(cx),
        }
    }
}
