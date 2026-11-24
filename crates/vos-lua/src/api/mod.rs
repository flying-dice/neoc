//! The `app` capability table: the only way an app reaches the host.
//!
//! ```text
//! app.id                          the app's id
//! app.db / app.kv                 data (migrations, statements, transactions) and a small key-value store
//! app.files.documents:read(path)  the app's managed files (documents, cache, temp)
//! app.tools.register { ... }      agent tools (alias: app.tool { ... })
//! app.t.string { ... } ...        input schema builders
//! app.notify { ... }              notifications
//! app.emit(name, data)            an event for the app's open pages
//! app:openWindow(name?, params?)  the app's window, opened or brought forward
//! app.agents:list()               the agent contacts
//! app.log(message)                a line in the host's log
//! ```

mod data;
mod files;
mod misc;
mod schema;
mod tools;

use crate::{require, vm::Shared};
use neoc_vm2::lifecycle::{async_function, Role};
use neoc_vm2::mlua;
use neoc_vm2::mlua::{FromLuaMulti, Function, IntoLuaMulti, Lua};
use std::{future::Future, sync::Arc};
use vos_application::PortError;

type Sh = Arc<Shared>;

/// Install the `app` table and `require` into the app's environment (they are globals of its chunks, not of
/// the engine).
pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    let app = lua.create_table()?;
    app.set("id", sh.app.as_str())?;
    misc::install(lua, sh, &app)?;
    app.set("t", schema::builders(lua)?)?;
    tools::install(lua, sh, &app)?;
    data::install(lua, sh, &app)?;
    files::install(lua, sh, &app)?;
    *sh.app_table.lock() = Some(app.clone());
    env.set("app", app)?;
    if sh.test_hooks {
        install_test_hook(lua, sh, env)?;
    }
    require::install(lua, sh, env)
}

/// `hook(name, fn)`, for the adapter's tests ([`crate::VmConfig::test_hooks`]): a function `AppVm::emit` calls.
/// Registering one changes the app's wiring, so a `readOnly` call may not.
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

/// An async host function of the app's own state: it takes the turn before its body runs ([`Role::Vm`]), and the
/// body gets the app's shared state. Every function of `app.db`, `app.kv` and `app.files` is made here.
pub(crate) fn vm_function<A, R, Fut>(
    lua: &Lua,
    sh: &Sh,
    body: impl Fn(Sh, Lua, A) -> Fut + Send + 'static,
) -> mlua::Result<Function>
where
    A: FromLuaMulti,
    R: IntoLuaMulti + Send + 'static,
    Fut: Future<Output = mlua::Result<R>> + Send + 'static,
{
    let sh = sh.clone();
    async_function(lua, Role::Vm, move |lua, args| body(sh.clone(), lua, args))
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
