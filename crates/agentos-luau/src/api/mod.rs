//! The `app` capability table: the only way an app reaches the host.
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

/// Run a host call under the wall-clock backstop; the app's CPU slice restarts when it returns (time
/// spent waiting on the host is not app CPU), but the call's own budget does not: every host call is
/// charged against it, and one is refused (or abandoned while it waits) once the call's budget is spent or
/// the app was unloaded.
///
/// A host call that finished is reported as finished, even when the call's budget ran out while it ran: the
/// app learns that its `commit` went through, and the next tick or host call stops it. (Raising here
/// would tell the app a transaction failed that is in the database.)
///
/// The wait is a guard ([`crate::budget::HostWait`]): however this future ends (it finished, it timed out, or
/// it was dropped half done because the host call failed where Lua 5.1 cannot yield, in a `table.sort`
/// comparator or a `gsub` callback), the slice starts counting again.
pub(crate) async fn host<T>(
    sh: &Shared,
    call: impl Future<Output = Result<T, PortError>>,
) -> mlua::Result<T> {
    let allowance = sh.limits.allowance();
    let stopped = |stop: Stop| mlua::Error::runtime(stop.message());
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

#[cfg(test)]
mod tests {
    use super::host;
    use crate::vm::{AppVm, Limits, VmConfig};
    use agentos_application::ports::AppPackage;
    use agentos_domain::AppId;
    use agentos_sqlite::AppDb;
    use std::{collections::BTreeMap, sync::Arc, time::Duration};

    async fn vm_with(limits: Limits) -> AppVm {
        let files = BTreeMap::from([("app.luau".to_owned(), b"app.on('x', function() end)".to_vec())]);
        let package = AppPackage { entry: "app.luau".into(), files };
        let config = VmConfig { limits, ..VmConfig::default() };
        let db = Arc::new(AppDb::open(None, &[]).expect("in-memory database"));
        AppVm::load(&AppId::parse("probe").expect("id"), package, db, config).await.expect("loads")
    }

    #[tokio::test]
    async fn a_host_call_that_finished_is_reported_even_when_the_calls_wall_clock_ran_out_meanwhile() {
        let vm = vm_with(Limits { call_wall: Duration::from_millis(100), ..Limits::default() }).await;
        let sh = vm.shared().clone();
        sh.budget.begin();
        // finishes (blocking its thread) after the call's deadline, without ever yielding to the timeout
        let committed = host(&sh, async {
            std::thread::sleep(Duration::from_millis(250));
            Ok(7)
        })
        .await;
        assert_eq!(committed.expect("the host call went through: the app is told so"), 7);
        // and the app is stopped by what comes next
        let next = host(&sh, async { Ok(8) }).await.expect_err("the call's budget is spent");
        assert!(next.to_string().contains("cpu budget exceeded"), "{next}");
    }

    #[tokio::test]
    async fn a_host_call_that_failed_after_the_deadline_reports_the_budget_not_its_own_error() {
        let vm = vm_with(Limits { call_wall: Duration::from_millis(100), ..Limits::default() }).await;
        let sh = vm.shared().clone();
        sh.budget.begin();
        let failed: mlua::Result<()> = host(&sh, async {
            std::thread::sleep(Duration::from_millis(250));
            Err(agentos_application::PortError::Rejected("late".into()))
        })
        .await;
        assert!(failed.expect_err("an error").to_string().contains("cpu budget exceeded"));
    }
}
