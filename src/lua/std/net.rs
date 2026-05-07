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
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    // `require("std:net").TcpListener.bind(addr)`
    let tcp_listener_t = lua.create_table()?;
    tcp_listener_t.set("bind", lua.create_function(make_listener)?)?;
    t.set("TcpListener", tcp_listener_t)?;

    Ok(t)
}

#[instrument(name = "TcpListener.bind", target = "luau::lib::net", skip_all)]
fn make_listener(lua: &Lua, addr: String) -> mlua::Result<(mlua::Value, mlua::Value)> {
    match bind(&addr) {
        Ok(listener) => ok(lua, Listener::from_arc(Arc::new(listener))),
        Err(e) => from_err(lua, "TcpListener.bind", e),
    }
}

fn bind(addr: &str) -> Result<TcpListener, String> {
    let std_listener =
        std::net::TcpListener::bind(addr).map_err(|e| format!("bind {addr}: {e}"))?;
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
