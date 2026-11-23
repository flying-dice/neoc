//! Network primitives — `std:net`.
//!
//! Exposes `TcpListener.bind(addr)`, the building block consumed by
//! `(require("vnd:hyper")).serve(listener, handler)`. Created here so
//! the script controls the topology: bind once and pass to a single
//! hyper service, or hand off via `thread.spawn { shared }` to fan out
//! across workers.
//!
//! ```lua
//! local TcpListener = require("std:net").TcpListener
//! local hyper       = require("vnd:hyper")
//! local thread      = require("std:thread")
//!
//! -- single-worker
//! local listener = TcpListener.bind("0.0.0.0:3000")
//! local server = hyper.serve(listener, function(req) ... end)
//!
//! -- shared via spawn-time handoff (one fd, all workers accept)
//! local listener = TcpListener.bind("0.0.0.0:3000")
//! for i = 1, 4 do
//!     thread.spawn({ file = "worker.lua", shared = { listener = listener } })
//! end
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use mlua::{Lua, Table, UserData, UserDataMethods};
use tokio::net::TcpListener;

use crate::lua::facade::Module;
use crate::lua::lifecycle::Life;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:net` surface through the facade.
///
/// `TcpListener` is a constructor sub-table reached as
/// `require("std:net").TcpListener.bind(addr)`. The fallible constructor routes
/// through [`crate::lua::facade::TypeSpec::try_constructor`], which opens the
/// `"TcpListener.bind"` span at `lua::lib::net` and marshals the
/// `(value, error)` idiom with the `"TcpListener.bind"` error prefix — exactly
/// as the hand-written
/// `#[instrument(name = "TcpListener.bind", target = "lua::lib::net")]`
/// registration did. The `Listener` (`addr`) method keeps its own
/// `impl UserData` and is recorded here.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:net")?;

    // `require("std:net").TcpListener.bind(addr)`
    m.type_("TcpListener", |t| {
        Listener::lua(t);
        /// Bind a TCP socket to `addr` (e.g. `"0.0.0.0:3000"`). Returns a listener handle on success.
        #[lua]
        t.try_constructor("bind", |lua, addr: String| {
            bind(&addr).map(|l| Listener::new(lua, l))
        })?;
        Ok(())
    })?;

    Ok(m)
}

fn bind(addr: &str) -> Result<TcpListener, String> {
    // `from_err` adds the `TcpListener.bind` context; here just name the address.
    let std_listener = std::net::TcpListener::bind(addr).map_err(|e| format!("{addr}: {e}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|e| format!("nonblocking: {e}"))?;
    TcpListener::from_std(std_listener).map_err(|e| format!("from_std: {e}"))
}

/// The socket behind a [`Listener`], the only strong owner of it that the library keeps: a `vnd:hyper` server's
/// accept loop waits on it through [`ListenerSlot::accept`] rather than holding a clone of the socket, so
/// taking it out ([`ListenerSlot::close`], which a script's shutdown does) drops the socket on the spot and
/// frees the port before the shutdown returns, not when the runtime next gets to the aborted task.
///
/// Any number of servers may accept on one slot (the `std:thread` pattern of several workers serving one shared
/// listener). Tokio's `poll_accept` remembers only the waker of whoever polled last, where `accept()` queues every
/// waiter, so the slot keeps the waits itself and hands tokio one waker that wakes them all: whoever polls
/// first takes the connection, the rest wait again, and a server that closes leaves the others woken as before.
pub struct ListenerSlot {
    socket: parking_lot::Mutex<Option<TcpListener>>,
    waiting: Arc<Waiting>,
    next_wait: std::sync::atomic::AtomicU64,
}

/// The waits for a connection, woken together. Each wait has an entry of its own, so a task with two waits (a
/// `select!` over two, or one kept while another is replaced) loses neither when the other ends.
#[derive(Default)]
struct Waiting(parking_lot::Mutex<Vec<(u64, std::task::Waker)>>);

impl Waiting {
    /// Record that wait `id` is waiting, to be woken through `waker`.
    fn add(&self, id: u64, waker: &std::task::Waker) {
        let mut waiting = self.0.lock();
        match waiting.iter_mut().find(|(known, _)| *known == id) {
            Some((_, known)) => {
                if !known.will_wake(waker) {
                    // the old waker is dropped after the lock is let go: dropping a waker can drop a task
                    let old = std::mem::replace(known, waker.clone());
                    drop(waiting);
                    drop(old);
                }
            }
            None => waiting.push((id, waker.clone())),
        }
    }

    /// Drop wait `id` (it ended).
    fn remove(&self, id: u64) {
        let removed = {
            let mut waiting = self.0.lock();
            let at = waiting.iter().position(|(known, _)| *known == id);
            at.map(|at| waiting.swap_remove(at))
        };
        // dropped outside the lock, as in `wake_all`
        drop(removed);
    }

    fn wake_all(&self) {
        // taken out first: a woken task polls again, and adds itself again, from another thread
        let waiting = std::mem::take(&mut *self.0.lock());
        for (_, waker) in waiting {
            waker.wake();
        }
    }
}

impl futures::task::ArcWake for Waiting {
    fn wake_by_ref(this: &Arc<Self>) {
        this.wake_all();
    }
}

impl ListenerSlot {
    fn new(socket: TcpListener) -> Self {
        Self {
            socket: parking_lot::Mutex::new(Some(socket)),
            waiting: Arc::new(Waiting::default()),
            next_wait: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Release the socket, and wake every wait on it: they get a `NotConnected` error.
    pub fn close(&self) {
        // dropped outside the lock
        let socket = self.socket.lock().take();
        drop(socket);
        self.waiting.wake_all();
    }

    /// The next connection, or a `NotConnected` error once the slot is closed. Any number of waits, from any
    /// number of tasks, may be pending on the same slot; a wait that is dropped takes only its own entry away.
    pub(crate) fn accept(&self) -> Accept<'_> {
        Accept {
            slot: self,
            id: self
                .next_wait
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Poll wait `id` for the next connection, or a `NotConnected` error once the slot is closed.
    fn poll_accept(
        &self,
        id: u64,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<(tokio::net::TcpStream, SocketAddr)>> {
        let socket = self.socket.lock();
        let Some(listener) = socket.as_ref() else {
            return std::task::Poll::Ready(Err(closed()));
        };
        self.waiting.add(id, cx.waker());
        let wake_all = futures::task::waker(self.waiting.clone());
        listener.poll_accept(&mut std::task::Context::from_waker(&wake_all))
    }

    /// The address the socket is bound to, or `NotConnected` once the slot is closed.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        match self.socket.lock().as_ref() {
            Some(listener) => listener.local_addr(),
            None => Err(closed()),
        }
    }
}

/// A wait for the next connection on a [`ListenerSlot`] (see [`ListenerSlot::accept`]).
pub(crate) struct Accept<'a> {
    slot: &'a ListenerSlot,
    id: u64,
}

impl std::future::Future for Accept<'_> {
    type Output = std::io::Result<(tokio::net::TcpStream, SocketAddr)>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.slot.poll_accept(self.id, cx)
    }
}

impl Drop for Accept<'_> {
    fn drop(&mut self) {
        // a server that stops waiting (it closed, or its loop was aborted) leaves nothing in the list
        self.slot.waiting.remove(self.id);
    }
}

fn closed() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotConnected, "the listener is closed")
}

/// Lua-facing TCP listener handle. Named `Listener` on the Rust side to
/// avoid collision with `tokio::net::TcpListener` in scope; exposed to
/// Lua as `TcpListener` via the module constructor table.
///
/// Dropping a Lua VM frees its listeners, but a script that is unloaded or replaced may still be referenced
/// for a moment (a call in flight, the next version loading): the socket is therefore released by the
/// engine's [`Life`] at shutdown, so a replacement can bind the same port.
#[derive(Clone)]
pub struct Listener(Arc<ListenerSlot>);

impl Listener {
    fn new(lua: &Lua, listener: TcpListener) -> Self {
        let slot = Arc::new(ListenerSlot::new(listener));
        // a weak reference: a script that drops a listener (and binds the next) leaves nothing registered
        Life::of(lua).on_shutdown_with(&slot, |slot| slot.close());
        Self(slot)
    }

    /// The slot that owns the socket: share this, not a clone of the socket, with whatever should end when the
    /// script is shut down.
    #[must_use]
    pub fn slot(&self) -> Arc<ListenerSlot> {
        self.0.clone()
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

impl crate::lua::std::thread::Sendable for Listener {
    fn as_shared(&self) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
        self.0.clone()
    }
    fn from_shared(arc: &std::sync::Arc<dyn std::any::Any + Send + Sync>) -> Option<Self> {
        arc.clone().downcast::<ListenerSlot>().ok().map(Listener)
    }
}

#[neoc_macros::lua_userdata]
impl UserData for Listener {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Return the local address the socket is bound to, as a string like `"127.0.0.1:8080"`.
        #[lua("string")]
        methods.add_method("addr", |_, this, _: ()| match this.local_addr() {
            Ok(a) => Ok(a.to_string()),
            Err(e) => Err(mlua::Error::external(e)),
        });
    }
}

#[cfg(test)]
mod slot_tests {
    use super::*;
    use std::time::Duration;

    async fn slot() -> Arc<ListenerSlot> {
        let listener = bind("127.0.0.1:0").unwrap();
        Arc::new(ListenerSlot::new(listener))
    }

    /// A wait that ends (a server closed, its loop was aborted) leaves nothing in the list of waiting tasks, so a
    /// script that serves and closes on one idle listener does not grow it.
    #[tokio::test]
    async fn a_dropped_wait_leaves_the_list_of_waiting_tasks() {
        let slot = slot().await;
        for _ in 0..50 {
            let waiting = tokio::time::timeout(Duration::from_millis(1), slot.accept()).await;
            assert!(waiting.is_err(), "nothing is connecting");
        }
        assert_eq!(slot.waiting.0.lock().len(), 0);
    }

    /// Two waits of one task are two entries: dropping one does not take the other's wake-up with it. The task
    /// polls both once (both pending), drops the first and then does nothing of its own: only the slot can wake
    /// it, for the second.
    #[tokio::test]
    async fn dropping_one_of_a_tasks_two_waits_keeps_the_other_woken() {
        use std::future::Future;
        use std::pin::Pin;
        use std::task::Poll;

        let slot = slot().await;
        let addr = slot.local_addr().unwrap();
        let accepted = tokio::spawn({
            let slot = slot.clone();
            async move {
                let mut first = Some(slot.accept());
                let mut second = Some(slot.accept());
                let mut polled = false;
                std::future::poll_fn(move |cx| {
                    if !polled {
                        polled = true;
                        let a = Pin::new(first.as_mut().unwrap()).poll(cx);
                        let b = Pin::new(second.as_mut().unwrap()).poll(cx);
                        assert!(a.is_pending() && b.is_pending(), "nothing is connecting");
                        // the first wait ends (a `select!` arm that lost); the task waits for the second
                        first = None;
                        return Poll::Pending;
                    }
                    Pin::new(second.as_mut().unwrap())
                        .poll(cx)
                        .map(|r| r.map(|_| ()))
                })
                .await
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let accepted = tokio::time::timeout(Duration::from_secs(2), accepted)
            .await
            .expect("the second wait was woken by the connection")
            .unwrap();
        assert!(accepted.is_ok());
    }

    /// Closing the slot wakes every task waiting on it, with an error, and frees the socket at once.
    #[tokio::test]
    async fn closing_wakes_every_waiting_task() {
        let slot = slot().await;
        let addr = slot.local_addr().unwrap();
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let slot = slot.clone();
                tokio::spawn(async move { slot.accept().await.map(|_| ()) })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(50)).await;
        slot.close();
        assert!(
            std::net::TcpListener::bind(addr).is_ok(),
            "the port is free"
        );
        for task in tasks {
            let ended = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("a waiting task was woken by the close")
                .unwrap();
            assert_eq!(ended.unwrap_err().kind(), std::io::ErrorKind::NotConnected);
        }
    }
}
