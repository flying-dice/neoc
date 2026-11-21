//! The data ports: one app's database, its transactions and its commit-time change feed.
//!
//! Everything a Lua `db:*` call needs is expressed here, so the use cases (live sources, write-back) are
//! testable with in-memory fakes and the SQLite adapter is one implementation among possible others.

use crate::error::PortError;
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc};
use vos_domain::{Revision, StoredRevision};

pub type PortResult<T> = Result<T, PortError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlMigration {
    pub version: u32,
    pub name: String,
    pub sql: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOutcome {
    pub changes: u64,
    pub last_insert_id: i64,
}

/// Statement parameters. Named binds (`:folder`) are what apps use; positional exists for the host's
/// own statements. Values are plain JSON: arrays and objects are stored as JSON text.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Params {
    #[default]
    None,
    Positional(Vec<Value>),
    /// Names without the leading `:`.
    Named(Vec<(String, Value)>),
}

impl Params {
    #[must_use]
    pub fn named<'a>(pairs: impl IntoIterator<Item = (&'a str, Value)>) -> Self {
        Self::Named(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Named(pairs) => pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The same params with one more named bind (replacing an existing one).
    #[must_use]
    pub fn with(self, name: &str, value: Value) -> Self {
        let mut pairs = match self {
            Self::Named(p) => p,
            _ => Vec::new(),
        };
        pairs.retain(|(k, _)| k != name);
        pairs.push((name.to_owned(), value));
        Self::Named(pairs)
    }
}

/// What preparing a query taught us (the SQLite authorizer reports every table and column it reads).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryInfo {
    /// Lower-cased names of the tables the query reads, through joins, sub-selects and views.
    pub tables: BTreeSet<String>,
    /// Result column names, in order.
    pub columns: Vec<String>,
    /// The named parameters the SQL uses (without `:`).
    pub named_params: Vec<String>,
    /// The statement does not write.
    pub read_only: bool,
}

/// The set of tables one commit changed. Batches are coalesced by the adapter (one per burst).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeBatch {
    pub tables: BTreeSet<String>,
}

impl ChangeBatch {
    #[must_use]
    pub fn of<'a>(tables: impl IntoIterator<Item = &'a str>) -> Self {
        Self { tables: tables.into_iter().map(str::to_lowercase).collect() }
    }

    /// "Something changed, we do not know what" (the feed lagged): every query must re-run.
    #[must_use]
    pub fn everything() -> Self {
        Self::of(["*"])
    }

    #[must_use]
    pub fn touches(&self, tables: &BTreeSet<String>) -> bool {
        self.tables.contains("*") || self.tables.iter().any(|t| tables.contains(t))
    }
}

/// A stream of committed changes. `None` ends the stream (the database was closed).
#[async_trait]
pub trait ChangeFeed: Send {
    async fn next_batch(&mut self) -> Option<ChangeBatch>;
}

/// An open transaction. It holds the database's write gate: other callers wait until it ends, so nothing
/// interleaves with it. Dropping it without `commit` rolls it back.
#[async_trait]
pub trait DataTx: Send {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<Value>>;
    async fn exec(&mut self, sql: String, params: Params) -> PortResult<ExecOutcome>;
    async fn commit(self: Box<Self>) -> PortResult<()>;
    async fn rollback(self: Box<Self>) -> PortResult<()>;
}

/// A single app's database (what `db:*` in Lua is backed by).
#[async_trait]
pub trait AppData: Send + Sync {
    /// Apply pending migrations in order (`db:migrate`): recorded, checksummed, idempotent.
    async fn migrate(&self, migrations: Vec<SqlMigration>) -> PortResult<()>;
    async fn query(&self, sql: String, params: Params) -> PortResult<Vec<Value>>;
    async fn exec(&self, sql: String, params: Params) -> PortResult<ExecOutcome>;
    /// Prepare `sql` without running it and report what it reads.
    async fn analyze(&self, sql: String) -> PortResult<QueryInfo>;
    async fn begin(&self) -> PortResult<Box<dyn DataTx>>;
    /// A fresh subscription to commit-time change batches.
    fn changes(&self) -> Box<dyn ChangeFeed>;
    /// A handle on the same database through which nothing can be written: every statement that would
    /// change data or schema fails, and so does `migrate`. A tool declared `readOnly` runs through this.
    fn read_only(&self) -> Arc<dyn AppData>;

    // ---- client state that survives closing a window (`persist = "app"`; `_ui_state`) ----
    /// The stored values of a window kind, by name.
    async fn ui_state(&self, window: &str) -> PortResult<serde_json::Map<String, Value>>;
    /// Store (insert or replace) values of a window kind.
    async fn set_ui_state(&self, window: &str, entries: serde_json::Map<String, Value>) -> PortResult<()>;
    /// Forget everything stored for every window (Settings: "clear saved window state").
    async fn clear_ui_state(&self) -> PortResult<()>;

    // ---- revisions: what a human's write replaced (kept 30 days) ----
    /// Keep a value; returns its id.
    async fn record_revision(&self, revision: Revision) -> PortResult<i64>;
    async fn revision(&self, id: i64) -> PortResult<Option<StoredRevision>>;
    /// A field's revisions, newest first.
    async fn revisions(&self, source: &str, key: &Value, field: &str) -> PortResult<Vec<StoredRevision>>;
    /// Delete revisions recorded before `cutoff_ms`; returns how many.
    async fn purge_revisions(&self, cutoff_ms: u64) -> PortResult<u64>;
}

/// Creates / destroys per-app databases.
#[async_trait]
pub trait DataStore: Send + Sync {
    /// Open (creating if needed) the app's database; applies `migrations` declared in the package manifest.
    async fn open(
        &self,
        app: &vos_domain::AppId,
        migrations: Vec<SqlMigration>,
    ) -> PortResult<std::sync::Arc<dyn AppData>>;
    /// Delete the app's data (uninstall with purge, delete data).
    async fn purge(&self, app: &vos_domain::AppId) -> PortResult<()>;
}
