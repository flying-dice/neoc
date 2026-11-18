//! The port implementations: map `AppDb` onto `agentos_application::ports::{DataStore, AppData}`.

use crate::{AppDb, DbError, Migration};
use agentos_application::{
    ports::{AppData, DataStore, ExecOutcome, PortResult, SqlMigration},
    PortError,
};
use agentos_domain::AppId;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

impl From<DbError> for PortError {
    fn from(e: DbError) -> Self {
        match e {
            // SQL / migration problems are the app's fault
            DbError::Sqlite(_) | DbError::MigrationChanged { .. } | DbError::MigrationOrder(_) => {
                PortError::Rejected(e.to_string())
            }
            DbError::Join(_) => PortError::Internal(e.to_string()),
        }
    }
}

#[async_trait]
impl AppData for AppDb {
    async fn query(&self, sql: String, params: Vec<Value>) -> PortResult<Vec<Value>> {
        Ok(AppDb::query(self, sql, params).await?)
    }
    async fn exec(&self, sql: String, params: Vec<Value>) -> PortResult<ExecOutcome> {
        let r = AppDb::exec(self, sql, params).await?;
        Ok(ExecOutcome { changes: r.changes as u64, last_insert_rowid: r.last_insert_rowid })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        data.exec("INSERT INTO t VALUES(?1)".into(), vec![json!("x")]).await.unwrap();
        assert_eq!(data.query("SELECT a FROM t".into(), vec![]).await.unwrap(), vec![json!({"a":"x"})]);
        let bad = data.query("SELEKT".into(), vec![]).await.unwrap_err();
        assert!(matches!(bad, PortError::Rejected(_)));
    }
}
