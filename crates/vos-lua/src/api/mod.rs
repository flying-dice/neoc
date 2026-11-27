//! The `app` API: the only way an app reaches the host. It is the mlua-ir module `vos:app` ([`app`]), so its Luau
//! and TypeScript declarations are read from this Rust (`crate::declarations`), and it is also the global `app`. The
//! RPC bus is its own module beside it, `vos:rpc` ([`rpc`]), the global `RPC`: the same four functions an app's page
//! has, in the house style (snake case).
//!
//! ```text
//! app.id                          the app's id
//! app.db / app.kv                 data (migrations, statements, transactions) and a small key-value store
//! app.files.documents:read(path)  the app's managed files (documents, cache, temp)
//! app.tools.all() / .call(...)    the tools of every running app, for an agent of the app's own
//! app.tools.lease(agent, opts?)   an MCP bearer token for that agent, to call them over `/mcp` (.release(token))
//! app.notify { ... }              a notification
//! app.open_window(params?)         the app's window, opened or brought forward
//! app.agents:list()               the agent contacts
//! app.log(message)                a line in the host's log
//!
//! RPC.send_request(app, method, p?)     a request on the RPC bus: the first handler to answer wins
//! RPC.on_request(method, fn)            serve one of this app's methods on the bus
//! RPC.send_notification(name, p?)       one of this app's notifications, to whoever listens
//! RPC.on_notification(app, name, fn)    hear an app's notification
//! ```
//!
//! The module is stateless: every function finds the app it belongs to in its Lua state ([`shared`]), which is one
//! VM per app.

// What mlua-ir's macros imply (as in neoc-vm's modules): a script's value is read before the call, so a function
// takes it owned; the macro creates the async functions itself, and the VM library's host runs each as a lifecycle
// leaf with the role its `#[host("Vm")]` names. A method is one for the script (`app.db:exec(...)`), whether or
// not its Rust needs the value it is called on: the values under `app` hold nothing, the app is found in the VM.
#![allow(
    clippy::needless_pass_by_value,
    clippy::disallowed_methods,
    clippy::unused_async,
    clippy::unused_self,
    clippy::trivially_copy_pass_by_ref
)]

pub(crate) mod context;
mod data;
mod files;
mod json;
mod misc;
mod peers;
mod tools;

use crate::{require, vm::Shared};
use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua};
use neoc_vm::modules;
use std::{future::Future, sync::Arc};
use vos_application::PortError;

pub(crate) use context::Context;

type Sh = Arc<Shared>;

/// The name the `app` API is required under.
pub(crate) const MODULE: &str = "vos:app";

/// The name the RPC bus is required under (it is also the global `RPC`).
pub(crate) const RPC_MODULE: &str = "vos:rpc";

/// The `app` API.
///
/// `local app = require("vos:app")` in an app's backend; it is also the global `app`. The host provides it (one VM
/// per app). The backend serves its methods on the bus (`RPC.on_request`).
#[mlua_ir::module("vos:app")]
#[allow(non_upper_case_globals, non_camel_case_types)] // the names are the script's: `app.db`, `app.kv`
pub mod app {
    use super::data::{Db, Kv};
    use super::files::Files;
    use super::misc::{self, Agents, Notification, ThisApp, WindowParams};
    use mlua_ir::Object;
    use neoc_vm::mlua::{self, Lua};

    /// The app's id.
    pub const id: ThisApp = ThisApp;

    /// The app's database: migrations, statements and transactions.
    pub const db: Db = Db;

    /// A small durable key-value store (JSON values).
    pub const kv: Kv = Kv;

    /// The app's managed files.
    pub const files: Files = Files;

    /// The agent contacts.
    pub const agents: Agents = Agents;

    /// The tools of every running app, for an app that offers them to an agent of its own: `app.tools.all()` and
    /// `app.tools.call(name, agent, args)`, or over the host's `/mcp` with `app.tools.lease(agent, opts?)` and
    /// `app.tools.release(token)`.
    #[derive(Clone, Copy, Debug, Object)]
    pub struct tools;

    /// Open the app's window on the person's screen, or bring it forward if it is open (every connected shell is
    /// asked; with none connected nothing happens). An app has one window, the frame of its page; `params` is
    /// JSON handed to it. A change to the world like `notify`: a read-only call may not.
    pub fn open_window(lua: &Lua, params: Option<WindowParams>) -> mlua::Result<()> {
        misc::open_window(&*super::shared(lua)?, params)
    }

    /// `app.notify { title, body }`: tell the person something now. It reaches the clients connected now; one that
    /// is not connected never gets it (nothing is kept for later). The host sends the app's id with it, never text
    /// from the app, and each shell client draws it as a banner above its windows with the app's name and icon
    /// from its own app list: not on a client where one of the app's windows is in front, on every other client,
    /// with or without a window open. A page with no shell (the app opened on its own) draws none. The title is
    /// shown on one line (line breaks and control characters become spaces; direction overrides and characters
    /// that draw nothing are removed, but not the joiners and variation selectors that make an emoji or a word)
    /// and both are text, never markup.
    ///
    /// Raises: a key other than `title` and `body`, no title, a title that is blank or over 200 characters, a body
    /// over 2000, a value that is not a string, a call inside a read-only call, and a 65th while 64 from the same
    /// call are still waiting. The host delivers waiting notifications whenever a call of the app ends, and when a
    /// call waits (`task.wait`, a request, a task or request handler pausing, or a tool that has notified and then
    /// works on), so a tool that notifies half way is heard then, not when it returns. The limit guards against a
    /// runaway loop; it is not a quota to plan around.
    pub fn notify(lua: &Lua, notification: Notification) -> mlua::Result<()> {
        misc::notify(&*super::shared(lua)?, notification)
    }

    /// Write a line to the host's log, with the app's id.
    pub fn log(lua: &Lua, message: String) -> mlua::Result<()> {
        let sh = super::shared(lua)?;
        tracing::info!(app = %sh.app, "{message}");
        Ok(())
    }
}

/// The RPC bus: `local RPC = require("vos:rpc")`, and the global `RPC`. A page has the same four functions on its
/// global `RPC`. Every call names its app; only an app serves its own methods and sends its own notifications.
#[mlua_ir::module("vos:rpc")]
pub mod rpc {
    use super::json::Json;
    use super::peers::{
        self, Listener, NotificationParams, Params, RequestError, RequestHandler, RequestResult,
    };
    use mlua_ir::Unknown;
    use neoc_vm::mlua::{self, Lua};

    /// Send a request on the RPC bus to `app` (this one's id included): every handler registered for `method`, a
    /// backend's (`on_request`) or an open page's, is asked, and the first answer wins (the others
    /// are cancelled). The handler is told this app called (`ctx.actor.kind` is `"app"`). The result is
    /// `(result, nil)`, or `(nil, err)` when the app is not running, nothing handles the method, or the handler
    /// failed. Raises for an id that is not an app's, a method name that is not 1 to 128 characters, and inside a
    /// read-only call. It waits without the app's turn.
    #[host("Wait")]
    pub async fn send_request(
        lua: Lua,
        app: String,
        method: String,
        params: Option<Params>,
    ) -> mlua::Result<Result<Json<Unknown<RequestResult>>, RequestError>> {
        peers::send_request(lua, app, method, params).await
    }

    /// Serve one of this app's methods on the RPC bus: `fn(params, ctx)` answers a request for `method`, from the
    /// app's page, another app or an agent. An open page of the app may serve it too: the first answer wins. A public
    /// method (no leading `_`) is an agent tool too, described by the app's contract. Raises for a method
    /// this backend already serves, and inside a read-only call.
    pub fn on_request(lua: &Lua, method: String, handler: RequestHandler) -> mlua::Result<()> {
        peers::on_request(lua, method, handler)
    }

    /// Send one of this app's notifications on the RPC bus: to the app's open pages and backend listeners and, unless
    /// it is a `_name`, to every other app that listens. Nothing waits for it. `name` is 1 to 64 letters, digits and `. _ : -` (not
    /// `host:...`), `params` is JSON of at most 64 KiB. A change to the world like `app.notify`: a read-only call may not.
    pub fn send_notification(
        lua: &Lua,
        name: String,
        params: Option<NotificationParams>,
    ) -> mlua::Result<()> {
        peers::send_notification(lua, name, params)
    }

    /// Hear an app's notification (this one's too: what its page sends): `fn(params, ctx)` runs, as a call of this
    /// app's own, each time `app` sends `name`, with `ctx.actor` that app. Listeners run in the order they were added.
    /// Raises for an id that is not an app's, another app's `_name` (private: it never leaves that app) or a `host:`
    /// name, and inside a read-only call.
    pub fn on_notification(lua: &Lua, app: String, name: String, listener: Listener) -> mlua::Result<()> {
        peers::on_notification(lua, app, name, listener)
    }
}

/// The app whose VM `lua` is: what [`install`] put there. The module is linked into every engine of the process,
/// so a state that is not an app's VM (a `std:thread` worker is never offered `vos:app`) has none.
pub(crate) fn shared(lua: &Lua) -> mlua::Result<Sh> {
    lua.app_data_ref::<Sh>()
        .map(|sh| Sh::clone(&sh))
        .ok_or_else(|| mlua::Error::runtime("the app API is only there in an app's own VM"))
}

/// Install the `app` table and `require` into the app's environment (they are globals of its chunks, not of
/// the engine).
pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    lua.set_app_data(sh.clone());
    let table = modules::build(lua, MODULE)
        .ok_or_else(|| mlua::Error::runtime("the app API is not linked into this binary"))??;
    *sh.app_table.lock() = Some(table.clone());
    env.set("app", table)?;
    env.set("RPC", rpc_table(lua)?)?;
    if sh.test_hooks {
        install_test_hook(lua, sh, env)?;
    }
    require::install(lua, sh, env)
}

/// The `RPC` table: the module holds nothing, so each build is the same API.
pub(crate) fn rpc_table(lua: &Lua) -> mlua::Result<mlua::Table> {
    modules::build(lua, RPC_MODULE)
        .ok_or_else(|| mlua::Error::runtime("the RPC bus is not linked into this binary"))?
}

/// `hook(name, fn)`, for the adapter's tests ([`crate::VmConfig::test_hooks`]): a function `AppVm::emit` calls.
/// Registering one changes the app's wiring, so a read-only call may not.
fn install_test_hook(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    let s = sh.clone();
    env.set(
        "hook",
        lua.create_function(move |_, (name, f): (String, Function)| {
            s.writable("hook")?;
            s.handlers.lock().insert(name, f);
            Ok(())
        })?,
    )
}

/// Run a host call (a database call) from inside a `lifecycle::Role::Vm` function, which holds the turn already.
/// It runs to its end, however long that takes, unless the app is unloaded or replaced meanwhile: the call is then
/// abandoned and the app's code gets an error, so a loop of host calls in an unloaded app ends. If Lua could not
/// suspend at the function (it was called from a `string.gsub` callback, say) the call is dropped as soon as the
/// script goes on, and whatever it holds, a database lock included, is released (`lifecycle`, rule 3).
pub(crate) async fn host<T>(
    sh: &Shared,
    call: impl Future<Output = Result<T, PortError>>,
) -> mlua::Result<T> {
    tokio::select! {
        done = call => done.map_err(mlua::Error::external),
        () = sh.life.closed() => Err(mlua::Error::runtime("the app was unloaded")),
    }
}
