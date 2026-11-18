//! Use-case tests for `AppService` against the in-memory port fakes.

use crate::{
    ports::{InstalledAppStore, ServerEvent},
    services::AppStatus,
    testing::Fakes,
    AppError,
};
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
    assert_eq!(caps, Capabilities { data: true, notify: true, shell: false, agents: false });
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
async fn ensure_installed_installs_the_missing_and_skips_the_known() {
    let f = Fakes::new();
    let a = f.add_app("notes", &[("a", "x")]);
    let b = f.add_app("todo", &[("b", "x")]);
    let svc = f.service();
    svc.ensure_installed(std::slice::from_ref(&a)).await;
    svc.ensure_installed(&[a, b, "mem://missing/app-ref.json".into()]).await;
    assert_eq!(svc.installed_apps().iter().map(AppId::as_str).collect::<Vec<_>>(), vec!["notes", "todo"]);
    assert_eq!(f.runtime.loads("notes"), 1, "an installed app is not reinstalled");
}

#[tokio::test]
async fn preview_reads_the_app_ref_only_and_says_whether_it_is_installed() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("add_note", "x")]);
    let svc = f.service();
    let before = svc.preview(&url).await.unwrap();
    assert_eq!(
        (before.id.as_str(), before.name.as_str(), before.version.as_str()),
        ("notes", "notes", "1.0.0")
    );
    assert_eq!(before.source, "notes", "the host part of mem://notes/...");
    assert!(before.installed.is_none());
    assert_eq!(f.runtime.loads("notes"), 0, "nothing ran");
    assert!(f.installed.records().is_empty(), "nothing was installed");

    svc.install_from_url(&url).await.unwrap();
    let after = svc.preview(&url).await.unwrap();
    let installed = after.installed.unwrap();
    assert_eq!(
        (installed.version.as_str(), installed.state),
        ("1.0.0", agentos_domain::api::AppState::Running)
    );
    assert!(svc.preview("mem://missing/app-ref.json").await.is_err());
}

#[tokio::test]
async fn the_catalog_lists_registry_apps_and_reports_what_it_could_not_read() {
    let f = Fakes::new();
    let notes = f.add_app("notes", &[("a", "x")]);
    f.add_app("todo", &[("b", "x")]);
    f.add_registry(
        "mem://registry/registry.json",
        &[&notes, "mem://todo/app-ref.json", "mem://gone/app-ref.json"],
    );
    let svc = f.service_with_registries(vec![
        "mem://registry/registry.json".into(),
        "mem://todo/app-ref.json".into(), // an app ref is a registry of one (and a duplicate here)
        "mem://nowhere/registry.json".into(),
    ]);
    svc.install_from_url(&notes).await.unwrap();
    let catalog = svc.catalog().await;
    let ids: Vec<&str> = catalog.apps.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["notes", "todo"], "listed once each, in registry order");
    assert!(catalog.apps[0].installed.is_some() && catalog.apps[1].installed.is_none());
    assert_eq!(catalog.errors.len(), 2, "{:?}", catalog.errors);
    assert!(
        catalog.errors.iter().any(|e| e.contains("gone"))
            && catalog.errors.iter().any(|e| e.contains("nowhere"))
    );
}

#[tokio::test]
async fn a_package_that_needs_a_newer_host_is_refused_at_preview_and_install() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("a", "x")]);
    let mut r: agentos_domain::AppRef =
        serde_json::from_slice(&f.fetcher.0.lock().unwrap()[&url].clone()).unwrap();
    r.manifest.requires = Some(agentos_domain::Requires { host: Some(">=9.0".into()) });
    f.fetcher.0.lock().unwrap().insert(url.clone(), serde_json::to_vec(&r).unwrap());
    let svc = f.service();
    assert!(svc.preview(&url).await.unwrap_err().to_string().contains("host API"));
    assert!(svc.install_from_url(&url).await.is_err());
}

// ---- what a reloaded or installed app ref is checked against ------------------------------------------

/// Edit the app ref published at `url` in place, as a remote author (or an attacker) could.
fn rewrite_ref(f: &Fakes, url: &str, edit: impl FnOnce(&mut agentos_domain::AppRef)) {
    let mut fetcher = f.fetcher.0.lock().unwrap();
    let mut r: agentos_domain::AppRef = serde_json::from_slice(&fetcher[url]).unwrap();
    edit(&mut r);
    fetcher.insert(url.to_owned(), serde_json::to_vec(&r).unwrap());
}

#[tokio::test]
async fn a_ref_installed_as_one_app_that_later_serves_another_id_is_refused() {
    let f = Fakes::new();
    let url = f.add_app("weather", &[("forecast", "x")]);
    let svc = f.service();
    let weather = svc.install_from_url(&url).await.unwrap();
    rewrite_ref(&f, &url, |r| r.manifest.id = AppId::parse("notes").unwrap());

    // a hot reload keeps the running app and says why
    let err = svc.reload(&weather).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::AppIdChanged { installed, served })
            if (installed.as_str(), served.as_str()) == ("weather", "notes")),
        "{err:?}"
    );
    assert_eq!(svc.status(&weather), Some(AppStatus::Running));
    assert_eq!(f.store.opened_with_migrations("notes"), None, "the other app's database is never opened");

    // a restart marks it failed, and still never opens notes' database
    svc.shutdown().await;
    let apps = f.service().restore_installed().await.unwrap();
    assert!(matches!(&apps[0].status, AppStatus::Failed(m) if m.contains("installed as `weather`")));
    assert_eq!(f.store.opened_with_migrations("notes"), None);
    assert_eq!(f.runtime.loads("notes"), 0);
}

#[tokio::test]
async fn permissions_that_grow_are_refused_until_the_app_is_installed_again() {
    let f = Fakes::new();
    let url = f.add_app_with("calc", &[("t", "x")], &[], vec![Permission::Data]);
    let svc = f.service();
    let id = svc.install_from_url(&url).await.unwrap();
    assert_eq!(f.installed.records()[0].permissions, vec![Permission::Data], "the approval is kept");
    rewrite_ref(&f, &url, |r| r.manifest.permissions = vec![Permission::Data, Permission::Shell]);

    let err = svc.reload(&id).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::PermissionsGrew { added, .. }) if added == &["shell"]),
        "{err:?}"
    );
    assert!(err.to_string().contains("shell"), "{err}");
    assert_eq!(svc.status(&id), Some(AppStatus::Running), "the approved version keeps serving");
    assert!(!f.runtime.capabilities("calc").unwrap().shell);

    // a restart does not quietly grant it either
    svc.shutdown().await;
    let second = f.service();
    let apps = second.restore_installed().await.unwrap();
    assert!(matches!(&apps[0].status, AppStatus::Failed(m) if m.contains("shell")));
    assert!(!f.runtime.capabilities("calc").unwrap().shell);
    assert!(second.enable(&id).await.is_err(), "enable is refused too");

    // approving = previewing and installing that digest again
    let preview = second.preview(&url).await.unwrap();
    assert_eq!(preview.permissions, vec!["data", "shell"]);
    second.install_previewed(&url, &preview.sha256).await.unwrap();
    assert_eq!(second.status(&id), Some(AppStatus::Running));
    assert!(f.runtime.capabilities("calc").unwrap().shell);
    assert_eq!(f.installed.records()[0].permissions, vec![Permission::Data, Permission::Shell]);
    // and now a reload with those permissions is fine, as is one that asks for fewer
    second.reload(&id).await.unwrap();
    rewrite_ref(&f, &url, |r| r.manifest.permissions = vec![Permission::Data]);
    second.reload(&id).await.unwrap();
    assert_eq!(f.installed.records()[0].permissions, vec![Permission::Data]);
}

#[tokio::test]
async fn install_takes_the_digest_that_was_previewed() {
    let f = Fakes::new();
    let url = f.add_app_with("notes", &[("t", "x")], &[], vec![Permission::Data]);
    let svc = f.service();
    let previewed = svc.preview(&url).await.unwrap();
    assert_eq!(previewed.sha256.len(), 64);
    assert_eq!(svc.preview(&url).await.unwrap().sha256, previewed.sha256, "the digest is stable");

    // the ref changes between the preview and the install (it now asks for more)
    rewrite_ref(&f, &url, |r| r.manifest.permissions.push(Permission::Shell));
    let err = svc.install_previewed(&url, &previewed.sha256).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::RefChanged { expected, .. }) if *expected == previewed.sha256),
        "{err:?}"
    );
    assert!(f.installed.records().is_empty() && f.registry.names().is_empty());
    assert_eq!(f.store.opened_with_migrations("notes"), None, "nothing was set up");

    // the same digest installs, and is recorded
    let fresh = svc.preview(&url).await.unwrap();
    assert_ne!(fresh.sha256, previewed.sha256);
    svc.install_previewed(&url, &fresh.sha256.to_uppercase()).await.unwrap();
    let rec = f.installed.records();
    assert_eq!(rec[0].ref_sha256, fresh.sha256);
    assert_eq!(rec[0].permissions, vec![Permission::Data, Permission::Shell]);
}

#[tokio::test]
async fn a_record_from_before_approvals_were_kept_adopts_what_the_manifest_asks_for() {
    let f = Fakes::new();
    let url = f.add_app_with("notes", &[("t", "x")], &[], vec![Permission::Data, Permission::Notify]);
    let first = f.service();
    first.install_from_url(&url).await.unwrap();
    first.shutdown().await;
    let mut legacy = f.installed.records().remove(0);
    legacy.permissions = vec![];
    legacy.ref_sha256 = String::new();
    f.installed.save(&legacy).await.unwrap();

    let apps = f.service().restore_installed().await.unwrap();
    assert_eq!(apps[0].status, AppStatus::Running, "an existing install keeps working");
    let rec = f.installed.records();
    assert_eq!(rec[0].permissions, vec![Permission::Data, Permission::Notify]);
    assert_eq!(rec[0].ref_sha256.len(), 64);
}

// ---- package limits ---------------------------------------------------------------------------------

#[tokio::test]
async fn an_app_ref_may_not_list_files_outside_its_own_origin_or_directory() {
    for foreign in
        ["file:///etc/passwd", "file:///dev/zero", "mem://evil/main.luau", "../main.luau", "/etc/passwd"]
    {
        let f = Fakes::new();
        let url = f.add_app("notes", &[("t", "x")]);
        rewrite_ref(&f, &url, |r| r.files[0].url = foreign.into());
        let svc = f.service();
        let err = svc.install_from_url(&url).await.unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::UnsafeFileUrl { .. })), "{foreign}: {err:?}");
        assert!(svc.preview(&url).await.is_err(), "{foreign} is refused at preview too");
        assert!(f.installed.records().is_empty() && f.store.opened_with_migrations("notes").is_none());
    }
}

#[tokio::test]
async fn files_over_the_cap_and_packages_over_the_cap_are_refused() {
    use agentos_domain::limits::{MAX_FILE_BYTES, MAX_PACKAGE_FILES};
    let big = |n: usize| "-- x\n".repeat(n / 5);
    // one file just over the file cap
    let f = Fakes::new();
    let one = big(MAX_FILE_BYTES + 5);
    let url = f.add_app_with("notes", &[("t", "x")], &[("lib/one.luau", &one)], vec![]);
    let err = f.service().install_from_url(&url).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::PackageTooLarge(m)) if m.contains("lib/one.luau")),
        "{err:?}"
    );

    // many files, each under the cap, together over the package cap
    let f = Fakes::new();
    let part = big(MAX_FILE_BYTES);
    let names: Vec<String> = (0..5).map(|i| format!("lib/m{i}.luau")).collect();
    let modules: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), part.as_str())).collect();
    let url = f.add_app_with("notes", &[("t", "x")], &modules, vec![]);
    let err = f.service().install_from_url(&url).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::PackageTooLarge(m)) if m.contains("package")),
        "{err:?}"
    );
    assert!(f.installed.records().is_empty() && f.registry.names().is_empty());

    // an app ref that lists too many files
    let f = Fakes::new();
    let url = f.add_app("notes", &[("t", "x")]);
    rewrite_ref(&f, &url, |r| {
        let first = r.files[0].clone();
        r.files.resize(MAX_PACKAGE_FILES + 1, first);
    });
    let err = f.service().preview(&url).await.unwrap_err();
    assert!(matches!(err, AppError::Domain(DomainError::PackageTooLarge(_))), "{err:?}");
}
