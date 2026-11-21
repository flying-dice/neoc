//! An open transaction: it owns the database's write gate, so nothing else touches the connection until it
//! commits or rolls back (or is dropped, which rolls back).
//!
//! Beginning runs in a task of its own, to the end, whether or not the caller is still waiting: a caller that is
//! dropped between taking the gate and `BEGIN` running on the blocking pool would otherwise roll back *before*
//! the `BEGIN` it queued ran, and leave the connection inside a transaction nobody owns.

use crate::{run_exec, run_query, AppDb, DbError};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{oneshot, OwnedMutexGuard};
use vos_application::{
    data::{DataTx, ExecOutcome, Params, PortResult},
    PortError,
};

pub struct SqliteTx {
    db: AppDb,
    gate: Option<OwnedMutexGuard<()>>,
}

impl SqliteTx {
    pub(crate) async fn begin(db: &AppDb) -> Result<Self, DbError> {
        let db = db.clone();
        let (sent, received) = oneshot::channel();
        tokio::spawn(async move {
            // when nobody is waiting any more the transaction comes back here and is dropped: rolled back, gate free
            drop(sent.send(Self::open(&db).await));
        });
        received.await.map_err(|_| DbError::Cancelled)?
    }

    async fn open(db: &AppDb) -> Result<Self, DbError> {
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

/// The rollback of a transaction that was never committed, owed to the connection. Whoever drops it does it, so it is
/// done wherever the guard ends up: normally on the blocking pool (below), but if the runtime shuts down before that
/// job ran, the job is dropped unrun and this runs on the thread that drops it. The gate goes with it and is released
/// only after the rollback, so the next caller always finds a clean connection.
struct Rollback {
    db: AppDb,
    gate: Option<OwnedMutexGuard<()>>,
}

impl Drop for Rollback {
    fn drop(&mut self) {
        // errors only say there is no transaction left to undo. This blocks on the connection's lock and on SQLite,
        // which is acceptable only because it runs where blocking is harmless: on the blocking pool (the normal path),
        // or, when the runtime is shutting down and that job was dropped unrun, or there is no runtime, on the
        // thread that drops the job, at shutdown. The wait is bounded by the statement already running, if any.
        let _ = self.db.shared.conn.lock().execute_batch("ROLLBACK");
        drop(self.gate.take());
    }
}

impl Drop for SqliteTx {
    fn drop(&mut self) {
        // never committed: undo it so the next caller sees a clean connection. The rollback takes the connection's
        // lock and runs SQLite, which may block, so it does not run on the async worker that dropped us.
        let Some(gate) = self.gate.take() else { return };
        let owed = Rollback { db: self.db.clone(), gate: Some(gate) };
        match tokio::runtime::Handle::try_current() {
            // the closure owns the guard: when it never runs (shutdown), dropping the closure does the rollback
            Ok(runtime) => drop(runtime.spawn_blocking(move || drop(owed))),
            // not inside a runtime (nothing to block): do it here
            Err(_) => drop(owed),
        }
    }
}

#[async_trait]
impl DataTx for SqliteTx {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        let read_only = self.db.read_only;
        Ok(self.db.blocking(move |s| run_query(s, &sql, &params, read_only)).await?)
    }

    async fn exec(&mut self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        let read_only = self.db.read_only;
        let r = self.db.blocking(move |s| run_exec(s, &sql, &params, read_only)).await?;
        Ok(ExecOutcome { changes: r.changes as u64, last_insert_id: r.last_insert_rowid })
    }

    async fn commit(mut self: Box<Self>) -> PortResult<()> {
        self.end("COMMIT").await.map_err(PortError::from)
    }

    async fn rollback(mut self: Box<Self>) -> PortResult<()> {
        self.end("ROLLBACK").await.map_err(PortError::from)
    }
}
