//! The `app` capability table: the only way an app reaches the host.
//!
//! ```text
//! app.manifest { ... }            identity, window, icon (validated)
//! app.window(name, spec)          a window kind (rendered by the view layer)
//! app.state { ... }               shared state, reads tracked
//! app.db / app.kv                 data (sources, migrations, exec) and a small key-value store
//! app.tools.register { ... }      agent tools (alias: app.tool { ... })
//! app.t.string { ... } ...        input schema builders
//! app.notify { ... }              notifications
//! app.shell.badge / command / deepLink
//! ```

mod data;
mod misc;
mod schema;
mod tools;

use crate::{
    require,
    vm::{Effect, Shared, ShellEffect},
};
use agentos_application::PortError;
use agentos_luavm::mlua;
use agentos_luavm::mlua::Lua;
use std::{future::Future, sync::Arc};

/// Install the `app` table and `require` into the app's environment (they are globals of its chunks, not of
/// the engine).
pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    let app = lua.create_table()?;
    app.set("id", sh.app.as_str())?;
    misc::install(lua, sh, &app)?;
    app.set("t", schema::builders(lua)?)?;
    tools::install(lua, sh, &app)?;
    data::install(lua, sh, &app)?;
    app.set("state", crate::state::state_constructor(lua, sh.tracker.clone())?)?;
    *sh.app_table.lock() = Some(app.clone());
    env.set("app", app)?;
    require::install(lua, sh, env)
}

/// Run a host call (a database call, the agent roster) from inside a `lifecycle::Role::Vm` function, which
/// holds the turn already. It runs to its end, however long that takes, unless the app is unloaded or replaced
/// meanwhile: the call is then abandoned and the app's code gets an error, so a loop of host calls in an
/// unloaded app ends.
pub(crate) async fn host<T>(
    sh: &Shared,
    call: impl Future<Output = Result<T, PortError>>,
) -> mlua::Result<T> {
    tokio::select! {
        done = call => done.map_err(mlua::Error::external),
        () = sh.life.closed() => Err(mlua::Error::runtime("the app was unloaded")),
    }
}

/// Render must be pure: anything that changes the world raises while a render is open.
pub(crate) fn check_effects_allowed(sh: &Shared, what: &str) -> mlua::Result<()> {
    if sh.tracker.lock().is_pure() {
        return Err(mlua::Error::runtime(format!(
            "{what} is not allowed while rendering: render must be pure"
        )));
    }
    Ok(())
}

/// Queue a shell declaration. A later declaration of the same kind replaces the earlier one (the shell only
/// ever needs the last badge, command list or link list), so a loop of them cannot grow the queue.
pub(crate) fn push_shell(sh: &Shared, effect: ShellEffect) {
    let mut effects = sh.effects.lock();
    effects.retain(|queued| match queued {
        Effect::Shell(old) => std::mem::discriminant(old) != std::mem::discriminant(&effect),
        Effect::Notify(_) | Effect::OpenWindow { .. } => true,
    });
    effects.push(Effect::Shell(effect));
}
