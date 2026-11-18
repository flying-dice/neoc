//! The `app` capability table: the only way a Luau app reaches the host.
//!
//! ```text
//! app.manifest { ... }            identity, window, permissions (validated)
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
    vm::{Shared, ShellEffect},
};
use agentos_application::PortError;
use mlua::Lua;
use std::{future::Future, sync::Arc};

pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>) -> mlua::Result<()> {
    let app = lua.create_table()?;
    app.set("id", sh.app.as_str())?;
    misc::install(lua, sh, &app)?;
    app.set("t", schema::builders(lua)?)?;
    tools::install(lua, sh, &app)?;
    data::install(lua, sh, &app)?;
    app.set("state", crate::state::state_constructor(lua, sh.tracker.clone())?)?;
    *sh.app_table.lock() = Some(app.clone());
    lua.globals().set("app", app)?;
    require::install(lua, sh)
}

/// Run a host call under the wall-clock backstop; the app's CPU slice restarts when it returns (time
/// spent waiting on the host is not app CPU).
pub(crate) async fn host<T>(
    sh: &Shared,
    call: impl Future<Output = Result<T, PortError>>,
) -> mlua::Result<T> {
    sh.slice.pause();
    let result = tokio::time::timeout(sh.limits.host_call_timeout, call).await;
    sh.slice.resume();
    match result {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(mlua::Error::external(e)),
        Err(_) => Err(mlua::Error::runtime(format!(
            "a host call took longer than {:?} and was abandoned",
            sh.limits.host_call_timeout
        ))),
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

pub(crate) fn need(granted: bool, capability: &str, what: &str) -> mlua::Result<()> {
    if granted {
        Ok(())
    } else {
        Err(mlua::Error::runtime(format!(
            "permission denied: {what} needs the `{capability}` permission in the package manifest"
        )))
    }
}

pub(crate) fn push_shell(sh: &Shared, effect: ShellEffect) {
    sh.effects.lock().push(crate::vm::Effect::Shell(effect));
}
