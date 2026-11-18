//! Use-case tests for `AppService` against the in-memory port fakes.

use crate::{ports::ServerEvent, services::AppStatus, testing::Fakes, AppError};
use agentos_domain::{
    Actor, AgentId, AgentIdentity, AppId, Capabilities, DomainError, Permission, QualifiedToolName,
};
use serde_json::json;

fn agent(grants: Vec<Permission>) -> AgentIdentity {
    AgentIdentity { id: AgentId("claude".into()), name: "Claude".into(), grants }
}

#[tokio::test]
async fn install_verifies_files_then_wires_data_runtime_tools_and_announces() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("add_note", "x")]);
    let svc = f.service();
    let id = svc.install_from_url(&url).await.unwrap();
    assert_eq!(id.as_str(), "notes");
    assert_eq!(f.registry.names(), vec!["notes__add_note".to_string()]);
    assert_eq!(f.events.take(), vec![ServerEvent::AppInstalled(id.clone())]);
    assert_eq!(f.store.opened_with_migrations("notes"), Some(1));
    assert!(matches!(svc.install_from_url(&url).await, Err(AppError::AlreadyInstalled(_))));
    assert_eq!(f.background.alive(), 1, "the live-source pump runs while the app does");
    let rec = f.installed.records();
    assert_eq!(
        (rec[0].source_url.as_str(), rec[0].enabled, rec[0].installed_at_ms),
        (url.as_str(), true, 1_000)
    );
}

#[tokio::test]
async fn the_package_carries_every_luau_module_and_the_granted_capabilities() {
    let f = Fakes::new();
    let url = f.add_app_with(
        "calc",
        &[("t", "x")],
        &[("lib/calc.luau", "return 1"), ("notes.txt", "not luau")],
        vec![Permission::Data, Permission::Notify],
    );
    f.service().install_from_url(&url).await.unwrap();
    let package = f.runtime.package("calc").unwrap();
    assert_eq!(package.entry, "main.luau");
    assert_eq!(package.files.keys().collect::<Vec<_>>(), vec!["lib/calc.luau", "main.luau"]);
    let caps = f.runtime.capabilities("calc").unwrap();
    assert_eq!(caps, Capabilities { data: true, notify: true, shell: false });
}

#[tokio::test]
async fn tampered_file_is_rejected_before_any_state_changes() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("t", "x")]);
    f.tamper("notes");
    let err = f.service().install_from_url(&url).await.unwrap_err();
    assert!(matches!(err, AppError::Domain(DomainError::ChecksumMismatch { .. })), "{err:?}");
    assert!(f.registry.names().is_empty());
    assert_eq!(f.store.opened_with_migrations("notes"), None);
    assert!(f.installed.records().is_empty());
}

#[tokio::test]
async fn an_app_that_fails_to_load_is_not_recorded_and_nothing_is_published() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("t", "x")]);
    f.runtime.fail_load("notes", true);
    let err = f.service().install_from_url(&url).await.unwrap_err();
    assert!(err.to_string().contains("notes/app.luau:3"), "{err}");
    assert!(f.registry.names().is_empty() && f.installed.records().is_empty());
    assert_eq!(f.background.alive(), 0);
}

#[tokio::test]
async fn uninstall_retracts_tools_unloads_vm_forgets_the_record_and_optionally_purges() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("add_note", "x")]);
    let svc = f.service();
    let id = svc.install_from_url(&url).await.unwrap();
    svc.uninstall(&id, true).await.unwrap();
    assert!(f.registry.names().is_empty());
    assert_eq!(f.runtime.loaded(), Vec::<String>::new());
    assert!(f.store.purged("notes"));
    assert!(f.installed.records().is_empty(), "an uninstalled app is not restored at the next start");
    assert_eq!(f.background.alive(), 0, "the pump stops");
    assert!(matches!(svc.uninstall(&id, false).await, Err(AppError::NotInstalled(_))));
}

#[tokio::test]
async fn shutdown_unloads_every_app_keeps_data_and_records() {
    let f = Fakes::new();
    let svc = f.service();
    svc.install_from_url(&f.add_app("notes", &[("add_note", "x")])).await.unwrap();
    svc.install_from_url(&f.add_app("todo", &[("add_task", "x")])).await.unwrap();
    f.events.take();

    svc.shutdown().await;
    assert!(f.registry.names().is_empty());
    assert!(f.runtime.loaded().is_empty());
    assert!(!f.store.purged("notes"), "shutdown keeps app data");
    assert_eq!(f.installed.records().len(), 2, "and remembers the installs");
    assert!(f.events.take().is_empty(), "clients are not told the apps were uninstalled");
    assert!(svc.installed_apps().is_empty());
    assert_eq!(f.background.alive(), 0);
}

#[tokio::test]
async fn tool_calls_are_permission_checked_per_agent_and_run_as_that_agent() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("add_note", "x")]);
    let svc = f.service();
    let id = svc.install_from_url(&url).await.unwrap();
    let name = QualifiedToolName::parse("notes__add_note").unwrap();

    f.clock.advance(500);
    let ok =
        svc.call_tool(&agent(vec![Permission::CallTools(id.clone())]), &name, json!({"a":1})).await.unwrap();
    assert_eq!(ok, json!({"echo": {"a":1}}));
    let ctx = f.runtime.last_ctx().unwrap();
    assert_eq!((ctx.actor, ctx.now_ms), (Actor::agent("claude", "Claude"), 1_500));
    let denied = svc.call_tool(&agent(vec![]), &name, json!({})).await.unwrap_err();
    assert!(matches!(denied, AppError::Domain(DomainError::PermissionDenied { .. })));
    assert_eq!(f.runtime.calls(), 1, "a denied call must never reach the VM");
}

// ---- persistence across restarts --------------------------------------------------------------------

#[tokio::test]
async fn installed_apps_come_back_after_a_restart() {
    let f = Fakes::new();
    let first = f.service();
    first.install_from_url(&f.add_app("notes", &[("add_note", "x")])).await.unwrap();
    first.shutdown().await;
    assert!(f.registry.names().is_empty());

    let second = f.service(); // a new process over the same system database
    let apps = second.restore_installed().await.unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].status, AppStatus::Running);
    assert_eq!(f.registry.names(), vec!["notes__add_note".to_string()]);
    assert_eq!(f.runtime.loads("notes"), 2);
    assert_eq!(f.background.alive(), 1);
    assert_eq!(second.restore_installed().await.unwrap().len(), 1, "restoring twice changes nothing");
    assert_eq!(f.runtime.loads("notes"), 2);
}

#[tokio::test]
async fn a_failing_app_is_marked_failed_and_the_others_still_start() {
    let f = Fakes::new();
    let first = f.service();
    first.install_from_url(&f.add_app("notes", &[("a", "x")])).await.unwrap();
    first.install_from_url(&f.add_app("todo", &[("b", "x")])).await.unwrap();
    first.shutdown().await;
    f.events.take();

    f.runtime.fail_load("notes", true);
    let second = f.service();
    let apps = second.restore_installed().await.unwrap();
    let by_id = |id: &str| apps.iter().find(|a| a.id.as_str() == id).unwrap().status.clone();
    assert!(matches!(by_id("notes"), AppStatus::Failed(m) if m.contains("boom")));
    assert_eq!(by_id("todo"), AppStatus::Running);
    assert_eq!(f.registry.names(), vec!["todo__b".to_string()], "the failed app publishes no tools");
    assert!(
        matches!(f.events.take().as_slice(), [ServerEvent::AppFailed { app, .. }] if app.as_str() == "notes")
    );

    let name = QualifiedToolName::parse("notes__a").unwrap();
    let err = second.call_tool(&agent(vec![Permission::CallAnyTool]), &name, json!({})).await.unwrap_err();
    assert!(matches!(err, AppError::NotRunning { .. }), "{err:?}");

    // fixed: a reload brings it back
    f.runtime.fail_load("notes", false);
    second.reload(&AppId::parse("notes").unwrap()).await.unwrap();
    assert_eq!(second.status(&AppId::parse("notes").unwrap()), Some(AppStatus::Running));
    assert!(f.registry.names().contains(&"notes__a".to_string()));
}

#[tokio::test]
async fn a_source_that_is_gone_fails_the_app_not_the_host() {
    let f = Fakes::new();
    let first = f.service();
    first.install_from_url(&f.add_app("notes", &[("a", "x")])).await.unwrap();
    first.shutdown().await;
    f.fetcher.0.lock().unwrap().remove("mem://notes/app-ref.json");
    let apps = f.service().restore_installed().await.unwrap();
    assert!(matches!(apps[0].status, AppStatus::Failed(_)));
}

// ---- enable / disable / delete data -----------------------------------------------------------------

#[tokio::test]
async fn disable_hides_the_app_from_agents_keeps_data_and_survives_a_restart() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app("notes", &[("add_note", "x")])).await.unwrap();
    f.events.take();

    svc.disable(&id).await.unwrap();
    assert!(f.registry.names().is_empty(), "agent access is blocked");
    assert!(f.runtime.loaded().is_empty(), "the VM is unloaded");
    assert!(!f.store.purged("notes"), "data is kept");
    assert_eq!(f.background.alive(), 0);
    assert_eq!(svc.status(&id), Some(AppStatus::Disabled));
    assert!(!f.installed.records()[0].enabled);
    assert_eq!(f.events.take(), vec![ServerEvent::AppDisabled(id.clone())]);
    svc.disable(&id).await.unwrap(); // idempotent

    let name = QualifiedToolName::parse("notes__add_note").unwrap();
    let err = svc.call_tool(&agent(vec![Permission::CallAnyTool]), &name, json!({})).await.unwrap_err();
    assert!(matches!(err, AppError::NotRunning { state, .. } if state == "disabled"));

    svc.shutdown().await;
    let again = f.service();
    assert_eq!(again.restore_installed().await.unwrap()[0].status, AppStatus::Disabled);
    assert!(f.runtime.loaded().is_empty(), "a disabled app is not loaded at start");

    again.enable(&id).await.unwrap();
    assert_eq!(f.registry.names(), vec!["notes__add_note".to_string()]);
    assert!(f.installed.records()[0].enabled);
    assert_eq!(again.status(&id), Some(AppStatus::Running));
    again.enable(&id).await.unwrap(); // idempotent
    assert_eq!(f.runtime.loads("notes"), 2);
}

#[tokio::test]
async fn enabling_an_app_that_no_longer_loads_marks_it_failed_and_keeps_its_record() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app("notes", &[("a", "x")])).await.unwrap();
    svc.disable(&id).await.unwrap();
    f.runtime.fail_load("notes", true);
    assert!(svc.enable(&id).await.is_err());
    assert!(matches!(svc.status(&id), Some(AppStatus::Failed(_))));
    assert_eq!(f.installed.records().len(), 1);
}

#[tokio::test]
async fn delete_data_purges_and_reloads_a_running_app_on_an_empty_database() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app("notes", &[("add_note", "x")])).await.unwrap();
    svc.delete_data(&id).await.unwrap();
    assert!(f.store.purged("notes"));
    assert_eq!(f.runtime.loads("notes"), 2, "reloaded so its migrations run on the fresh database");
    assert_eq!(f.store.opened_with_migrations("notes"), Some(1));
    assert_eq!(svc.status(&id), Some(AppStatus::Running));
    assert_eq!(f.registry.names(), vec!["notes__add_note".to_string()]);

    svc.disable(&id).await.unwrap();
    svc.delete_data(&id).await.unwrap();
    assert_eq!(f.runtime.loads("notes"), 2, "a disabled app stays unloaded");
    assert!(matches!(svc.delete_data(&AppId::parse("ghost").unwrap()).await, Err(AppError::NotInstalled(_))));
}

// ---- reload -------------------------------------------------------------------------------------------

#[tokio::test]
async fn reload_rebuilds_the_vm_and_keeps_the_database() {
    let f = Fakes::new();
    let svc = f.service();
    let url = f.add_app("notes", &[("old_tool", "x")]);
    let id = svc.install_from_url(&url).await.unwrap();

    f.add_app("notes", &[("new_tool", "x")]); // the developer saved a change
    svc.reload(&id).await.unwrap();
    assert_eq!(f.registry.names(), vec!["notes__new_tool".to_string()]);
    assert_eq!(f.runtime.loads("notes"), 2);
    assert!(!f.store.purged("notes"));
    assert_eq!(f.background.alive(), 1, "one pump: the old one stopped");
}

#[tokio::test]
async fn a_reload_that_does_not_load_keeps_the_running_version() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app("notes", &[("good", "x")])).await.unwrap();
    f.runtime.fail_load("notes", true);
    let err = svc.reload(&id).await.unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert_eq!(svc.status(&id), Some(AppStatus::Running), "the previous version keeps serving");
    assert_eq!(f.registry.names(), vec!["notes__good".to_string()]);
}

#[tokio::test]
async fn sync_registries_installs_the_missing_and_skips_the_known() {
    let f = Fakes::new();
    let a = f.add_app("notes", &[("a", "x")]);
    let b = f.add_app("todo", &[("b", "x")]);
    let svc = f.service();
    svc.sync_registries(std::slice::from_ref(&a)).await;
    svc.sync_registries(&[a, b, "mem://missing/app-ref.json".into()]).await;
    assert_eq!(svc.installed_apps().iter().map(AppId::as_str).collect::<Vec<_>>(), vec!["notes", "todo"]);
    assert_eq!(f.runtime.loads("notes"), 1, "an installed app is not reinstalled");
}
