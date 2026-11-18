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
    budget::Stop,
    require,
    vm::{Effect, Shared, ShellEffect},
};
use agentos_application::PortError;
use mlua::Lua;
use std::{
    future::Future,
    sync::{atomic::Ordering, Arc},
};

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
/// spent waiting on the host is not app CPU), but the call's own budget does not: every host call is
/// charged against it, and one is refused (or abandoned while it waits) once the call's budget is spent or
/// the app was unloaded.
///
/// A host call that finished is reported as finished, even when the call's budget ran out while it ran: the
/// app learns that its `commit` went through, and the next interrupt or host call stops it. (Raising here
/// would tell the app a transaction failed that is in the database.)
///
/// Refused inside the app's own coroutine, which cannot wait for the host (see `vm::COROUTINE_GUARD`).
pub(crate) async fn host<T>(
    sh: &Shared,
    call: impl Future<Output = Result<T, PortError>>,
) -> mlua::Result<T> {
    let allowance = sh.limits.allowance();
    let stopped = |stop: Stop| mlua::Error::runtime(stop.message());
    if sh.coroutines_running.load(Ordering::SeqCst) > 0 {
        return Err(mlua::Error::runtime(
            "a host call cannot be made inside a coroutine: make it from the function that resumes the coroutine",
        ));
    }
    sh.budget.charge_host_call(allowance).map_err(stopped)?;
    let limit = sh.limits.host_call_timeout.min(sh.budget.call_wall_left(allowance));
    let waiting = sh.budget.wait_for_host();
    let result = tokio::select! {
        timed = tokio::time::timeout(limit, call) => Some(timed),
        () = sh.budget.retirement().retired() => None,
    };
    drop(waiting);
    let budget_left = sh.budget.check_after_host_call(allowance);
    match result {
        Some(Ok(Ok(v))) => Ok(v),
        failed => {
            budget_left.map_err(stopped)?;
            match failed {
                Some(Ok(Err(e))) => Err(mlua::Error::external(e)),
                Some(Err(_)) => Err(mlua::Error::runtime(format!(
                    "a host call took longer than {limit:?} and was abandoned"
                ))),
                _ => Err(stopped(Stop::Unloaded)),
            }
        }
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
