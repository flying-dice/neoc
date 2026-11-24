//! Network primitives: `std:net`.
//!
//! `TcpListener.bind(addr)` binds a socket. The script binds it and hands it to what serves on it, so the script
//! decides the topology: one listener for one server, or one listener shared by several workers.
//!
//! ```lua
//! local net = require("std:net")
//! local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
//! print(listener:addr()) -- 127.0.0.1:49152
//! ```
//!
//! [`ListenerSlot`] is the socket as the rest of the runtime takes it: what serves on a listener waits on the slot
//! ([`ListenerSlot::accept`]) and holds no socket of its own.
//!
//! The API's rules:
//! - A failure to bind is a typed `NetError` whose kind is the operating system's (`addr_in_use`, `addr_not_available`,
//!   `permission_denied`, ...), with the address.
//! - `addr()` of a listener the engine has closed returns a typed `closed` error.
//! - A listener is handed to a `std:thread` worker as one of `std:thread`'s `Shared` handles.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, Waker};

use parking_lot::Mutex;
use tokio::net::{TcpListener as Socket, TcpStream};

/// The socket behind a `TcpListener`, the only strong owner of it that the runtime keeps: a server's accept loop
/// waits on it through [`ListenerSlot::accept`] rather than holding a clone of the socket, so taking it out
/// ([`ListenerSlot::close`], which a script's shutdown does) drops the socket on the spot and frees the port before
/// the shutdown returns, not when the runtime next gets to the aborted task.
///
/// Any number of servers may accept on one slot (several workers serving one shared listener). Tokio's
/// `poll_accept` remembers only the waker of whoever polled last, where `accept()` queues every waiter, so the slot
/// keeps the waits itself and hands tokio one waker that wakes them all: whoever polls first takes the connection,
/// the rest wait again, and a server that closes leaves the others woken as before.
pub struct ListenerSlot {
    socket: Mutex<Option<Socket>>,
    waiting: Arc<Waiting>,
    next_wait: AtomicU64,
}

/// The waits for a connection, woken together. Each wait has an entry of its own, so a task with two waits (a
/// `select!` over two, or one kept while another is replaced) loses neither when the other ends.
#[derive(Default)]
struct Waiting(Mutex<Vec<(u64, Waker)>>);

impl Waiting {
    /// Record that wait `id` is waiting, to be woken through `waker`.
    fn add(&self, id: u64, waker: &Waker) {
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
    fn new(socket: Socket) -> Self {
        Self {
            socket: Mutex::new(Some(socket)),
            waiting: Arc::new(Waiting::default()),
            next_wait: AtomicU64::new(0),
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
    pub fn accept(&self) -> Accept<'_> {
        Accept { slot: self, id: self.next_wait.fetch_add(1, Ordering::Relaxed) }
    }

    /// Poll wait `id` for the next connection, or a `NotConnected` error once the slot is closed.
    fn poll_accept(&self, id: u64, cx: &mut Context<'_>) -> Poll<std::io::Result<(TcpStream, SocketAddr)>> {
        let socket = self.socket.lock();
        let Some(listener) = socket.as_ref() else {
            return Poll::Ready(Err(closed()));
        };
        self.waiting.add(id, cx.waker());
        let wake_all = futures::task::waker(self.waiting.clone());
        listener.poll_accept(&mut Context::from_waker(&wake_all))
    }

    /// The address the socket is bound to, or `NotConnected` once the slot is closed.
    ///
    /// # Errors
    /// The slot is closed, or the operating system cannot say.
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
    id: u64,
}

impl Future for Accept<'_> {
    type Output = std::io::Result<(TcpStream, SocketAddr)>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
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

/// A nonblocking socket bound to `addr`, registered with the runtime.
fn bind(addr: &str) -> std::io::Result<Socket> {
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    Socket::from_std(listener)
}

#[mlua_ir::module("std:net")]
pub mod net {
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{LuaError, Object};
    use serde::Serialize;

    use super::ListenerSlot;
    use neoc_vm_core::lifecycle::Life;

    /// Why a socket could not be bound, or has no address to give. The kind is the operating system's.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum NetError {
        /// Another socket is bound to the address.
        #[error("{addr}: {reason}")]
        AddrInUse {
            /// The address asked for.
            addr: String,
            /// What the operating system said.
            reason: String,
        },
        /// The address is not one of this machine's.
        #[error("{addr}: {reason}")]
        AddrNotAvailable {
            /// The address asked for.
            addr: String,
            /// What the operating system said.
            reason: String,
        },
        /// The process may not bind the address (a port below 1024, say).
        #[error("{addr}: {reason}")]
        PermissionDenied {
            /// The address asked for.
            addr: String,
            /// What the operating system said.
            reason: String,
        },
        /// The text is not an address: `host:port` is.
        #[error("{addr}: {reason}")]
        InvalidInput {
            /// The address asked for.
            addr: String,
            /// What is wrong with it.
            reason: String,
        },
        /// The listener is closed: its engine shut down, or stopped its background work.
        #[error("the listener is closed")]
        Closed,
        /// Any other failure (a host name that does not resolve, say).
        #[error("{addr}: {reason}")]
        Other {
            /// The address asked for.
            addr: String,
            /// What the operating system said.
            reason: String,
        },
    }

    impl NetError {
        fn of(error: &std::io::Error, addr: String) -> Self {
            use std::io::ErrorKind as Kind;
            let reason = error.to_string();
            match error.kind() {
                Kind::AddrInUse => NetError::AddrInUse { addr, reason },
                Kind::AddrNotAvailable => NetError::AddrNotAvailable { addr, reason },
                Kind::PermissionDenied => NetError::PermissionDenied { addr, reason },
                Kind::InvalidInput => NetError::InvalidInput { addr, reason },
                Kind::NotConnected => NetError::Closed,
                _ => NetError::Other { addr, reason },
            }
        }
    }

    /// A bound TCP socket, for what serves on it. Copies of the handle are the same socket.
    ///
    /// Dropping a Lua VM frees its listeners, but a script that is unloaded or replaced may still be referenced
    /// for a moment (a call in flight, the next version loading): the socket is therefore released by the
    /// engine's [`Life`] at shutdown, so a replacement can bind the same port.
    #[derive(Clone, Object)]
    pub struct TcpListener {
        slot: Arc<ListenerSlot>,
    }

    impl TcpListener {
        /// The slot that owns the socket: share this, not a clone of the socket, with whatever should end when the
        /// script is shut down.
        #[must_use]
        pub fn slot(&self) -> Arc<ListenerSlot> {
            self.slot.clone()
        }
    }

    #[mlua_ir::object]
    impl TcpListener {
        /// Bind a TCP socket to `addr`: `"0.0.0.0:3000"`, `"[::1]:8080"`, or port `0` for one the operating system
        /// picks (`addr()` says which). A host name is resolved first.
        ///
        /// ```luau
        /// local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
        /// local taken, err = net.TcpListener.bind((assert(listener:addr())))
        /// assert(taken == nil and err ~= nil and err.kind == "addr_in_use")
        /// ```
        pub fn bind(lua: &Lua, addr: String) -> Result<TcpListener, NetError> {
            let socket = super::bind(&addr).map_err(|e| NetError::of(&e, addr))?;
            let slot = Arc::new(ListenerSlot::new(socket));
            // a weak reference: a script that drops a listener (and binds the next) leaves nothing registered
            Life::of(lua).on_shutdown_with(&slot, |slot| slot.close());
            Ok(TcpListener { slot })
        }

        /// The address the socket is bound to, as `"127.0.0.1:8080"`.
        pub fn addr(&self) -> Result<String, NetError> {
            self.slot.local_addr().map(|addr| addr.to_string()).map_err(|e| NetError::of(&e, String::new()))
        }
    }
}

/// Whether `addr` can be bound again, for a test that says a closed listener gave its port back.
///
/// The socket is closed by the time the test asks, but on Unix a test running beside it that starts a process
/// forks this one, and the child holds a copy of every descriptor (the listener's too) until it execs: the port
/// stays taken for that moment, so the bind is tried again for a short while before the port is said to be held.
#[cfg(any(test, feature = "testing"))]
pub fn port_is_free(addr: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpListener::bind(addr).is_ok() {
            return true;
        }
        if !cfg!(unix) || std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use neoc_vm_core::lifecycle::Life;

    fn slot() -> Arc<ListenerSlot> {
        Arc::new(ListenerSlot::new(bind("127.0.0.1:0").unwrap()))
    }

    /// A wait that ends (a server closed, its loop was aborted) leaves nothing in the list of waiting tasks, so a
    /// script that serves and closes on one idle listener does not grow it.
    #[tokio::test]
    async fn a_dropped_wait_leaves_the_list_of_waiting_tasks() {
        let slot = slot();
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
        use std::pin::Pin;

        let slot = slot();
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
                    Pin::new(second.as_mut().unwrap()).poll(cx).map(|r| r.map(|_| ()))
                })
                .await
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _client = TcpStream::connect(addr).await.unwrap();
        let accepted = tokio::time::timeout(Duration::from_secs(2), accepted)
            .await
            .expect("the second wait was woken by the connection")
            .unwrap();
        assert!(accepted.is_ok());
    }

    /// Closing the slot wakes every task waiting on it, with an error, and frees the socket at once.
    #[tokio::test]
    async fn closing_wakes_every_waiting_task() {
        let slot = slot();
        let addr = slot.local_addr().unwrap();
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let slot = slot.clone();
                tokio::spawn(async move { slot.accept().await.map(|_| ()) })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(50)).await;
        slot.close();
        assert!(port_is_free(&addr.to_string()), "the port is free");
        for task in tasks {
            let ended = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("a waiting task was woken by the close")
                .unwrap();
            assert_eq!(ended.unwrap_err().kind(), std::io::ErrorKind::NotConnected);
        }
    }

    fn engine(life: Life) -> mlua::Lua {
        let lua = neoc_vm_core::make_engine_in(life).unwrap();
        let net: mlua::Table = lua.load(r#"return require("std:net")"#).eval().unwrap();
        lua.globals().set("net", net).unwrap();
        lua
    }

    #[tokio::test]
    async fn bind_gives_a_listener_that_says_its_address() {
        let lua = engine(Life::new());
        let (addr, other): (String, String) = lua
            .load(
                r#"
                local listener, err = net.TcpListener.bind("127.0.0.1:0")
                assert(err == nil)
                local addr, addr_err = listener:addr()
                assert(addr_err == nil and addr == listener:addr())
                return addr, (assert(assert(net.TcpListener.bind("127.0.0.1:0")):addr()))
                "#,
            )
            .eval()
            .unwrap();
        let parsed: SocketAddr = addr.parse().unwrap();
        assert!(parsed.ip().is_loopback() && parsed.port() != 0, "{addr}");
        assert_ne!(addr, other, "the operating system picks a port for each");
    }

    #[tokio::test]
    async fn a_failure_to_bind_is_a_typed_error_with_the_address() {
        let lua = engine(Life::new());
        let (addr, kind, named, text): (String, String, String, String) = lua
            .load(
                r#"
                held = assert(net.TcpListener.bind("127.0.0.1:0"))
                local addr = assert(held:addr())
                local value, err = net.TcpListener.bind(addr)
                assert(value == nil and err.op == "std:net.TcpListener.bind")
                assert(err.message == addr .. ": " .. err.reason)
                return addr, err.kind, err.addr, tostring(err)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!((kind.as_str(), named.as_str()), ("addr_in_use", addr.as_str()));
        assert!(text.starts_with(&format!("std:net.TcpListener.bind: addr_in_use: {addr}: ")), "{text}");

        let kinds: Vec<String> = lua
            .load(
                r#"
                local out = {}
                for _, addr in { "no port", "", "127.0.0.1", "127.0.0.1:99999", "192.0.2.1:0" } do
                    local value, err = net.TcpListener.bind(addr)
                    assert(value == nil and err.addr == addr, addr)
                    table.insert(out, err.kind)
                end
                return out
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(kinds[..4], ["invalid_input", "invalid_input", "invalid_input", "invalid_input"]);
        assert_eq!(kinds[4], "addr_not_available", "192.0.2.1 is documentation space: no machine has it");
    }

    #[tokio::test]
    async fn a_wrong_argument_is_refused_with_its_place() {
        let lua = engine(Life::new());
        for call in ["net.TcpListener.bind()", "net.TcpListener.bind(8080)", "net.TcpListener.bind({})"] {
            let refused = lua.load(format!("return {call}")).eval::<mlua::Value>().unwrap_err().to_string();
            assert!(refused.contains("std:net.TcpListener.bind: argument #1 'addr'"), "{call}: {refused}");
        }
    }

    /// The engine's shutdown, and `stop_background`, release the socket at once (a replacement can bind the port)
    /// although the script still holds the listener, which then says it is closed. The registry holds a listener
    /// weakly: one the script dropped is freed.
    #[tokio::test]
    async fn the_engines_shutdown_releases_the_socket() {
        for stop in [Life::shutdown, Life::stop_background] {
            let life = Life::new();
            let lua = engine(life.clone());
            let addr: String = lua
                .load(r#"held = assert(net.TcpListener.bind("127.0.0.1:0")); return (assert(held:addr()))"#)
                .eval()
                .unwrap();
            assert!(std::net::TcpListener::bind(&addr).is_err(), "the port is taken");
            stop(&life);
            assert!(port_is_free(&addr), "the port is free");
            let (value, kind, text): (mlua::Value, String, String) = lua
                .load("local value, err = held:addr(); return value, err.kind, tostring(err)")
                .eval()
                .unwrap();
            assert!(value.is_nil());
            assert_eq!(kind, "closed");
            assert_eq!(text, "std:net.TcpListener:addr: closed: the listener is closed");
        }

        let lua = engine(Life::new());
        let addr: String = lua
            .load(r#"local dropped = assert(net.TcpListener.bind("127.0.0.1:0")); return (assert(dropped:addr()))"#)
            .eval()
            .unwrap();
        lua.gc_collect().unwrap();
        assert!(port_is_free(&addr), "a collected listener frees its port");
    }
}
