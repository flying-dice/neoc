//! Driven adapter: SQLite (rusqlite, bundled) behind the `DataStore` / `AppData` ports.
//!
//! Per-app SQLite: one `rusqlite::Connection` per app behind a mutex, all work on tokio's blocking
//! pool, app-declared migrations recorded in `_migrations`, and commit-time change notifications
//! (`update_hook` buffered until `commit_hook`; discarded on rollback).

mod ports;
pub use ports::SqliteDataStore;

use parking_lot::Mutex;
use rusqlite::{
    hooks::{Action, AuthAction, Authorization},
    types::ValueRef,
    Connection, ToSql,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration {version} ({name}) was edited after being applied")]
    MigrationChanged { version: u32, name: String },
    #[error("migrations must be numbered 1..N in order, got {0}")]
    MigrationOrder(u32),
    #[error("blocking task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// An app-declared migration (comes from the app manifest / `migrations/001_init.sql`).
#[derive(Clone, Debug)]
pub struct Migration {
    pub version: u32,
    pub name: String,
    pub sql: String,
}

impl Migration {
    pub fn new(version: u32, name: &str, sql: &str) -> Self {
        Self { version, name: name.into(), sql: sql.into() }
    }
    fn checksum(&self) -> String {
        format!("{:x}", Sha256::digest(self.sql.as_bytes()))
    }
}

/// One committed change batch: (table, op) pairs, de-duplicated. Row ids are included for
/// fine-grained invalidation if wanted.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Change {
    pub table: String,
    pub op: &'static str,
    pub rowid: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ExecResult {
    pub changes: usize,
    pub last_insert_rowid: i64,
}

#[derive(Clone)]
pub struct AppDb {
    conn: Arc<Mutex<Connection>>,
    tx: broadcast::Sender<Arc<Vec<Change>>>,
}

impl AppDb {
    /// Open (or create) the app's database and apply pending migrations. `path = None` is in-memory.
    pub fn open(path: Option<&Path>, migrations: &[Migration]) -> Result<Self, DbError> {
        let mut conn = match path {
            Some(p) => Connection::open(p)?,
            None => Connection::open_in_memory()?,
        };
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        apply_migrations(&mut conn, migrations)?;

        // Sandbox the app's SQL: no ATTACH (would escape the per-app file), no extension loading.
        conn.authorizer(Some(|ctx: rusqlite::hooks::AuthContext<'_>| match ctx.action {
            AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
            _ => Authorization::Allow,
        }))?;

        // Change notifications. update_hook fires mid-transaction, so buffer and publish on commit.
        let (tx, _) = broadcast::channel(256);
        let pending: Arc<Mutex<Vec<Change>>> = Arc::default();
        let p1 = pending.clone();
        conn.update_hook(Some(move |action: Action, _db: &str, table: &str, rowid: i64| {
            let op = match action {
                Action::SQLITE_INSERT => "insert",
                Action::SQLITE_UPDATE => "update",
                Action::SQLITE_DELETE => "delete",
                _ => "unknown",
            };
            p1.lock().push(Change { table: table.into(), op, rowid });
        }))?;
        let (p2, tx2) = (pending.clone(), tx.clone());
        conn.commit_hook(Some(move || {
            let batch = std::mem::take(&mut *p2.lock());
            if !batch.is_empty() {
                let _ = tx2.send(Arc::new(batch)); // Err = no subscribers; fine
            }
            false // do not veto the commit
        }))?;
        let p3 = pending;
        conn.rollback_hook(Some(move || p3.lock().clear()))?;

        Ok(Self { conn: Arc::new(Mutex::new(conn)), tx })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<Change>>> {
        self.tx.subscribe()
    }

    /// Run a read query; returns rows as JSON objects. Runs on the blocking pool.
    pub async fn query(&self, sql: String, params: Vec<Value>) -> Result<Vec<Value>, DbError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock();
            let mut stmt = conn.prepare_cached(&sql)?;
            let names: Vec<String> = stmt.column_names().map(String::from).collect();
            let params = to_sql_params(&params);
            let p: Vec<&dyn ToSql> = params.iter().map(|v| v as &dyn ToSql).collect();
            let mut rows = stmt.query(p.as_slice())?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                let mut obj = Map::new();
                for (i, n) in names.iter().enumerate() {
                    obj.insert(n.clone(), value_ref_to_json(row.get_ref(i)?));
                }
                out.push(Value::Object(obj));
            }
            Ok(out)
        })
        .await?
    }

    /// Run a write statement. Returns `changes()` / `last_insert_rowid()`; change notifications
    /// are published by the commit hook.
    pub async fn exec(&self, sql: String, params: Vec<Value>) -> Result<ExecResult, DbError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock();
            let params = to_sql_params(&params);
            let p: Vec<&dyn ToSql> = params.iter().map(|v| v as &dyn ToSql).collect();
            let changes = conn.prepare_cached(&sql)?.execute(p.as_slice())?;
            Ok(ExecResult { changes, last_insert_rowid: conn.last_insert_rowid() })
        })
        .await?
    }

    /// Run several statements atomically (one commit => one notification batch).
    pub async fn transaction(&self, stmts: Vec<(String, Vec<Value>)>) -> Result<(), DbError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock();
            let tx = conn.transaction()?;
            for (sql, params) in stmts {
                let params = to_sql_params(&params);
                let p: Vec<&dyn ToSql> = params.iter().map(|v| v as &dyn ToSql).collect();
                tx.execute(&sql, p.as_slice())?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?
    }

    pub fn user_version_applied(&self) -> Result<u32, DbError> {
        let c = self.conn.lock();
        Ok(c.query_row("SELECT COALESCE(MAX(version),0) FROM _migrations", [], |r| r.get(0))?)
    }
}

/// JSON -> SQLite value. NB: do not rely on rusqlite's own `ToSql for serde_json::Value` (feature
/// `serde_json`): it stores a JSON *string* as its quoted JSON text (`"\"hello\""`).
fn to_sql_params(params: &[Value]) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value as S;
    params
        .iter()
        .map(|v| match v {
            Value::Null => S::Null,
            Value::Bool(b) => S::Integer(i64::from(*b)),
            Value::Number(n) => {
                n.as_i64().map_or_else(|| S::Real(n.as_f64().unwrap_or(f64::NAN)), S::Integer)
            }
            Value::String(s) => S::Text(s.clone()),
            other => S::Text(other.to_string()), // arrays/objects are stored as JSON text
        })
        .collect()
}

fn value_ref_to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => json!(b.to_vec()),
    }
}

fn apply_migrations(conn: &mut Connection, migrations: &[Migration]) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations(
            version INTEGER PRIMARY KEY, name TEXT NOT NULL, checksum TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')))",
    )?;
    for (i, m) in migrations.iter().enumerate() {
        if usize::try_from(m.version).ok() != Some(i + 1) {
            return Err(DbError::MigrationOrder(m.version));
        }
        let applied: Option<String> = conn
            .query_row("SELECT checksum FROM _migrations WHERE version=?1", [m.version], |r| r.get(0))
            .ok();
        match applied {
            Some(sum) if sum == m.checksum() => {}
            Some(_) => return Err(DbError::MigrationChanged { version: m.version, name: m.name.clone() }),
            None => {
                // DDL is transactional in SQLite: a failing migration leaves no trace.
                let tx = conn.transaction()?;
                tx.execute_batch(&m.sql)?;
                tx.execute(
                    "INSERT INTO _migrations(version,name,checksum) VALUES(?1,?2,?3)",
                    (m.version, &m.name, m.checksum()),
                )?;
                tx.commit()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn migs() -> Vec<Migration> {
        vec![
            Migration::new(1, "init", "CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT NOT NULL);"),
            Migration::new(
                2,
                "add_pinned",
                "ALTER TABLE notes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;",
            ),
        ]
    }

    #[tokio::test]
    async fn migrations_apply_in_order_and_are_recorded() {
        let db = AppDb::open(None, &migs()).unwrap();
        assert_eq!(db.user_version_applied().unwrap(), 2);
        let rows =
            db.query("SELECT version,name FROM _migrations ORDER BY version".into(), vec![]).await.unwrap();
        assert_eq!(rows, vec![json!({"version":1,"name":"init"}), json!({"version":2,"name":"add_pinned"})]);
        // column from migration 2 exists
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
        assert!(matches!(
            AppDb::open(Some(&path), &edited),
            Err(DbError::MigrationChanged { version: 1, .. })
        ));
        // a failing migration leaves nothing behind
        let mut bad = migs();
        bad.push(Migration::new(3, "bad", "CREATE TABLE x(a); THIS IS NOT SQL;"));
        assert!(AppDb::open(Some(&path), &bad).is_err());
        let db = AppDb::open(Some(&path), &migs()).unwrap();
        assert!(db.query("SELECT * FROM x".into(), vec![]).await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn change_notifications_fire_on_commit_only() {
        let db = AppDb::open(None, &migs()).unwrap();
        let mut rx = db.subscribe();
        let r = db.exec("INSERT INTO notes(body) VALUES('a')".into(), vec![]).await.unwrap();
        assert_eq!(r, ExecResult { changes: 1, last_insert_rowid: 1 });
        let batch = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        assert_eq!(*batch, vec![Change { table: "notes".into(), op: "insert", rowid: 1 }]);

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
                db.exec("INSERT INTO notes(body) VALUES(?1)".into(), vec![json!(format!("n{i}"))])
                    .await
                    .unwrap();
            }));
        }
        for h in hs {
            h.await.unwrap();
        }
        let n = db.query("SELECT count(*) AS n FROM notes".into(), vec![]).await.unwrap();
        assert_eq!(n[0]["n"], 50);
    }
}
