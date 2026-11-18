//! An open transaction: it owns the database's write gate, so nothing else touches the connection until it
//! commits or rolls back (or is dropped, which rolls back).

use crate::{run_exec, run_query, AppDb, DbError};
use agentos_application::{
    data::{DataTx, ExecOutcome, Params, PortResult},
    PortError,
};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::OwnedMutexGuard;

pub struct SqliteTx {
    db: AppDb,
    gate: Option<OwnedMutexGuard<()>>,
}

impl SqliteTx {
    pub(crate) async fn begin(db: &AppDb) -> Result<Self, DbError> {
        let gate = db.gate().lock_owned().await;
        let this = Self { db: db.clone(), gate: Some(gate) };
        // from here a failure drops `this`, which releases the gate
        this.db.blocking(|s| Ok(s.conn.lock().execute_batch("BEGIN")?)).await?;
        Ok(this)
    }

    async fn end(&mut self, statement: &'static str) -> Result<(), DbError> {
        let result = self.db.blocking(move |s| Ok(s.conn.lock().execute_batch(statement)?)).await;
        if result.is_err() && statement == "COMMIT" {
            // a failed commit leaves the transaction open: end it so the connection is clean
            let _ = self.db.blocking(|s| Ok(s.conn.lock().execute_batch("ROLLBACK")?)).await;
        }
        self.gate = None; // finished either way
        result
    }
}

impl Drop for SqliteTx {
    fn drop(&mut self) {
        if self.gate.is_some() {
            // never committed: undo it so the next caller sees a clean connection
            let _ = self.db.shared.conn.lock().execute_batch("ROLLBACK");
        }
    }
}

#[async_trait]
impl DataTx for SqliteTx {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        Ok(self.db.blocking(move |s| run_query(s, &sql, &params)).await?)
    }

    async fn exec(&mut self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        let r = self.db.blocking(move |s| run_exec(s, &sql, &params)).await?;
        Ok(ExecOutcome { changes: r.changes as u64, last_insert_id: r.last_insert_rowid })
    }

    async fn commit(mut self: Box<Self>) -> PortResult<()> {
        self.end("COMMIT").await.map_err(PortError::from)
    }

    async fn rollback(mut self: Box<Self>) -> PortResult<()> {
        self.end("ROLLBACK").await.map_err(PortError::from)
    }
}
