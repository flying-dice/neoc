//! The change feed: commit batches coalesced into one batch per burst.

use crate::{AppDb, Change};
use vos_application::data::{ChangeBatch, ChangeFeed};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast::{self, error::RecvError};

/// How long after the first commit of a burst to keep collecting (one frame).
pub const COALESCE_WINDOW: Duration = Duration::from_millis(16);

/// A subscription to an [`AppDb`]'s commits. Several commits inside [`COALESCE_WINDOW`] become one batch.
pub struct SqliteFeed {
    rx: broadcast::Receiver<Arc<Vec<Change>>>,
    window: Duration,
}

impl SqliteFeed {
    #[must_use]
    pub fn new(db: &AppDb) -> Self {
        Self { rx: db.subscribe(), window: COALESCE_WINDOW }
    }

    #[must_use]
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }
}

fn add(batch: &mut ChangeBatch, changes: &[Change]) {
    batch.tables.extend(changes.iter().map(|c| c.table.to_lowercase()));
}

#[async_trait]
impl ChangeFeed for SqliteFeed {
    async fn next_batch(&mut self) -> Option<ChangeBatch> {
        let mut batch = ChangeBatch::default();
        match self.rx.recv().await {
            Ok(changes) => add(&mut batch, &changes),
            // we missed commits and cannot know which tables: everything must re-run
            Err(RecvError::Lagged(_)) => return Some(ChangeBatch::everything()),
            Err(RecvError::Closed) => return None,
        }
        let deadline = tokio::time::Instant::now() + self.window;
        loop {
            match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                Ok(Ok(changes)) => add(&mut batch, &changes),
                Ok(Err(RecvError::Lagged(_))) => return Some(ChangeBatch::everything()),
                Ok(Err(RecvError::Closed)) | Err(_) => return Some(batch),
            }
        }
    }
}
