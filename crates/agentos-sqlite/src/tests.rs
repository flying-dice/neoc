use super::*;
use agentos_application::{
    data::{ChangeBatch, ChangeFeed, Params},
    ports::{AppData, DataStore, InstalledApp, InstalledAppStore, SqlMigration},
    PortError,
};
use agentos_domain::{Actor, AppId, Revision, RevisionReason};
use std::time::Duration;

fn migs() -> Vec<Migration> {
    vec![
        Migration::new(1, "init", "CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT NOT NULL);"),
        Migration::new(2, "add_pinned", "ALTER TABLE notes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;"),
    ]
}

fn tables(names: &[&str]) -> std::collections::BTreeSet<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

#[tokio::test]
async fn migrations_apply_in_order_and_are_recorded() {
    let db = AppDb::open(None, &migs()).unwrap();
    assert_eq!(db.user_version_applied().unwrap(), 2);
    let rows =
        db.query("SELECT version,name FROM _migrations ORDER BY version".into(), vec![]).await.unwrap();
    assert_eq!(rows, vec![json!({"version":1,"name":"init"}), json!({"version":2,"name":"add_pinned"})]);
    db.exec("INSERT INTO notes(body,pinned) VALUES(?1,?2)".into(), vec![json!("hi"), json!(1)])
        .await
        .unwrap();
}

#[tokio::test]
async fn migrations_are_idempotent_incremental_and_tamper_checked() {
    let dir = std::env::temp_dir().join(format!("appdb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.db");
    let _ = std::fs::remove_file(&path);
    {
        let db = AppDb::open(Some(&path), &migs()[..1]).unwrap(); // old app version
        assert_eq!(db.user_version_applied().unwrap(), 1);
    }
    {
        let db = AppDb::open(Some(&path), &migs()).unwrap(); // app upgrade: only #2 runs
        assert_eq!(db.user_version_applied().unwrap(), 2);
    }
    let mut edited = migs();
    edited[0].sql.push_str(" -- edited");
    assert!(matches!(AppDb::open(Some(&path), &edited), Err(DbError::MigrationChanged { version: 1, .. })));
    // a failing migration leaves nothing behind
    let mut bad = migs();
    bad.push(Migration::new(3, "bad", "CREATE TABLE x(a); THIS IS NOT SQL;"));
    assert!(AppDb::open(Some(&path), &bad).is_err());
    let db = AppDb::open(Some(&path), &migs()).unwrap();
    assert!(db.query("SELECT * FROM x".into(), vec![]).await.is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn db_migrate_through_the_port_is_incremental_and_checksummed() {
    let db = AppDb::open(None, &[]).unwrap();
    let m = |v: u32, sql: &str| SqlMigration { version: v, name: format!("m{v}"), sql: sql.into() };
    AppData::migrate(&db, vec![m(1, "CREATE TABLE a(x)")]).await.unwrap();
    AppData::migrate(&db, vec![m(1, "CREATE TABLE a(x)"), m(2, "CREATE TABLE b(y)")]).await.unwrap();
    assert_eq!(db.user_version_applied().unwrap(), 2);
    let edited = AppData::migrate(&db, vec![m(1, "CREATE TABLE a(x, z)")]).await.unwrap_err();
    assert!(matches!(edited, PortError::Rejected(m) if m.contains("edited after being applied")));
    let gap =
        AppData::migrate(&db, vec![m(1, "CREATE TABLE a(x)"), m(3, "CREATE TABLE c(z)")]).await.unwrap_err();
    assert!(matches!(gap, PortError::Rejected(_)));
}

#[tokio::test]
async fn change_notifications_fire_on_commit_only() {
    let db = AppDb::open(None, &migs()).unwrap();
    let mut rx = db.subscribe();
    let r = db.exec("INSERT INTO notes(body) VALUES('a')".into(), vec![]).await.unwrap();
    assert_eq!(r, ExecResult { changes: 1, last_insert_rowid: 1 });
    let batch = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
    assert!(batch.contains(&Change { table: "notes".into(), op: "insert", rowid: 1 }), "{batch:?}");

    // multi-statement txn => ONE batch
    db.transaction(vec![
        ("UPDATE notes SET body='b' WHERE id=1".into(), vec![]),
        ("INSERT INTO notes(body) VALUES('c')".into(), vec![]),
    ])
    .await
    .unwrap();
    let batch = rx.recv().await.unwrap();
    assert_eq!(batch.len(), 2);

    // failed txn => rolled back => nothing published
    let err = db
        .transaction(vec![
            ("INSERT INTO notes(body) VALUES('d')".into(), vec![]),
            ("INSERT INTO notes(body) VALUES(NULL)".into(), vec![]), // NOT NULL violation
        ])
        .await;
    assert!(err.is_err());
    assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv()).await.is_err());
    let n = db.query("SELECT count(*) AS n FROM notes".into(), vec![]).await.unwrap();
    assert_eq!(n[0]["n"], 2);
}

#[tokio::test]
async fn json_params_round_trip_as_plain_sql_values() {
    let db = AppDb::open(None, &migs()).unwrap();
    db.exec("INSERT INTO notes(body,pinned) VALUES(?1,?2)".into(), vec![json!("plain"), json!(true)])
        .await
        .unwrap();
    let r = db.query("SELECT body, pinned, typeof(body) AS t FROM notes".into(), vec![]).await.unwrap();
    assert_eq!(r[0], json!({"body":"plain","pinned":1,"t":"text"}));
}

#[tokio::test]
async fn named_params_bind_by_name_and_a_string_is_never_stored_as_quoted_json() {
    let db = AppDb::open(None, &migs()).unwrap();
    let named = |pairs: &[(&str, Value)]| Params::named(pairs.iter().map(|(k, v)| (*k, v.clone())));
    db.exec_with(
        "INSERT INTO notes(body, pinned) VALUES(:body, :pinned)".into(),
        named(&[("pinned", json!(true)), ("body", json!("hello"))]),
    )
    .await
    .unwrap();
    // the same name twice, and an extra param that the SQL does not use (the window wrapper adds some)
    let rows = db
        .query_with(
            "SELECT body, typeof(body) AS t FROM notes WHERE body = :v OR body = :v".into(),
            named(&[("v", json!("hello")), ("unused", json!(1))]),
        )
        .await
        .unwrap();
    assert_eq!(rows, vec![json!({"body": "hello", "t": "text"})], "not \"\\\"hello\\\"\"");
    let missing = db.query_with("SELECT :a, :b".into(), named(&[("a", json!(1))])).await.unwrap_err();
    assert!(missing.to_string().contains("missing parameter :b"), "{missing}");
    let mixed = db.query_with("SELECT ?, :a".into(), named(&[("a", json!(1))])).await.unwrap_err();
    assert!(matches!(mixed, DbError::BadParams(_)));
    let none = db.query_with("SELECT :a".into(), Params::None).await.unwrap_err();
    assert!(matches!(none, DbError::BadParams(_)));
    // arrays and objects are stored as JSON text
    let r = db.query_with("SELECT :v AS v".into(), named(&[("v", json!([1, 2]))])).await.unwrap();
    assert_eq!(r[0]["v"], json!("[1,2]"));
}

#[tokio::test]
async fn app_sql_cannot_attach_other_databases() {
    let db = AppDb::open(None, &migs()).unwrap();
    let e = db.exec("ATTACH DATABASE '/tmp/evil.db' AS evil".into(), vec![]).await;
    assert!(e.is_err(), "ATTACH must be denied");
}

#[tokio::test]
async fn concurrent_callers_do_not_block_the_runtime() {
    let db = AppDb::open(None, &migs()).unwrap();
    let mut hs = vec![];
    for i in 0..50 {
        let db = db.clone();
        hs.push(tokio::spawn(async move {
            db.exec("INSERT INTO notes(body) VALUES(?1)".into(), vec![json!(format!("n{i}"))]).await.unwrap();
        }));
    }
    for h in hs {
        h.await.unwrap();
    }
    let n = db.query("SELECT count(*) AS n FROM notes".into(), vec![]).await.unwrap();
    assert_eq!(n[0]["n"], 50);
}

// ---- dependency tracking -----------------------------------------------------------------------------

fn shop() -> AppDb {
    AppDb::open(
        None,
        &[Migration::new(
            1,
            "init",
            "CREATE TABLE folders(id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE notes(id INTEGER PRIMARY KEY, folder_id INTEGER, body TEXT);
             CREATE TABLE tags(note_id INTEGER, tag TEXT);
             CREATE VIEW tagged AS SELECT n.id, t.tag FROM notes n JOIN tags t ON t.note_id = n.id;",
        )],
    )
    .unwrap()
}

#[tokio::test]
async fn analyze_reports_every_table_a_query_reads_and_what_it_selects() {
    let db = shop();
    let simple = db
        .analyze("SELECT id, body FROM notes WHERE folder_id = :folder AND body LIKE :q".into())
        .await
        .unwrap();
    assert_eq!(simple.tables, tables(&["notes"]));
    assert_eq!(simple.columns, ["id", "body"]);
    assert_eq!(simple.named_params, ["folder", "q"]);
    assert!(simple.read_only);

    let join = db
        .analyze(
            "SELECT f.name, (SELECT count(*) FROM notes n WHERE n.folder_id = f.id) AS count FROM folders f"
                .into(),
        )
        .await
        .unwrap();
    assert_eq!(join.tables, tables(&["folders", "notes"]), "a sub-select is a dependency too");
    assert_eq!(join.columns, ["name", "count"]);

    let view = db.analyze("SELECT * FROM tagged".into()).await.unwrap();
    assert!(view.tables.contains("notes") && view.tables.contains("tags"), "a view's tables count: {view:?}");

    let wrapped = db
        .analyze(
            "SELECT * FROM (\nSELECT id FROM notes ORDER BY id\n) LIMIT :__limit OFFSET :__offset".into(),
        )
        .await
        .unwrap();
    assert_eq!(wrapped.tables, tables(&["notes"]));

    assert!(!db.analyze("DELETE FROM notes WHERE id = 1".into()).await.unwrap().read_only);
    assert!(db.analyze("SELECT * FROM nope".into()).await.is_err(), "bad SQL fails at declaration time");
}

#[tokio::test]
async fn a_commit_reports_the_tables_it_wrote_even_where_the_update_hook_is_blind() {
    let db = AppDb::open(
        None,
        &[Migration::new(
            1,
            "init",
            "CREATE TABLE kv(k TEXT PRIMARY KEY, v) WITHOUT ROWID;
             CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT UNIQUE);",
        )],
    )
    .unwrap();
    let mut feed = SqliteFeed::new(&db).with_window(Duration::from_millis(5));
    // the update hook never fires for WITHOUT ROWID tables
    db.exec("INSERT INTO kv VALUES('a', 1)".into(), vec![]).await.unwrap();
    let batch = tokio::time::timeout(Duration::from_secs(1), feed.next_batch()).await.unwrap().unwrap();
    assert_eq!(batch, ChangeBatch::of(["kv"]));

    // a REPLACE conflict deletes a row without the delete hook; the table is still reported
    db.exec("INSERT INTO notes(id, body) VALUES(1, 'x')".into(), vec![]).await.unwrap();
    feed.next_batch().await.unwrap();
    db.exec("INSERT OR REPLACE INTO notes(id, body) VALUES(2, 'x')".into(), vec![]).await.unwrap();
    assert_eq!(feed.next_batch().await.unwrap(), ChangeBatch::of(["notes"]));
}

#[tokio::test]
async fn a_burst_of_commits_is_one_batch_and_a_lagging_reader_gets_everything() {
    let db = AppDb::open(None, &migs()).unwrap();
    let mut feed = SqliteFeed::new(&db).with_window(Duration::from_millis(100));
    for i in 0..5 {
        db.exec("INSERT INTO notes(body) VALUES(?1)".into(), vec![json!(format!("n{i}"))]).await.unwrap();
    }
    let batch = feed.next_batch().await.unwrap();
    assert_eq!(batch, ChangeBatch::of(["notes"]), "five commits, one batch");
    assert!(
        tokio::time::timeout(Duration::from_millis(150), feed.next_batch()).await.is_err(),
        "no second batch"
    );

    // 300 commits with nobody reading overflows the 256-slot channel
    let mut slow = SqliteFeed::new(&db).with_window(Duration::from_millis(1));
    for i in 0..300 {
        db.exec("INSERT INTO notes(body) VALUES(?1)".into(), vec![json!(format!("m{i}"))]).await.unwrap();
    }
    let lagged = slow.next_batch().await.unwrap();
    assert!(lagged.touches(&tables(&["anything"])), "unknown loss means every query re-runs");
}

#[tokio::test]
async fn the_feed_ends_when_the_database_is_dropped() {
    let db = AppDb::open(None, &migs()).unwrap();
    let mut feed = SqliteFeed::new(&db);
    drop(db);
    assert_eq!(feed.next_batch().await, None);
}

// ---- transactions and limits --------------------------------------------------------------------------

#[tokio::test]
async fn a_transaction_holds_the_gate_and_commits_as_one_batch() {
    let db = AppDb::open(None, &migs()).unwrap();
    let mut feed = SqliteFeed::new(&db).with_window(Duration::from_millis(5));
    let mut tx = AppData::begin(&db).await.unwrap();
    tx.exec("INSERT INTO notes(body) VALUES('a')".into(), Params::None).await.unwrap();
    tx.exec("INSERT INTO notes(body) VALUES('b')".into(), Params::None).await.unwrap();

    // another caller waits for the transaction instead of interleaving with it
    let other = db.clone();
    let waiting =
        tokio::spawn(async move { other.query("SELECT count(*) AS n FROM notes".into(), vec![]).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished(), "a reader must not see (or wait inside) an open transaction");
    let inside = tx.query("SELECT count(*) AS n FROM notes".into(), Params::None).await.unwrap();
    assert_eq!(inside[0]["n"], 2, "the transaction sees its own writes");

    tx.commit().await.unwrap();
    assert_eq!(waiting.await.unwrap().unwrap()[0]["n"], 2);
    assert_eq!(feed.next_batch().await.unwrap(), ChangeBatch::of(["notes"]));
    assert!(tokio::time::timeout(Duration::from_millis(60), feed.next_batch()).await.is_err());
}

#[tokio::test]
async fn a_dropped_or_rolled_back_transaction_leaves_nothing_and_frees_the_gate() {
    let db = AppDb::open(None, &migs()).unwrap();
    let mut feed = SqliteFeed::new(&db).with_window(Duration::from_millis(5));
    {
        let mut tx = AppData::begin(&db).await.unwrap();
        tx.exec("INSERT INTO notes(body) VALUES('lost')".into(), Params::None).await.unwrap();
        // dropped without commit: rolled back
    }
    let tx = AppData::begin(&db).await.unwrap();
    let mut tx2 = tx;
    tx2.exec("INSERT INTO notes(body) VALUES('also lost')".into(), Params::None).await.unwrap();
    tx2.rollback().await.unwrap();
    let n = db.query("SELECT count(*) AS n FROM notes".into(), vec![]).await.unwrap();
    assert_eq!(n[0]["n"], 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(60), feed.next_batch()).await.is_err(),
        "no change published"
    );
    db.exec("INSERT INTO notes(body) VALUES('fine')".into(), vec![]).await.unwrap();
}

#[tokio::test]
async fn a_runaway_statement_is_interrupted_at_its_budget_and_the_database_stays_usable() {
    let db = AppDb::open_with(None, &migs(), true, Duration::from_millis(200)).unwrap();
    let started = std::time::Instant::now();
    let err = db
        .query(
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c".into(),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DbError::Timeout(_)), "{err}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(matches!(PortError::from(err), PortError::Rejected(m) if m.contains("interrupted")));
    // a write that spins is interrupted too, and the gate is not left held
    let err = db
        .exec(
            "INSERT INTO notes(body) WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT 'x' FROM c".into(),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DbError::Timeout(_)), "{err}");
    assert_eq!(db.query("SELECT count(*) AS n FROM notes".into(), vec![]).await.unwrap()[0]["n"], 0);
}

// ---- app tables, revisions ---------------------------------------------------------------------------

#[tokio::test]
async fn every_app_database_has_a_kv_table_and_the_system_database_does_not() {
    let app = AppDb::open(None, &[]).unwrap();
    app.exec("INSERT INTO _kv(key, value) VALUES('k', '1')".into(), vec![]).await.unwrap();
    app.exec(
        "INSERT INTO _ui_state(window, instance, name, value) VALUES('main', '', 'folder', '3')".into(),
        vec![],
    )
    .await
    .unwrap();
    let system = AppDb::open_system(None, &[]).unwrap();
    assert!(system.query("SELECT * FROM _kv".into(), vec![]).await.is_err());
}

fn revision(value: &str, at: u64) -> Revision {
    Revision {
        source: "note".into(),
        key: json!(7),
        field: "body".into(),
        value: json!(value),
        reason: RevisionReason::TextConflict,
        replaced_by: Actor::user("You"),
        recorded_at_ms: at,
    }
}

#[tokio::test]
async fn revisions_are_kept_listed_newest_first_and_purged_by_age() {
    let db = AppDb::open(None, &[]).unwrap();
    let a = AppData::record_revision(&db, revision("first", 1_000)).await.unwrap();
    let b = AppData::record_revision(&db, revision("second", 2_000)).await.unwrap();
    let mut other = revision("elsewhere", 2_500);
    other.field = "title".into();
    AppData::record_revision(&db, other).await.unwrap();

    let got = AppData::revision(&db, a).await.unwrap().unwrap();
    assert_eq!((got.id, got.revision), (a, revision("first", 1_000)));
    assert!(AppData::revision(&db, 999).await.unwrap().is_none());
    let listed = AppData::revisions(&db, "note", &json!(7), "body").await.unwrap();
    assert_eq!(listed.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b, a]);
    assert!(AppData::revisions(&db, "note", &json!(8), "body").await.unwrap().is_empty());

    assert_eq!(AppData::purge_revisions(&db, 1_500).await.unwrap(), 1);
    assert_eq!(AppData::revisions(&db, "note", &json!(7), "body").await.unwrap().len(), 1);
}

// ---- the system database and the data store ------------------------------------------------------------

#[tokio::test]
async fn installed_apps_are_saved_listed_toggled_and_removed() {
    let db = SqliteSystemDb::open(None).unwrap();
    let app = |id: &str, at: u64| InstalledApp {
        id: AppId::parse(id).unwrap(),
        source_url: format!("file:///apps/{id}/app-ref.json"),
        version: "1.0.0".into(),
        enabled: true,
        installed_at_ms: at,
        ref_sha256: format!("{at:064x}"),
    };
    db.save(&app("todo", 20)).await.unwrap();
    db.save(&app("notes", 10)).await.unwrap();
    assert_eq!(db.list().await.unwrap(), vec![app("notes", 10), app("todo", 20)]);
    db.set_enabled(&AppId::parse("notes").unwrap(), false).await.unwrap();
    assert!(!db.list().await.unwrap()[0].enabled);
    db.save(&InstalledApp { version: "2.0.0".into(), ..app("todo", 20) }).await.unwrap();
    assert_eq!(db.list().await.unwrap()[1].version, "2.0.0", "saving again replaces");
    db.remove(&AppId::parse("notes").unwrap()).await.unwrap();
    assert_eq!(db.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn installed_apps_survive_reopening_the_system_database() {
    let dir = std::env::temp_dir().join(format!("system-installed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("system.db");
    let record = InstalledApp {
        id: AppId::parse("notes").unwrap(),
        source_url: "file:///x/app-ref.json".into(),
        version: "1.0.0".into(),
        enabled: false,
        installed_at_ms: 5,
        ref_sha256: "ab".repeat(32),
    };
    SqliteSystemDb::open(Some(&path)).unwrap().save(&record).await.unwrap();
    assert_eq!(SqliteSystemDb::open(Some(&path)).unwrap().list().await.unwrap(), vec![record]);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn data_store_port_applies_declared_migrations_and_serves_queries() {
    let store = SqliteDataStore::new(None);
    let app = AppId::parse("notes").unwrap();
    let data = store
        .open(
            &app,
            vec![SqlMigration { version: 1, name: "init".into(), sql: "CREATE TABLE t(a TEXT);".into() }],
        )
        .await
        .unwrap();
    data.exec("INSERT INTO t VALUES(?1)".into(), Params::Positional(vec![json!("x")])).await.unwrap();
    assert_eq!(data.query("SELECT a FROM t".into(), Params::None).await.unwrap(), vec![json!({"a":"x"})]);
    let bad = data.query("SELEKT".into(), Params::None).await.unwrap_err();
    assert!(matches!(bad, PortError::Rejected(_)));
}

#[tokio::test]
async fn purge_deletes_the_database_files() {
    let dir = std::env::temp_dir().join(format!("purge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = SqliteDataStore::new(Some(dir.clone()));
    let app = AppId::parse("notes").unwrap();
    drop(store.open(&app, vec![]).await.unwrap());
    assert!(dir.join("notes.db").exists());
    store.purge(&app).await.unwrap();
    assert!(!dir.join("notes.db").exists());
    let _ = std::fs::remove_dir_all(dir);
}

// ---- read-only handles ----------------------------------------------------------------------------------

#[tokio::test]
async fn a_read_only_handle_reads_and_refuses_every_write_while_the_writable_one_keeps_working() {
    use agentos_application::data::{AppData, Params};
    let db = shop();
    AppData::exec(&db, "INSERT INTO notes(body) VALUES ('a')".into(), Params::None).await.unwrap();
    let ro = AppData::read_only(&db);

    let rows = ro.query("SELECT body FROM notes".into(), Params::None).await.unwrap();
    assert_eq!(rows.len(), 1);
    for sql in [
        "INSERT INTO notes(body) VALUES ('b')",
        "UPDATE notes SET body = 'z'",
        "DELETE FROM notes",
        "CREATE TABLE t(a)",
        "DROP TABLE tags",
    ] {
        let err = ro.exec(sql.into(), Params::None).await.unwrap_err().to_string().to_lowercase();
        assert!(err.contains("readonly") || err.contains("read-only"), "{sql}: {err}");
    }
    // a write smuggled through the read path, and the pragma that would lift the guard
    assert!(ro
        .query("INSERT INTO notes(body) VALUES ('c') RETURNING id".into(), Params::None)
        .await
        .is_err());
    assert!(ro.query("PRAGMA query_only = 0".into(), Params::None).await.is_err());
    assert!(ro.migrate(vec![]).await.is_err());
    let mut tx = ro.begin().await.unwrap();
    assert!(tx.exec("DELETE FROM notes".into(), Params::None).await.is_err());
    tx.rollback().await.unwrap();

    assert_eq!(db.query("SELECT body FROM notes".into(), vec![]).await.unwrap().len(), 1, "nothing changed");
    AppData::exec(&db, "INSERT INTO notes(body) VALUES ('d')".into(), Params::None).await.unwrap();
    assert_eq!(db.query("SELECT body FROM notes".into(), vec![]).await.unwrap().len(), 2);
}
