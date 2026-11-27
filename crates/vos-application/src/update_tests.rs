//! Use-case tests for updating an installed app to a newer version (`AppService::update`), from Discover and at start.

use crate::{
    ports::{Params, ServerEvent},
    services::AppStatus,
    testing::Fakes,
    AppError,
};
use serde_json::json;
use vos_domain::{AppId, DomainError};

const M1: &str = "CREATE TABLE notes(body);";
const M2: &str = "ALTER TABLE notes ADD COLUMN pinned;";

fn notes() -> AppId {
    AppId::parse("notes").unwrap()
}

#[tokio::test]
async fn an_update_keeps_the_data_runs_the_new_migrations_and_takes_over() {
    let f = Fakes::new();
    let v1 = f.add_app_version("notes", "0.4.0", &[("list", "x")], &[M1]);
    let v2 = f.add_app_version("notes", "0.5.0", &[("list", "x"), ("pin", "x")], &[M1, M2]);
    let svc = f.service();
    let id = svc.install_from_url(&v1).await.unwrap();
    svc.data_of(&id)
        .unwrap()
        .exec("INSERT INTO notes VALUES ('kept')".into(), Params::default())
        .await
        .unwrap();
    f.events.take();

    svc.update(&id, &v2).await.unwrap();
    let db = f.store.data("notes");
    assert_eq!(db.execs().len(), 1, "the same database, with what was written in it");
    assert_eq!(
        db.migrations().iter().map(|m| m.sql.as_str()).collect::<Vec<_>>(),
        [M1, M1, M2],
        "the new version's migrations ran on it (what ran already is skipped by the database)"
    );
    assert_eq!(f.store.opened_with_migrations("notes"), Some(1), "opened once, at install");
    assert!(!f.store.purged("notes"));
    assert_eq!(f.registry.names(), ["notes__list", "notes__pin"], "its tools follow the new version");
    assert_eq!(svc.status(&id), Some(AppStatus::Running));
    assert_eq!(f.events.take(), [ServerEvent::AppUpdated(id.clone())]);
    let rec = f.installed.records().remove(0);
    assert_eq!(
        (rec.source_url.as_str(), rec.version.as_str(), rec.ref_sha256.as_str()),
        (v2.as_str(), "0.5.0", svc.preview(&v2).await.unwrap().sha256.as_str()),
        "the record follows the new version"
    );

    // the next start loads the new version
    svc.shutdown().await;
    let apps = f.service().restore_installed().await.unwrap();
    assert_eq!((apps[0].version.as_str(), apps[0].source_url.as_str()), ("0.5.0", v2.as_str()));
}

#[tokio::test]
async fn an_update_that_does_not_load_keeps_the_running_version_and_its_record() {
    let f = Fakes::new();
    let v1 = f.add_app_version("notes", "0.4.0", &[("list", "x")], &[M1]);
    let v2 = f.add_app_version("notes", "0.5.0", &[("pin", "x")], &[M1, M2]);
    let svc = f.service();
    let id = svc.install_from_url(&v1).await.unwrap();
    let before = f.installed.records();
    f.events.take();

    f.runtime.fail_load("notes", true);
    let err = svc.update(&id, &v2).await.unwrap_err();
    assert!(err.to_string().contains("boom"), "{err}");
    assert_eq!(svc.status(&id), Some(AppStatus::Running), "the old version keeps serving");
    assert_eq!(f.registry.names(), ["notes__list"]);
    assert_eq!(f.installed.records(), before, "nothing is recorded");
    assert!(f.events.take().is_empty());
    assert_eq!(svc.apps()[0].version, "0.4.0");

    // a file of the new version that does not match its digest is refused before anything loads
    f.runtime.fail_load("notes", false);
    f.fetcher.0.lock().unwrap().insert("mem://notes/0.5.0/main.lua".into(), b"evil".to_vec());
    let err = svc.update(&id, &v2).await.unwrap_err();
    assert!(matches!(err, AppError::Domain(DomainError::ChecksumMismatch { .. })), "{err:?}");
    assert_eq!(f.runtime.loads("notes"), 2, "the install, and the update that failed to load");
    assert_eq!(f.installed.records(), before);

    // and one that cannot be fetched
    assert!(svc.update(&id, "mem://notes/9.9.9/app-ref.json").await.is_err());
    assert_eq!(f.installed.records(), before);
}

#[tokio::test]
async fn an_older_version_is_not_an_update() {
    let f = Fakes::new();
    let v1 = f.add_app_version("notes", "0.4.0", &[("list", "x")], &[M1]);
    let v10 = f.add_app_version("notes", "0.10.0", &[("list", "x")], &[M1]);
    let svc = f.service();
    let id = svc.install_from_url(&v10).await.unwrap();
    let err = svc.update(&id, &v1).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::OlderVersion { installed, offered, .. })
            if (installed.as_str(), offered.as_str()) == ("0.10.0", "0.4.0")),
        "versions compare as numbers: {err:?}"
    );
    assert_eq!(f.runtime.loads("notes"), 1, "nothing was loaded");
    assert_eq!(f.installed.records()[0].source_url, v10);
}

#[tokio::test]
async fn another_app_is_not_an_update() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app("notes", &[("list", "x")])).await.unwrap();
    let todo = f.add_app_version("todo", "2.0.0", &[("add", "x")], &[M1]);
    let err = svc.update(&id, &todo).await.unwrap_err();
    assert!(
        matches!(&err, AppError::Domain(DomainError::AppIdChanged { installed, served })
            if (installed.as_str(), served.as_str()) == ("notes", "todo")),
        "{err:?}"
    );
    assert_eq!(f.store.opened_with_migrations("todo"), None, "the other app's database is never opened");
    assert_eq!(f.runtime.loads("todo"), 0);
    // an app that is not installed has nothing to update
    let ghost = AppId::parse("ghost").unwrap();
    assert!(matches!(svc.update(&ghost, &todo).await, Err(AppError::NotInstalled(_))));
}

#[tokio::test]
async fn an_update_takes_the_digest_that_was_previewed() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app_version("notes", "0.4.0", &[], &[M1])).await.unwrap();
    let v2 = f.add_app_version("notes", "0.5.0", &[("pin", "x")], &[M1, M2]);
    let previewed = svc.preview(&v2).await.unwrap();

    // the new version changes between the preview and the update
    f.edit_ref(&v2, |r| r["manifest"]["ui"]["permissions"] = json!({ "camera": {} }));
    let err = svc.update_previewed(&id, &v2, &previewed.sha256).await.unwrap_err();
    assert!(matches!(err, AppError::Domain(DomainError::RefChanged { .. })), "{err:?}");
    assert_eq!(f.runtime.loads("notes"), 1, "nothing was loaded");
    assert_eq!(f.installed.records()[0].version, "0.4.0");

    let fresh = svc.preview(&v2).await.unwrap();
    assert_eq!(fresh.permissions, ["camera"], "the preview says what the new version asks for");
    svc.update_previewed(&id, &v2, &fresh.sha256).await.unwrap();
    assert_eq!(f.installed.records()[0].ref_sha256, fresh.sha256);
}

#[tokio::test]
async fn the_same_version_rebuilt_is_an_update() {
    let f = Fakes::new();
    let svc = f.service();
    let first = f.add_app("notes", &[("list", "x")]); // 1.0.0
    let id = svc.install_from_url(&first).await.unwrap();
    let rebuilt = f.add_app_version("notes", "1.0.0", &[("list", "x"), ("pin", "x")], &[M1]);
    svc.update(&id, &rebuilt).await.unwrap();
    let rec = f.installed.records().remove(0);
    assert_eq!((rec.source_url.as_str(), rec.version.as_str()), (rebuilt.as_str(), "1.0.0"));
    assert_eq!(f.registry.names(), ["notes__list", "notes__pin"]);
}

#[tokio::test]
async fn a_disabled_app_is_enabled_before_it_is_updated() {
    let f = Fakes::new();
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app_version("notes", "0.4.0", &[], &[M1])).await.unwrap();
    svc.disable(&id).await.unwrap();
    let v2 = f.add_app_version("notes", "0.5.0", &[], &[M1, M2]);
    assert!(matches!(svc.update(&id, &v2).await, Err(AppError::NotRunning { .. })));
    assert_eq!(f.installed.records()[0].version, "0.4.0");
}

#[tokio::test]
async fn the_catalog_offers_the_highest_version_and_marks_an_update() {
    let f = Fakes::new();
    let v1 = f.add_app_version("notes", "0.4.0", &[], &[M1]);
    let v3 = f.add_app_version("notes", "0.10.0", &[], &[M1]);
    let v2 = f.add_app_version("notes", "0.5.0", &[], &[M1]);
    let todo = f.add_app_version("todo", "1.0.0", &[], &[M1]);
    let calc = f.add_app_version("calc", "1.0.0", &[], &[M1]);
    f.add_registry("mem://registry/registry.json", &[&v1, &v3, &v2, &todo, &calc]);
    let svc = f.service_with_registries(vec!["mem://registry/registry.json".into()]);
    svc.install_from_url(&v1).await.unwrap();
    svc.install_from_url(&calc).await.unwrap();

    let catalog = svc.catalog().await;
    let row = |id: &str| catalog.apps.iter().find(|a| a.id == id).unwrap();
    assert_eq!(catalog.apps.len(), 3, "an app listed at several versions is listed once");
    let notes = row("notes");
    assert_eq!((notes.version.as_str(), notes.url.as_str()), ("0.10.0", v3.as_str()), "the highest version");
    assert_eq!(notes.installed.as_ref().map(|i| i.version.as_str()), Some("0.4.0"));
    assert!(notes.update, "installed at a lower version: an update");
    assert!(!row("todo").update, "not installed: an install");
    assert!(!row("calc").update, "installed at this version: nothing to update");
}

#[tokio::test]
async fn ensure_installed_updates_to_a_newer_url_and_skips_an_older_one() {
    let f = Fakes::new();
    let v1 = f.add_app_version("notes", "0.4.0", &[("list", "x")], &[M1]);
    let v2 = f.add_app_version("notes", "0.5.0", &[("list", "x"), ("pin", "x")], &[M1, M2]);
    let svc = f.service();
    svc.ensure_installed(std::slice::from_ref(&v1)).await;
    f.events.take();

    // the config now lists the new version: the deployer's approval
    svc.ensure_installed(&[v2.clone(), "mem://missing/app-ref.json".into()]).await;
    assert_eq!(f.events.take(), [ServerEvent::AppUpdated(notes())]);
    assert_eq!(f.installed.records()[0].source_url, v2);
    assert_eq!(f.registry.names(), ["notes__list", "notes__pin"]);
    assert_eq!(f.store.opened_with_migrations("notes"), Some(1), "updated on its database, not reinstalled");

    // and back to the old one: skipped, the newer version stays
    svc.ensure_installed(std::slice::from_ref(&v1)).await;
    assert_eq!(f.installed.records()[0].source_url, v2);
    assert_eq!(f.runtime.loads("notes"), 2);
    assert!(f.events.take().is_empty());
}
