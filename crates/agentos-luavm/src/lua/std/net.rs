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
/// as the hand-written `#[instrument(name = "TcpListener.bind", target =
/// "lua::lib::net")]` registration did. The `Listener` (`addr`) method keeps
/// its own `impl UserData` and is recorded here for the header.
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
    std_listener.set_nonblocking(true).map_err(|e| format!("nonblocking: {e}"))?;
    TcpListener::from_std(std_listener).map_err(|e| format!("from_std: {e}"))
}

/// The socket behind a [`Listener`]: taken out (and so dropped, which closes it once the tasks that were
/// accepting on it are gone too) when the app is unloaded.
#[derive(Default)]
pub struct ListenerSlot(parking_lot::Mutex<Option<Arc<TcpListener>>>);

impl ListenerSlot {
    fn close(&self) {
        self.0.lock().take();
    }
}

/// Lua-facing TCP listener handle. Named `Listener` on the Rust side to
/// avoid collision with `tokio::net::TcpListener` in scope; exposed to
/// Lua as `TcpListener` via the module constructor table.
///
/// Dropping a Lua VM frees its listeners, but an app that is unloaded or replaced may still be referenced for
/// a moment (a call in flight, the next version loading): the socket is therefore released by the app's
/// [`Life`] at shutdown, so a replacement can bind the same port.
#[derive(Clone)]
pub struct Listener(Arc<ListenerSlot>);

impl Listener {
    fn new(lua: &Lua, listener: TcpListener) -> Self {
        let slot = Arc::new(ListenerSlot(parking_lot::Mutex::new(Some(Arc::new(listener)))));
        let weak = Arc::downgrade(&slot);
        Life::of(lua).on_shutdown(move || {
            if let Some(slot) = weak.upgrade() {
                slot.close();
            }
        });
        Self(slot)
    }
    /// The socket, or `None` once the app was shut down.
    pub fn arc(&self) -> Option<Arc<TcpListener>> {
        self.0 .0.lock().clone()
    }
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.arc()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "the listener is closed"))?
            .local_addr()
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
        methods.add_method("addr", |_, this, (): ()| match this.local_addr() {
            Ok(a) => Ok(a.to_string()),
            Err(e) => Err(mlua::Error::external(e)),
        });
    }
}
