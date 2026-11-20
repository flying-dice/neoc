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
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:net")?;

    // `require("std:net").TcpListener.bind(addr)`
    m.type_("TcpListener", |t| {
        t.try_constructor(
            "bind",
            &[("addr", "string")],
            "Bind a TCP socket to `addr` (e.g. `\"0.0.0.0:3000\"`). Returns a listener handle on success.",
            |lua, addr: String| bind(&addr).map(|l| Listener::new(lua, l)),
        )?
        .method(
            "addr",
            &[],
            "string",
            "Return the local address the socket is bound to, as a string like `\"127.0.0.1:8080\"`.",
        );
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
/// accept loop polls it through [`ListenerSlot::poll_accept`] rather than holding a clone of the socket, so
/// taking it out ([`ListenerSlot::close`], which a script's shutdown does) drops the socket on the spot and
/// frees the port before the shutdown returns, not when the runtime next gets to the aborted task.
///
/// Any number of servers may accept on one slot (the `std:thread` pattern of several workers serving one shared
/// listener). Tokio's `poll_accept` remembers only the waker of whoever polled last, where `accept()` queues every
/// waiter, so the slot keeps the waiting tasks itself and hands tokio one waker that wakes them all: whoever polls
/// first takes the connection, the rest wait again, and a server that closes leaves the others woken as before.
pub struct ListenerSlot {
    socket: parking_lot::Mutex<Option<TcpListener>>,
    waiting: Arc<Waiting>,
}

/// The tasks waiting to accept, woken together.
#[derive(Default)]
struct Waiting(parking_lot::Mutex<Vec<std::task::Waker>>);

impl Waiting {
    /// Add `waker` unless that task is already waiting.
    fn add(&self, waker: &std::task::Waker) {
        let mut waiting = self.0.lock();
        if !waiting.iter().any(|known| known.will_wake(waker)) {
            waiting.push(waker.clone());
        }
    }

    /// Drop `waker` (its task stopped waiting).
    fn remove(&self, waker: &std::task::Waker) {
        self.0.lock().retain(|known| !known.will_wake(waker));
    }

    fn wake_all(&self) {
        // taken out first: a woken task polls again, and adds itself again, from another thread
        let waiting = std::mem::take(&mut *self.0.lock());
        for waker in waiting {
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
        }
    }

    /// Release the socket, and wake every task waiting to accept on it: they get a `NotConnected` error.
    pub fn close(&self) {
        // dropped outside the lock
        let socket = self.socket.lock().take();
        drop(socket);
        self.waiting.wake_all();
    }

    /// The next connection, or a `NotConnected` error once the slot is closed. Any number of tasks may wait on
    /// the same slot; a future dropped while it waits takes its task out of the list.
    pub fn accept(&self) -> Accept<'_> {
        Accept {
            slot: self,
            waker: None,
        }
    }

    /// Poll for the next connection, or a `NotConnected` error once the slot is closed. Safe to call from any
    /// number of tasks at once; [`ListenerSlot::accept`] is the future that also cleans up after itself.
    pub fn poll_accept(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<(tokio::net::TcpStream, SocketAddr)>> {
        let socket = self.socket.lock();
        let Some(listener) = socket.as_ref() else {
            return std::task::Poll::Ready(Err(closed()));
        };
        self.waiting.add(cx.waker());
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
pub struct Accept<'a> {
    slot: &'a ListenerSlot,
    waker: Option<std::task::Waker>,
}

impl std::future::Future for Accept<'_> {
    type Output = std::io::Result<(tokio::net::TcpStream, SocketAddr)>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if !self.waker.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
            self.waker = Some(cx.waker().clone());
        }
        self.slot.poll_accept(cx)
    }
}

impl Drop for Accept<'_> {
    fn drop(&mut self) {
        // a server that stops waiting (it closed, or its loop was aborted) leaves nothing in the list
        if let Some(waker) = self.waker.take() {
            self.slot.waiting.remove(&waker);
        }
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

impl UserData for Listener {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
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
