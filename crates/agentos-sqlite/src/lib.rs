//! Driven adapter: SQLite (rusqlite, bundled) behind the `DataStore` / `AppData` ports.
//!
//! Per-app SQLite: one `rusqlite::Connection` per app behind a mutex, all work on tokio's blocking pool and
//! behind one async *write gate* (a transaction holds it, so nothing interleaves with it). On top of that:
//!
//! - recorded, checksummed, incremental migrations (`_migrations`);
//! - named parameters, with the gotcha around rusqlite's JSON params handled (see [`to_sql_value`]);
//! - **dependency tracking**: the authorizer reports every table a statement reads (joins, sub-selects,
//!   views) and writes at prepare time;
//! - **change feed**: `update_hook` buffers, `commit_hook` publishes one batch per commit, `rollback_hook`
//!   discards; tables written are also recorded from the authorizer, so changes the hook cannot see
//!   (`WITHOUT ROWID` tables, `REPLACE` conflict deletes, DDL) still reach live queries;
//! - **interrupt**: a progress handler aborts a statement that runs past its wall-clock budget;
//! - `ATTACH` is denied;
//! - **limits**: one value (string, blob, row) is at most [`MAX_VALUE_BYTES`] (`SQLITE_LIMIT_LENGTH`), and a
//!   query returns at most [`MAX_QUERY_ROWS`] rows and [`MAX_QUERY_BYTES`] bytes, else it fails with a clear
//!   error instead of exhausting the host. A blob column reaches the caller as standard base64 text.

mod agents;
#[cfg(test)]
mod agents_tests;
mod devices;
mod feed;
#[cfg(test)]
mod limits_tests;
mod ports;
mod tx;

pub use devices::{SqliteDeviceRegistry, SqliteSystemDb};
pub use feed::SqliteFeed;
pub use ports::SqliteDataStore;
pub use tx::SqliteTx;

use agentos_application::data::{Params, QueryInfo};
use parking_lot::Mutex;
use rusqlite::{
    hooks::{Action, AuthAction, Authorization},
    limits::Limit,
    types::ValueRef,
    Connection, Statement,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
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
    #[error("beginning the transaction was cut short (the runtime is shutting down)")]
    Cancelled,
    #[error("the statement ran longer than {0:?} and was interrupted")]
    Timeout(Duration),
    #[error("{0}")]
    BadParams(String),
    #[error("attempt to write through a read-only handle")]
    ReadOnly,
    #[error("the query returned more than {MAX_QUERY_ROWS} rows; narrow it with WHERE or LIMIT")]
    TooManyRows,
    #[error("the query result is larger than {MAX_QUERY_BYTES} bytes; select fewer rows or columns")]
    ResultTooLarge,
}

/// An app-declared migration (comes from the app manifest or `db:migrate`).
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

/// One committed change: (table, op) pairs. Row ids are included for fine-grained invalidation if wanted.
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

/// Tables the authorizer saw while a statement was being prepared.
#[derive(Default)]
struct Collector {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
}

/// Internal tables every app database has (key-value store for `app.kv`, persisted client state).
const APP_TABLES: &str = "
CREATE TABLE IF NOT EXISTS _kv(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS _ui_state(
    window TEXT NOT NULL, instance TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL,
    PRIMARY KEY(window, instance, name)
);
CREATE TABLE IF NOT EXISTS _ui_revisions(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL, key TEXT NOT NULL, field TEXT NOT NULL,
    value TEXT NOT NULL, reason TEXT NOT NULL, replaced_by TEXT NOT NULL, recorded_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS _ui_revisions_by_field ON _ui_revisions(source, key, field, id);
CREATE INDEX IF NOT EXISTS _ui_revisions_by_age ON _ui_revisions(recorded_at);";

/// The largest string, blob or row SQLite will build (`SQLITE_LIMIT_LENGTH`; its default is 1 GB, which
/// `zeroblob(500000000)` would happily allocate).
pub const MAX_VALUE_BYTES: i32 = 8 * 1024 * 1024;
/// Rows one query may return.
pub const MAX_QUERY_ROWS: usize = 10_000;
/// Bytes one query result may hold, as it reaches the caller (blobs counted as base64).
pub const MAX_QUERY_BYTES: usize = 16 * 1024 * 1024;

/// Default wall-clock budget of one statement.
pub const DEFAULT_STATEMENT_TIMEOUT: Duration = Duration::from_secs(5);

struct Shared {
    conn: Mutex<Connection>,
    /// Held across a whole transaction; every other caller waits for it.
    gate: Arc<tokio::sync::Mutex<()>>,
    tx: broadcast::Sender<Arc<Vec<Change>>>,
    pending: Arc<Mutex<Vec<Change>>>,
    collector: Arc<Mutex<Option<Collector>>>,
    /// Interrupt deadline in ms since `base`; 0 = none.
    deadline: Arc<AtomicU64>,
    base: Instant,
    timeout: Duration,
    /// Raised while the adapter itself flips `query_only` (the authorizer refuses app SQL doing it).
    privileged: Arc<AtomicBool>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        // a transaction nobody ended is rolled back while the hooks are still alive (closing the connection
        // rolls it back through them)
        let conn = self.conn.get_mut();
        if !conn.is_autocommit() {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }
}

#[derive(Clone)]
pub struct AppDb {
    shared: Arc<Shared>,
    /// Every statement through this handle runs with `PRAGMA query_only`: nothing it does changes data or schema.
    read_only: bool,
}

impl AppDb {
    /// Open (or create) an app's database and apply pending migrations. `path = None` is in-memory.
    pub fn open(path: Option<&Path>, migrations: &[Migration]) -> Result<Self, DbError> {
        Self::open_with(path, migrations, true, DEFAULT_STATEMENT_TIMEOUT)
    }

    /// Like [`AppDb::open`] without the per-app internal tables (the host's own system database).
    pub fn open_system(path: Option<&Path>, migrations: &[Migration]) -> Result<Self, DbError> {
        Self::open_with(path, migrations, false, DEFAULT_STATEMENT_TIMEOUT)
    }

    pub fn open_with(
        path: Option<&Path>,
        migrations: &[Migration],
        app_tables: bool,
        timeout: Duration,
    ) -> Result<Self, DbError> {
        let mut conn = match path {
            Some(p) => Connection::open(p)?,
            None => Connection::open_in_memory()?,
        };
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_VALUE_BYTES)?;
        apply_migrations(&mut conn, migrations)?;
        if app_tables {
            conn.execute_batch(APP_TABLES)?;
        }

        let collector: Arc<Mutex<Option<Collector>>> = Arc::default();
        let deadline = Arc::new(AtomicU64::new(0));
        let base = Instant::now();

        // The authorizer sandboxes the app's SQL (no ATTACH: it would escape the per-app file) and, while a
        // collector is armed, records every table the statement reads and writes.
        let c = collector.clone();
        let privileged = Arc::new(AtomicBool::new(false));
        let host_pragma = privileged.clone();
        conn.authorizer(Some(move |ctx: rusqlite::hooks::AuthContext<'_>| {
            match ctx.action {
                AuthAction::Attach { .. } | AuthAction::Detach { .. } => return Authorization::Deny,
                // a read-only handle must stay read-only: app SQL may not lift `query_only`
                AuthAction::Pragma { pragma_name, .. }
                    if pragma_name.eq_ignore_ascii_case("query_only")
                        && !host_pragma.load(Ordering::SeqCst) =>
                {
                    return Authorization::Deny
                }
                AuthAction::Read { table_name, .. } => note(&c, table_name, false),
                AuthAction::Insert { table_name }
                | AuthAction::Update { table_name, .. }
                | AuthAction::Delete { table_name } => note(&c, table_name, true),
                _ => {}
            }
            Authorization::Allow
        }))?;

        // Interrupt a statement that overstays its deadline (a huge or runaway query).
        let d = deadline.clone();
        conn.progress_handler(
            1000,
            Some(move || {
                let at = d.load(Ordering::Relaxed);
                at != 0 && u64::try_from(base.elapsed().as_millis()).unwrap_or(u64::MAX) > at
            }),
        )?;

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
        let p3 = pending.clone();
        conn.rollback_hook(Some(move || p3.lock().clear()))?;

        Ok(Self {
            read_only: false,
            shared: Arc::new(Shared {
                conn: Mutex::new(conn),
                gate: Arc::default(),
                tx,
                pending,
                collector,
                deadline,
                base,
                timeout,
                privileged,
            }),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<Change>>> {
        self.shared.tx.subscribe()
    }

    /// Run a read query with positional params; returns rows as JSON objects. Runs on the blocking pool.
    pub async fn query(&self, sql: String, params: Vec<Value>) -> Result<Vec<Value>, DbError> {
        self.query_with(sql, Params::Positional(params)).await
    }

    /// Run a write statement with positional params. Change notifications are published by the commit hook.
    pub async fn exec(&self, sql: String, params: Vec<Value>) -> Result<ExecResult, DbError> {
        self.exec_with(sql, Params::Positional(params)).await
    }

    pub(crate) async fn query_with(&self, sql: String, params: Params) -> Result<Vec<Value>, DbError> {
        let _gate = self.shared.gate.lock().await;
        let read_only = self.read_only;
        self.blocking(move |s| run_query(s, &sql, &params, read_only)).await
    }

    pub(crate) async fn exec_with(&self, sql: String, params: Params) -> Result<ExecResult, DbError> {
        let _gate = self.shared.gate.lock().await;
        let read_only = self.read_only;
        self.blocking(move |s| run_exec(s, &sql, &params, read_only)).await
    }

    /// Run `work` on the blocking pool with the connection locked.
    pub(crate) async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Shared) -> Result<T, DbError> + Send + 'static,
    ) -> Result<T, DbError> {
        let shared = self.shared.clone();
        tokio::task::spawn_blocking(move || work(&shared)).await?
    }

    pub(crate) fn gate(&self) -> Arc<tokio::sync::Mutex<()>> {
        self.shared.gate.clone()
    }

    /// Apply pending migrations in order (incremental; a changed applied migration is an error).
    pub async fn migrate(&self, migrations: Vec<Migration>) -> Result<(), DbError> {
        if self.read_only {
            return Err(DbError::ReadOnly);
        }
        let _gate = self.shared.gate.lock().await;
        self.blocking(move |s| apply_migrations(&mut s.conn.lock(), &migrations)).await
    }

    /// Prepare `sql` without running it and report what it reads.
    pub async fn analyze(&self, sql: String) -> Result<QueryInfo, DbError> {
        self.blocking(move |s| {
            let conn = s.conn.lock();
            let (prepared, collected) = prepare_collecting(s, &conn, &sql);
            let stmt = prepared?;
            let mut named: Vec<String> = Vec::new();
            for i in 1..=stmt.parameter_count() {
                if let Some(name) = stmt.parameter_name(i) {
                    let name = name.trim_start_matches([':', '@', '$']).to_owned();
                    if !named.contains(&name) {
                        named.push(name);
                    }
                }
            }
            Ok(QueryInfo {
                tables: collected.reads,
                columns: stmt.column_names().map(str::to_owned).collect(),
                named_params: named,
                read_only: stmt.readonly(),
            })
        })
        .await
    }

    /// Run several statements atomically (one commit => one notification batch).
    pub async fn transaction(&self, stmts: Vec<(String, Vec<Value>)>) -> Result<(), DbError> {
        let _gate = self.shared.gate.lock().await;
        self.blocking(move |s| {
            let mut conn = s.conn.lock();
            let tx = conn.transaction()?;
            for (sql, params) in stmts {
                let params = to_sql_params(&params);
                tx.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub fn user_version_applied(&self) -> Result<u32, DbError> {
        let c = self.shared.conn.lock();
        Ok(c.query_row("SELECT COALESCE(MAX(version),0) FROM _migrations", [], |r| r.get(0))?)
    }
}

fn note(collector: &Mutex<Option<Collector>>, table: &str, write: bool) {
    if table.starts_with("sqlite_") {
        return;
    }
    if let Some(c) = collector.lock().as_mut() {
        let set = if write { &mut c.writes } else { &mut c.reads };
        set.insert(table.to_lowercase());
    }
}

/// Prepare with the authorizer's collector armed.
fn prepare_collecting<'c>(
    s: &Shared,
    conn: &'c Connection,
    sql: &str,
) -> (rusqlite::Result<Statement<'c>>, Collector) {
    *s.collector.lock() = Some(Collector::default());
    let prepared = conn.prepare(sql);
    let collected = s.collector.lock().take().unwrap_or_default();
    (prepared, collected)
}

/// Bind `params` onto a prepared statement.
fn bind(stmt: &mut Statement<'_>, params: &Params) -> Result<(), DbError> {
    match params {
        Params::None => {
            if stmt.parameter_count() > 0 {
                return Err(DbError::BadParams("the statement needs parameters and none were given".into()));
            }
        }
        Params::Positional(values) => {
            for (i, v) in values.iter().enumerate() {
                stmt.raw_bind_parameter(i + 1, to_sql_value(v))?;
            }
        }
        Params::Named(pairs) => {
            for i in 1..=stmt.parameter_count() {
                let Some(name) = stmt.parameter_name(i) else {
                    return Err(DbError::BadParams(
                        "a statement with named parameters cannot also use `?`".into(),
                    ));
                };
                let key = name.trim_start_matches([':', '@', '$']);
                let value = pairs
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v)
                    .ok_or_else(|| DbError::BadParams(format!("missing parameter :{key}")))?;
                stmt.raw_bind_parameter(i, to_sql_value(value))?;
            }
        }
    }
    Ok(())
}

/// Arms the interrupt deadline for one statement and disarms it afterwards.
struct Armed<'a>(&'a Shared);
impl<'a> Armed<'a> {
    fn new(s: &'a Shared) -> Self {
        let now = u64::try_from(s.base.elapsed().as_millis()).unwrap_or(u64::MAX);
        let budget = u64::try_from(s.timeout.as_millis()).unwrap_or(u64::MAX);
        s.deadline.store(now.saturating_add(budget).max(1), Ordering::Relaxed);
        Self(s)
    }
}
impl Drop for Armed<'_> {
    fn drop(&mut self) {
        self.0.deadline.store(0, Ordering::Relaxed);
    }
}

/// Map a rusqlite error, turning an interrupt into the timeout it was.
fn map_run_error(s: &Shared, e: rusqlite::Error) -> DbError {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::OperationInterrupted => {
            DbError::Timeout(s.timeout)
        }
        other => DbError::Sqlite(other),
    }
}

/// `PRAGMA query_only` for as long as it lives: SQLite refuses every write, including DDL, at execution time
/// (so a statement cached while writable cannot sneak one through). Held under the connection lock.
struct QueryOnly<'a> {
    conn: Option<&'a Connection>,
    privileged: &'a AtomicBool,
}

impl<'a> QueryOnly<'a> {
    fn new(s: &'a Shared, conn: &'a Connection, enabled: bool) -> Result<Self, DbError> {
        if enabled {
            s.privileged.store(true, Ordering::SeqCst);
            let set = conn.pragma_update(None, "query_only", true);
            s.privileged.store(false, Ordering::SeqCst);
            set?;
        }
        Ok(Self { conn: enabled.then_some(conn), privileged: &s.privileged })
    }
}

impl Drop for QueryOnly<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn {
            self.privileged.store(true, Ordering::SeqCst);
            let _ = conn.pragma_update(None, "query_only", false);
            self.privileged.store(false, Ordering::SeqCst);
        }
    }
}

pub(crate) fn run_query(
    s: &Shared,
    sql: &str,
    params: &Params,
    read_only: bool,
) -> Result<Vec<Value>, DbError> {
    let conn = s.conn.lock();
    let _read_only = QueryOnly::new(s, &conn, read_only)?;
    let mut stmt = conn.prepare_cached(sql)?;
    let names: Vec<String> = stmt.column_names().map(str::to_owned).collect();
    bind(&mut stmt, params)?;
    let _armed = Armed::new(s);
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    let mut bytes = 0usize;
    loop {
        match rows.next() {
            Ok(Some(row)) => {
                if out.len() >= MAX_QUERY_ROWS {
                    return Err(DbError::TooManyRows);
                }
                let mut obj = Map::new();
                for (i, n) in names.iter().enumerate() {
                    let v = row.get_ref(i)?;
                    bytes = bytes.saturating_add(n.len()).saturating_add(json_size(v));
                    if bytes > MAX_QUERY_BYTES {
                        return Err(DbError::ResultTooLarge);
                    }
                    obj.insert(n.clone(), value_ref_to_json(v));
                }
                out.push(Value::Object(obj));
            }
            Ok(None) => return Ok(out),
            Err(e) => return Err(map_run_error(s, e)),
        }
    }
}

pub(crate) fn run_exec(
    s: &Shared,
    sql: &str,
    params: &Params,
    read_only: bool,
) -> Result<ExecResult, DbError> {
    let conn = s.conn.lock();
    let _read_only = QueryOnly::new(s, &conn, read_only)?;
    let (prepared, collected) = prepare_collecting(s, &conn, sql);
    let mut stmt = prepared?;
    bind(&mut stmt, params)?;
    // Record the written tables first: the commit that ends this statement then publishes them even when
    // the update hook cannot see the change (WITHOUT ROWID tables, REPLACE conflicts, DDL).
    s.pending.lock().extend(collected.writes.into_iter().map(|table| Change {
        table,
        op: "write",
        rowid: 0,
    }));
    let _armed = Armed::new(s);
    let changes = stmt.raw_execute().map_err(|e| map_run_error(s, e))?;
    Ok(ExecResult { changes, last_insert_rowid: conn.last_insert_rowid() })
}

/// JSON -> SQLite value. NB: do not rely on rusqlite's own `ToSql for serde_json::Value` (feature
/// `serde_json`): it stores a JSON *string* as its quoted JSON text (`"\"hello\""`).
fn to_sql_value(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as S;
    match v {
        Value::Null => S::Null,
        Value::Bool(b) => S::Integer(i64::from(*b)),
        Value::Number(n) => n.as_i64().map_or_else(|| S::Real(n.as_f64().unwrap_or(f64::NAN)), S::Integer),
        Value::String(s) => S::Text(s.clone()),
        other => S::Text(other.to_string()), // arrays/objects are stored as JSON text
    }
}

fn to_sql_params(params: &[Value]) -> Vec<rusqlite::types::Value> {
    params.iter().map(to_sql_value).collect()
}

/// What a value takes once it is JSON (counted before it is built, so a huge one is refused cheaply).
fn json_size(v: ValueRef<'_>) -> usize {
    match v {
        ValueRef::Null => 4,
        ValueRef::Integer(_) | ValueRef::Real(_) => 24,
        ValueRef::Text(t) => t.len(),
        ValueRef::Blob(b) => b.len().div_ceil(3).saturating_mul(4),
    }
}

fn value_ref_to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::String(base64(b)),
    }
}

/// Standard base64 (RFC 4648, padded): how a BLOB column leaves the database. A JSON array of byte numbers
/// is ~4x larger and, as Lua values, ~16x.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let sextet = |n: u32, shift: u32| char::from(ALPHABET[(n >> shift & 63) as usize]);
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        out.push(sextet(n, 18));
        out.push(sextet(n, 12));
        out.push(if chunk.len() > 1 { sextet(n, 6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(n, 0) } else { '=' });
    }
    out
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
mod tests;
