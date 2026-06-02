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

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:net` surface through the facade.
///
/// `TcpListener` is a constructor sub-table reached as
/// `require("std:net").TcpListener.bind(addr)`. The fallible constructor routes
/// through [`crate::lua::facade::TypeSpec::try_constructor`], which opens the
/// `"TcpListener.bind"` span at `luau::lib::net` and marshals the
/// `(value, error)` idiom with the `"TcpListener.bind"` error prefix — exactly
/// as the hand-written `#[instrument(name = "TcpListener.bind", target =
/// "luau::lib::net")]` registration did. The `Listener` (`addr`) method keeps
/// its own `impl UserData` and is recorded here for the header.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:net")?;

    // `require("std:net").TcpListener.bind(addr)`
    m.type_("TcpListener", |t| {
        t.try_constructor(
            "bind",
            &[("addr", "string")],
            "Bind a TCP socket to `addr` (e.g. `\"0.0.0.0:3000\"`). Returns a listener handle on success.",
            |_, addr: String| bind(&addr).map(|l| Listener::from_arc(Arc::new(l))),
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
    let std_listener =
        std::net::TcpListener::bind(addr).map_err(|e| format!("{addr}: {e}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|e| format!("nonblocking: {e}"))?;
    TcpListener::from_std(std_listener).map_err(|e| format!("from_std: {e}"))
}

/// Lua-facing TCP listener handle. Named `Listener` on the Rust side to
/// avoid collision with `tokio::net::TcpListener` in scope; exposed to
/// Lua as `TcpListener` via the module constructor table.
#[derive(Clone)]
pub struct Listener(Arc<TcpListener>);

impl Listener {
    pub fn from_arc(arc: Arc<TcpListener>) -> Self {
        Self(arc)
    }
    pub fn arc(&self) -> Arc<TcpListener> {
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
        arc.clone()
            .downcast::<TcpListener>()
            .ok()
            .map(Listener::from_arc)
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
