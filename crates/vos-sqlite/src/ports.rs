//! The port implementations: map `AppDb` onto `vos_application::ports::{DataStore, AppData}`.

use crate::{tx::SqliteTx, AppDb, DbError, Migration};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use vos_application::{
    data::{DataTx, ExecOutcome, Params},
    ports::{AppData, DataStore, PortResult, SqlMigration},
    PortError,
};
use vos_domain::AppId;

impl From<DbError> for PortError {
    fn from(e: DbError) -> Self {
        match e {
            // SQL / migration / parameter problems and runaway statements are the app's fault
            DbError::Sqlite(_)
            | DbError::MigrationChanged { .. }
            | DbError::MigrationOrder(_)
            | DbError::BadParams(_)
            | DbError::ReadOnly
            | DbError::TooManyRows
            | DbError::ResultTooLarge
            | DbError::Timeout(_) => PortError::Rejected(e.to_string()),
            DbError::Join(_) | DbError::Cancelled => PortError::Internal(e.to_string()),
        }
    }
}

#[async_trait]
impl AppData for AppDb {
    async fn migrate(&self, migrations: Vec<SqlMigration>) -> PortResult<()> {
        let migs = migrations.iter().map(|m| Migration::new(m.version, &m.name, &m.sql)).collect();
        Ok(AppDb::migrate(self, migs).await?)
    }
    async fn query(&self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        Ok(self.query_with(sql, params).await?)
    }
    async fn exec(&self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        let r = self.exec_with(sql, params).await?;
        Ok(ExecOutcome { changes: r.changes as u64, last_insert_id: r.last_insert_rowid })
    }
    async fn begin(&self) -> PortResult<Box<dyn DataTx>> {
        Ok(Box::new(SqliteTx::begin(self).await?))
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        Arc::new(AppDb { shared: self.shared.clone(), read_only: true })
    }
}

/// One database file per app under `root` (or in memory when `root` is `None`).
pub struct SqliteDataStore {
    root: Option<PathBuf>,
    open: Mutex<HashMap<AppId, AppDb>>,
}

impl SqliteDataStore {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self { root, open: Mutex::default() }
    }
    fn path(&self, app: &AppId) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join(format!("{app}.db")))
    }
}

#[async_trait]
impl DataStore for SqliteDataStore {
    async fn open(&self, app: &AppId, migrations: Vec<SqlMigration>) -> PortResult<Arc<dyn AppData>> {
        let path = self.path(app);
        let migs: Vec<Migration> =
            migrations.iter().map(|m| Migration::new(m.version, &m.name, &m.sql)).collect();
        // opening applies migrations: blocking work
        let db = tokio::task::spawn_blocking(move || AppDb::open(path.as_deref(), &migs))
            .await
            .map_err(|e| PortError::Internal(e.to_string()))??;
        self.open.lock().insert(app.clone(), db.clone());
        Ok(Arc::new(db))
    }

    async fn purge(&self, app: &AppId) -> PortResult<()> {
        self.open.lock().remove(app); // drops our handle; the app VM's clone is gone after unload
        if let Some(p) = self.path(app) {
            for ext in ["", "-wal", "-shm"] {
                let mut f = p.clone().into_os_string();
                f.push(ext);
                let _ = tokio::fs::remove_file(f).await;
            }
        }
        Ok(())
    }
}
