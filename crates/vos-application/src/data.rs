//! The data ports: one app's database and its transactions.
//!
//! Everything a Lua `db:*` call needs is expressed here, so the use cases are testable with in-memory fakes
//! and the SQLite adapter is one implementation among possible others.

use crate::error::PortError;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

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
    async fn begin(&self) -> PortResult<Box<dyn DataTx>>;
    /// A handle on the same database through which nothing can be written: every statement that would
    /// change data or schema fails, and so does `migrate`. A tool declared `readOnly` runs through this.
    fn read_only(&self) -> Arc<dyn AppData>;
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
