//! The port implementations: map `AppDb` onto `agentos_application::ports::{DataStore, AppData}`.

use crate::{feed::SqliteFeed, tx::SqliteTx, AppDb, DbError, Migration};
use agentos_application::{
    data::{ChangeFeed, DataTx, ExecOutcome, Params, QueryInfo},
    ports::{AppData, DataStore, PortResult, SqlMigration},
    PortError,
};
use agentos_domain::{Actor, AppId, Revision, RevisionReason, StoredRevision};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

impl From<DbError> for PortError {
    fn from(e: DbError) -> Self {
        match e {
            // SQL / migration / parameter problems and runaway statements are the app's fault
            DbError::Sqlite(_)
            | DbError::MigrationChanged { .. }
            | DbError::MigrationOrder(_)
            | DbError::BadParams(_)
            | DbError::Timeout(_) => PortError::Rejected(e.to_string()),
            DbError::Join(_) => PortError::Internal(e.to_string()),
        }
    }
}

fn to_stored(row: &Value) -> Option<StoredRevision> {
    Some(StoredRevision {
        id: row["id"].as_i64()?,
        revision: Revision {
            source: row["source"].as_str()?.to_owned(),
            key: serde_json::from_str(row["key"].as_str()?).ok()?,
            field: row["field"].as_str()?.to_owned(),
            value: serde_json::from_str(row["value"].as_str()?).ok()?,
            reason: RevisionReason::parse(row["reason"].as_str()?)?,
            replaced_by: serde_json::from_str::<Actor>(row["replaced_by"].as_str()?).ok()?,
            recorded_at_ms: row["recorded_at"].as_u64()?,
        },
    })
}

const REVISION_COLUMNS: &str = "id, source, key, field, value, reason, replaced_by, recorded_at";

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
    async fn analyze(&self, sql: String) -> PortResult<QueryInfo> {
        Ok(AppDb::analyze(self, sql).await?)
    }
    async fn begin(&self) -> PortResult<Box<dyn DataTx>> {
        Ok(Box::new(SqliteTx::begin(self).await?))
    }
    fn changes(&self) -> Box<dyn ChangeFeed> {
        Box::new(SqliteFeed::new(self))
    }

    async fn record_revision(&self, r: Revision) -> PortResult<i64> {
        let params = Params::Positional(vec![
            json!(r.source),
            json!(r.key.to_string()),
            json!(r.field),
            json!(r.value.to_string()),
            json!(r.reason.as_str()),
            json!(serde_json::to_string(&r.replaced_by).map_err(|e| PortError::Internal(e.to_string()))?),
            json!(i64::try_from(r.recorded_at_ms).unwrap_or(i64::MAX)),
        ]);
        let out = self
            .exec_with(
                "INSERT INTO _ui_revisions(source, key, field, value, reason, replaced_by, recorded_at) \
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                    .into(),
                params,
            )
            .await?;
        Ok(out.last_insert_rowid)
    }
    async fn revision(&self, id: i64) -> PortResult<Option<StoredRevision>> {
        let rows = self
            .query_with(
                format!("SELECT {REVISION_COLUMNS} FROM _ui_revisions WHERE id = ?1"),
                Params::Positional(vec![json!(id)]),
            )
            .await?;
        Ok(rows.first().and_then(to_stored))
    }
    async fn revisions(&self, source: &str, key: &Value, field: &str) -> PortResult<Vec<StoredRevision>> {
        let rows = self
            .query_with(
                format!(
                    "SELECT {REVISION_COLUMNS} FROM _ui_revisions \
                     WHERE source = ?1 AND key = ?2 AND field = ?3 ORDER BY id DESC"
                ),
                Params::Positional(vec![json!(source), json!(key.to_string()), json!(field)]),
            )
            .await?;
        Ok(rows.iter().filter_map(to_stored).collect())
    }
    async fn purge_revisions(&self, cutoff_ms: u64) -> PortResult<u64> {
        let out = self
            .exec_with(
                "DELETE FROM _ui_revisions WHERE recorded_at < ?1".into(),
                Params::Positional(vec![json!(i64::try_from(cutoff_ms).unwrap_or(i64::MAX))]),
            )
            .await?;
        Ok(out.changes as u64)
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
