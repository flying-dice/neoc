//! Live sources (ui-system.md §3.3, §3.7): parameterised, windowed, live reads of an app's data.
//!
//! A [`SourceDef`] is what Lua's `db:query` / `db:row` / `db:scalar` declared. A view *subscribes* to one
//! with params and a window and gets a snapshot; afterwards every commit that touches a table the query
//! reads re-runs the subscription (in one read transaction, so the list, the detail and the counts agree)
//! and the result is diffed against what that subscriber last received: the ordered window keys plus only
//! the rows whose content hash changed. Cost is bounded by window size, not table size.
//!
//! These types are internal to the host. The protocol messages (`data.patch`) are built from them by the
//! driving adapter.

use crate::data::{AppData, ChangeBatch, ChangeFeed, DataTx, Params, PortResult, QueryInfo};
use crate::error::PortError;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::Arc,
};

/// Hard cap on rows in one window (ui-system.md §3.7 budgets).
pub const MAX_WINDOW: u64 = 500;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub String);

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotalMode {
    Exact,
    /// Skips the `count(*)` (huge tables): the total is unknown.
    Estimate,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    /// A stale two-way write still lands (the human wins).
    HumanWins,
    /// A stale write is refused and the client adopts the current value (approvals).
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Rows {
        key: String,
        page: Option<u32>,
        total: TotalMode,
    },
    Row {
        key: String,
        rev: Option<String>,
        writable: Option<Vec<String>>,
        has_write: bool,
        conflict: Conflict,
    },
    Scalar,
    /// A chat thread (`ChatTranscript`): it only names a thread and its agent. Its data is not a query: it
    /// arrives over `/agui`, so it is declared to the client and never subscribed.
    Thread,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDef {
    pub id: SourceId,
    pub kind: SourceKind,
    pub sql: String,
    pub info: QueryInfo,
}

impl SourceDef {
    /// Check the declaration against what preparing the SQL found.
    pub fn validate(&self) -> Result<(), String> {
        if !self.info.read_only {
            return Err("a source must be a read-only query".into());
        }
        let has_column = |c: &str| self.info.columns.iter().any(|x| x == c);
        match &self.kind {
            SourceKind::Rows { key, page, .. } => {
                if !has_column(key) {
                    return Err(format!("key column `{key}` is not selected by the query"));
                }
                if let Some(page) = page {
                    if *page == 0 || u64::from(*page) > MAX_WINDOW {
                        return Err(format!("page must be 1..={MAX_WINDOW}"));
                    }
                    if !has_total_order(&self.sql, key) {
                        return Err(format!(
                            "a windowed query needs a total order: end its `order by` with the key `{key}`"
                        ));
                    }
                }
            }
            SourceKind::Row { key, rev, .. } => {
                if !has_column(key) {
                    return Err(format!("key column `{key}` is not selected by the query"));
                }
                if let Some(rev) = rev {
                    if !has_column(rev) {
                        return Err(format!("rev column `{rev}` is not selected by the query"));
                    }
                }
            }
            SourceKind::Scalar => {
                if self.info.columns.len() != 1 {
                    return Err("a scalar source selects exactly one column".into());
                }
            }
            SourceKind::Thread => {}
        }
        Ok(())
    }

    /// The default window of a rows source.
    #[must_use]
    pub fn default_window(&self) -> Window {
        match &self.kind {
            SourceKind::Rows { page: Some(p), .. } => Window { from: 0, to: u64::from(*p) },
            _ => Window { from: 0, to: MAX_WINDOW },
        }
    }
}

/// Does the query's last `ORDER BY` mention the key column, making the order total? A heuristic over the
/// SQL text (the key is unique, so ending the order by with it is what makes windows stable).
#[must_use]
pub fn has_total_order(sql: &str, key: &str) -> bool {
    let lower = sql.to_lowercase();
    let Some(at) = lower.rfind("order by") else { return false };
    let clause = &lower[at + "order by".len()..];
    let key = key.to_lowercase();
    clause.match_indices(&key).any(|(i, m)| {
        let before = clause[..i].chars().next_back();
        let after = clause[i + m.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        !word(before) && !word(after)
    })
}

/// `[from, to)` rows of a rows source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub from: u64,
    pub to: u64,
}

impl Window {
    fn limit(self) -> u64 {
        self.to.saturating_sub(self.from).clamp(1, MAX_WINDOW)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(pub u64);

// ---- deltas ---------------------------------------------------------------------------------------

/// A rows source patch: the whole window as an ordered key list plus only the rows that changed.
#[derive(Clone, Debug, PartialEq)]
pub struct RowsDelta {
    /// `None` when the source skips counting.
    pub total: Option<u64>,
    pub from: u64,
    /// The whole window, in order.
    pub keys: Vec<Value>,
    /// Rows whose content hash differs from what the subscriber last received (key, row).
    pub changed: Vec<(Value, Value)>,
    /// Keys that were in the previous window and are not in this one.
    pub dropped: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowDelta {
    /// `None`: no such row (any more).
    pub row: Option<Value>,
    pub rev: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceUpdate {
    Rows(RowsDelta),
    Row(RowDelta),
    Scalar(Value),
    /// Re-running the query failed (the app's SQL or data is wrong): the subscriber keeps its last value.
    Failed(String),
}

/// Receives the updates a refresh computes. The `/rpc` adapter implements it per view (`data.patch`).
#[async_trait]
pub trait LiveSink: Send + Sync {
    async fn deliver(&self, subscription: SubscriptionId, update: SourceUpdate);
}

// ---- state kept per subscription ------------------------------------------------------------------

fn content_hash(v: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    v.to_string().hash(&mut h);
    h.finish()
}

fn key_string(v: &Value) -> String {
    v.to_string()
}

#[derive(Clone, Debug, Default)]
struct RowsState {
    total: Option<u64>,
    from: u64,
    keys: Vec<Value>,
    hashes: HashMap<String, u64>,
}

#[derive(Clone, Debug)]
enum SourceState {
    Rows(RowsState),
    Row(u64),
    RowMissing,
    Scalar(Value),
}

/// Diff a fresh window against what the subscriber has. `None` when nothing the subscriber sees changed.
fn rows_delta(
    prev: Option<&RowsState>,
    total: Option<u64>,
    from: u64,
    rows: Vec<Value>,
    key: &str,
    force: bool,
) -> PortResult<(RowsState, Option<RowsDelta>)> {
    let mut keys = Vec::with_capacity(rows.len());
    let mut hashes = HashMap::with_capacity(rows.len());
    let mut changed = Vec::new();
    for row in rows {
        let k = row
            .get(key)
            .cloned()
            .ok_or_else(|| PortError::Rejected(format!("row has no key column `{key}`")))?;
        let h = content_hash(&row);
        let ks = key_string(&k);
        if hashes.insert(ks.clone(), h).is_some() {
            return Err(PortError::Rejected(format!(
                "duplicate key {ks} in a window: the key must be unique"
            )));
        }
        if prev.is_none_or(|p| p.hashes.get(&ks) != Some(&h)) {
            changed.push((k.clone(), row));
        }
        keys.push(k);
    }
    let dropped: Vec<Value> = prev
        .map(|p| p.keys.iter().filter(|k| !hashes.contains_key(&key_string(k))).cloned().collect())
        .unwrap_or_default();
    let unchanged = !force
        && prev.is_some_and(|p| p.keys == keys && p.total == total && p.from == from)
        && changed.is_empty()
        && dropped.is_empty();
    let state = RowsState { total, from, keys: keys.clone(), hashes };
    let delta = (!unchanged).then_some(RowsDelta { total, from, keys, changed, dropped });
    Ok((state, delta))
}

// ---- the engine -----------------------------------------------------------------------------------

struct Subscription {
    def: Arc<SourceDef>,
    params: Params,
    window: Window,
    sink: Arc<dyn LiveSink>,
    state: Option<SourceState>,
    /// Bumped by `update`: a refresh that started before it must not overwrite the newer state.
    epoch: u64,
}

/// One subscription to re-run in a refresh.
struct Job {
    id: SubscriptionId,
    def: Arc<SourceDef>,
    params: Params,
    window: Window,
    prev: Option<SourceState>,
    epoch: u64,
}

/// One app's live sources and their subscribers.
pub struct LiveSources {
    data: Arc<dyn AppData>,
    defs: HashMap<SourceId, Arc<SourceDef>>,
    subs: Mutex<HashMap<SubscriptionId, Subscription>>,
    next: Mutex<u64>,
}

/// A read target: the database, or a transaction giving a consistent snapshot across several reads.
enum Reader<'a> {
    Data(&'a dyn AppData),
    Tx(&'a mut dyn DataTx),
}
impl Reader<'_> {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        match self {
            Reader::Data(d) => d.query(sql, params).await,
            Reader::Tx(t) => t.query(sql, params).await,
        }
    }
}

fn wrap(sql: &str, head: &str, tail: &str) -> String {
    format!("{head} (\n{}\n) {tail}", sql.trim().trim_end_matches(';').trim_end())
}

enum Fetched {
    Rows { total: Option<u64>, rows: Vec<Value> },
    Row(Option<Value>),
    Scalar(Value),
}

async fn fetch(
    reader: &mut Reader<'_>,
    def: &SourceDef,
    params: &Params,
    window: Window,
) -> PortResult<Fetched> {
    match &def.kind {
        SourceKind::Rows { total, .. } => {
            let page_sql = wrap(&def.sql, "select * from", "limit :__limit offset :__offset");
            let page_params =
                params.clone().with("__limit", window.limit().into()).with("__offset", window.from.into());
            let rows = reader.query(page_sql, page_params).await?;
            let total = match total {
                TotalMode::Exact => {
                    let n =
                        reader.query(wrap(&def.sql, "select count(*) as n from", ""), params.clone()).await?;
                    Some(n.first().and_then(|r| r.get("n")).and_then(Value::as_u64).unwrap_or(0))
                }
                TotalMode::Estimate | TotalMode::None => None,
            };
            Ok(Fetched::Rows { total, rows })
        }
        SourceKind::Row { .. } => {
            let mut rows = reader.query(def.sql.clone(), params.clone()).await?;
            rows.truncate(1);
            Ok(Fetched::Row(rows.pop()))
        }
        SourceKind::Thread => Err(PortError::Internal("a thread source has no query to run".into())),
        SourceKind::Scalar => {
            let rows = reader.query(def.sql.clone(), params.clone()).await?;
            let value = rows
                .first()
                .and_then(|r| r.as_object())
                .and_then(|o| o.values().next())
                .cloned()
                .unwrap_or(Value::Null);
            Ok(Fetched::Scalar(value))
        }
    }
}

/// Compute the update for a fetch against the subscriber's previous state; returns the new state.
fn compute(
    def: &SourceDef,
    prev: Option<&SourceState>,
    from: u64,
    fetched: Fetched,
    force: bool,
) -> PortResult<(SourceState, Option<SourceUpdate>)> {
    match (fetched, &def.kind) {
        (Fetched::Rows { total, rows }, SourceKind::Rows { key, .. }) => {
            let prev = match prev {
                Some(SourceState::Rows(r)) => Some(r),
                _ => None,
            };
            let (state, delta) = rows_delta(prev, total, from, rows, key, force)?;
            Ok((SourceState::Rows(state), delta.map(SourceUpdate::Rows)))
        }
        (Fetched::Row(row), SourceKind::Row { rev, .. }) => {
            let Some(row) = row else {
                let update = (force || !matches!(prev, Some(SourceState::RowMissing)))
                    .then_some(SourceUpdate::Row(RowDelta { row: None, rev: None }));
                return Ok((SourceState::RowMissing, update));
            };
            let hash = content_hash(&row);
            let same = !force && matches!(prev, Some(SourceState::Row(h)) if *h == hash);
            let rev = rev.as_ref().and_then(|c| row.get(c)).cloned();
            Ok((
                SourceState::Row(hash),
                (!same).then_some(SourceUpdate::Row(RowDelta { row: Some(row), rev })),
            ))
        }
        (Fetched::Scalar(value), SourceKind::Scalar) => {
            let same = !force && matches!(prev, Some(SourceState::Scalar(v)) if *v == value);
            Ok((SourceState::Scalar(value.clone()), (!same).then_some(SourceUpdate::Scalar(value))))
        }
        _ => Err(PortError::Internal("source kind and fetch result disagree".into())),
    }
}

impl LiveSources {
    #[must_use]
    pub fn new(data: Arc<dyn AppData>, defs: Vec<SourceDef>) -> Self {
        let defs = defs.into_iter().map(|d| (d.id.clone(), Arc::new(d))).collect();
        Self { data, defs, subs: Mutex::default(), next: Mutex::new(1) }
    }

    #[must_use]
    pub fn def(&self, id: &SourceId) -> Option<Arc<SourceDef>> {
        self.defs.get(id).cloned()
    }

    #[must_use]
    pub fn defs(&self) -> Vec<Arc<SourceDef>> {
        let mut v: Vec<_> = self.defs.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subs.lock().len()
    }

    /// Check params against the declared SQL: every `:name` the SQL uses must be given, none may be extra.
    fn check_params(def: &SourceDef, params: &Params) -> PortResult<()> {
        let given: Vec<&str> = match params {
            Params::Named(p) => p.iter().map(|(k, _)| k.as_str()).collect(),
            _ => vec![],
        };
        for needed in &def.info.named_params {
            if !given.contains(&needed.as_str()) {
                return Err(PortError::Rejected(format!("source `{}` needs param :{needed}", def.id)));
            }
        }
        for extra in given {
            if !def.info.named_params.iter().any(|n| n == extra) {
                return Err(PortError::Rejected(format!("source `{}` has no param :{extra}", def.id)));
            }
        }
        Ok(())
    }

    /// Subscribe and run the first query. The snapshot is returned (the caller puts it in `view.open`);
    /// only later changes go through `sink`.
    pub async fn subscribe(
        &self,
        source: &SourceId,
        params: Params,
        window: Option<Window>,
        sink: Arc<dyn LiveSink>,
    ) -> PortResult<(SubscriptionId, SourceUpdate)> {
        let def = self.def(source).ok_or_else(|| PortError::NotFound(format!("source `{source}`")))?;
        Self::check_params(&def, &params)?;
        let window = window.unwrap_or_else(|| def.default_window());
        let fetched = fetch(&mut Reader::Data(self.data.as_ref()), &def, &params, window).await?;
        let (state, update) = compute(&def, None, window.from, fetched, true)?;
        let id = {
            let mut n = self.next.lock();
            let id = SubscriptionId(*n);
            *n += 1;
            id
        };
        self.subs.lock().insert(id, Subscription { def, params, window, sink, state: Some(state), epoch: 0 });
        let update = update.ok_or_else(|| PortError::Internal("a first snapshot is never empty".into()))?;
        Ok((id, update))
    }

    /// `source.set`: change params and/or window; returns the delta against what the subscriber has.
    pub async fn update(
        &self,
        id: SubscriptionId,
        params: Option<Params>,
        window: Option<Window>,
    ) -> PortResult<Option<SourceUpdate>> {
        self.update_inner(id, params, window, false).await
    }

    /// Like [`LiveSources::update`], but always answers: an unchanged window comes back as a delta with
    /// the keys and no rows (the client asked, so it gets a reply).
    pub async fn update_always(
        &self,
        id: SubscriptionId,
        params: Option<Params>,
        window: Option<Window>,
    ) -> PortResult<SourceUpdate> {
        self.update_inner(id, params, window, true)
            .await?
            .ok_or_else(|| PortError::Internal("a forced update always answers".into()))
    }

    /// The params a subscription currently runs with (they select the row of a row source).
    #[must_use]
    pub fn params_of(&self, id: SubscriptionId) -> Option<Params> {
        self.subs.lock().get(&id).map(|s| s.params.clone())
    }

    async fn update_inner(
        &self,
        id: SubscriptionId,
        params: Option<Params>,
        window: Option<Window>,
        force: bool,
    ) -> PortResult<Option<SourceUpdate>> {
        let (def, params, window, prev, epoch) = {
            let mut subs = self.subs.lock();
            let sub =
                subs.get_mut(&id).ok_or_else(|| PortError::NotFound(format!("subscription {}", id.0)))?;
            if let Some(p) = &params {
                Self::check_params(&sub.def, p)?;
            }
            sub.epoch += 1;
            (
                sub.def.clone(),
                params.unwrap_or_else(|| sub.params.clone()),
                window.unwrap_or(sub.window),
                sub.state.clone(),
                sub.epoch,
            )
        };
        let fetched = fetch(&mut Reader::Data(self.data.as_ref()), &def, &params, window).await?;
        let (state, update) = compute(&def, prev.as_ref(), window.from, fetched, force)?;
        if let Some(sub) = self.subs.lock().get_mut(&id) {
            if sub.epoch == epoch {
                sub.params = params;
                sub.window = window;
                sub.state = Some(state);
            }
        }
        Ok(update)
    }

    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.subs.lock().remove(&id);
    }

    /// Re-run every subscription whose query reads a changed table, in one read transaction, and deliver
    /// the deltas. Returns how many updates were delivered.
    pub async fn refresh(&self, changed: &ChangeBatch) -> usize {
        let affected: Vec<Job> = self
            .subs
            .lock()
            .iter()
            .filter(|(_, s)| changed.touches(&s.def.info.tables))
            .map(|(id, s)| Job {
                id: *id,
                def: s.def.clone(),
                params: s.params.clone(),
                window: s.window,
                prev: s.state.clone(),
                epoch: s.epoch,
            })
            .collect();
        if affected.is_empty() {
            return 0;
        }
        // one snapshot for every affected source: the list, the detail and the counts agree
        let mut results = Vec::with_capacity(affected.len());
        match self.data.begin().await {
            Ok(mut tx) => {
                for job in &affected {
                    results
                        .push(fetch(&mut Reader::Tx(tx.as_mut()), &job.def, &job.params, job.window).await);
                }
                if let Err(e) = tx.rollback().await {
                    tracing::warn!(error = %e, "ending the live-source snapshot failed");
                }
            }
            Err(e) => results.extend(affected.iter().map(|_| Err(e.clone()))),
        }
        let mut deliveries = Vec::new();
        for (Job { id, def, window, prev, epoch, .. }, result) in affected.into_iter().zip(results) {
            let outcome = result.and_then(|f| compute(&def, prev.as_ref(), window.from, f, false));
            let mut subs = self.subs.lock();
            let Some(sub) = subs.get_mut(&id) else { continue };
            if sub.epoch != epoch {
                continue; // the subscriber moved on while we were reading
            }
            match outcome {
                Ok((state, update)) => {
                    sub.state = Some(state);
                    if let Some(update) = update {
                        deliveries.push((sub.sink.clone(), id, update));
                    }
                }
                Err(e) => deliveries.push((sub.sink.clone(), id, SourceUpdate::Failed(e.to_string()))),
            }
        }
        let n = deliveries.len();
        for (sink, id, update) in deliveries {
            sink.deliver(id, update).await;
        }
        n
    }
}

/// Drive a [`LiveSources`] from a database's change feed until the feed ends. The host spawns this per app.
pub async fn pump_changes(mut feed: Box<dyn ChangeFeed>, live: Arc<LiveSources>) {
    while let Some(batch) = feed.next_batch().await {
        live.refresh(&batch).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{CollectingSink, ScriptedData};
    use serde_json::json;

    fn rows_def(page: Option<u32>) -> SourceDef {
        SourceDef {
            id: SourceId("notes".into()),
            kind: SourceKind::Rows { key: "id".into(), page, total: TotalMode::Exact },
            sql: "select id, title from notes order by title, id".into(),
            info: QueryInfo {
                tables: ["notes".to_owned()].into(),
                columns: vec!["id".into(), "title".into()],
                named_params: vec![],
                read_only: true,
            },
        }
    }
    fn note(id: i64, title: &str) -> Value {
        json!({"id": id, "title": title})
    }
    fn setup(page: Option<u32>, rows: Vec<Value>) -> (Arc<ScriptedData>, LiveSources, Arc<CollectingSink>) {
        let data = Arc::new(ScriptedData::default());
        data.set_rows("from notes", rows);
        let live = LiveSources::new(data.clone(), vec![rows_def(page)]);
        (data, live, Arc::new(CollectingSink::default()))
    }

    #[test]
    fn total_order_means_the_order_by_ends_with_the_key() {
        assert!(has_total_order("select * from t order by updated_at desc, id desc", "id"));
        assert!(has_total_order("SELECT * FROM t ORDER BY id", "id"));
        assert!(!has_total_order("select * from t order by updated_at desc", "id"));
        assert!(!has_total_order("select * from t", "id"));
        assert!(!has_total_order("select * from t order by paid", "id"), "a longer word is not the key");
        assert!(!has_total_order("select * from t order by identity", "id"));
    }

    #[test]
    fn definitions_are_validated_against_the_prepared_query() {
        assert!(rows_def(Some(60)).validate().is_ok());
        let mut d = rows_def(Some(60));
        d.sql = "select id, title from notes order by title".into();
        assert!(d.validate().unwrap_err().contains("total order"));
        let mut d = rows_def(None);
        d.sql = "select id, title from notes".into();
        assert!(d.validate().is_ok(), "an un-windowed source needs no order");
        let mut d = rows_def(None);
        d.info.columns = vec!["title".into()];
        assert!(d.validate().unwrap_err().contains("key column"));
        let mut d = rows_def(None);
        d.info.read_only = false;
        assert!(d.validate().is_err());
        assert!(rows_def(Some(0)).validate().is_err());
        assert!(rows_def(Some(501)).validate().is_err());
    }

    #[test]
    fn the_first_delta_sends_every_row_and_a_repeat_sends_nothing() {
        let rows = vec![note(1, "a"), note(2, "b")];
        let (state, delta) = rows_delta(None, Some(2), 0, rows.clone(), "id", false).unwrap();
        let delta = delta.unwrap();
        assert_eq!(delta.keys, vec![json!(1), json!(2)]);
        assert_eq!(delta.changed.len(), 2);
        assert!(delta.dropped.is_empty());
        assert!(rows_delta(Some(&state), Some(2), 0, rows, "id", false).unwrap().1.is_none());
    }

    #[test]
    fn only_changed_rows_inserts_moves_and_removals_are_described() {
        let (state, _) =
            rows_delta(None, Some(3), 0, vec![note(1, "a"), note(2, "b"), note(3, "c")], "id", false)
                .unwrap();
        // 2 edited, 3 removed, 4 inserted at the front, 1 untouched but moved
        let (_, delta) =
            rows_delta(Some(&state), Some(3), 0, vec![note(4, "d"), note(2, "B"), note(1, "a")], "id", false)
                .unwrap();
        let d = delta.unwrap();
        assert_eq!(d.keys, vec![json!(4), json!(2), json!(1)], "the window is the new order");
        assert_eq!(
            d.changed,
            vec![(json!(4), note(4, "d")), (json!(2), note(2, "B"))],
            "row 1 is not resent"
        );
        assert_eq!(d.dropped, vec![json!(3)]);
    }

    #[test]
    fn a_reorder_alone_is_a_delta_with_no_rows() {
        let (state, _) = rows_delta(None, None, 0, vec![note(1, "a"), note(2, "b")], "id", false).unwrap();
        let d = rows_delta(Some(&state), None, 0, vec![note(2, "b"), note(1, "a")], "id", false)
            .unwrap()
            .1
            .unwrap();
        assert!(d.changed.is_empty() && d.dropped.is_empty());
        assert_eq!(d.keys, vec![json!(2), json!(1)]);
    }

    #[test]
    fn a_total_change_alone_is_reported() {
        let (state, _) = rows_delta(None, Some(5), 0, vec![note(1, "a")], "id", false).unwrap();
        let d = rows_delta(Some(&state), Some(6), 0, vec![note(1, "a")], "id", false).unwrap().1.unwrap();
        assert_eq!(d.total, Some(6));
        assert!(d.changed.is_empty());
    }

    #[test]
    fn duplicate_or_missing_keys_are_errors_not_silent_corruption() {
        assert!(rows_delta(None, None, 0, vec![note(1, "a"), note(1, "b")], "id", false).is_err());
        assert!(rows_delta(None, None, 0, vec![json!({"title": "no key"})], "id", false).is_err());
    }

    #[tokio::test]
    async fn a_commit_to_a_read_table_pushes_one_delta_and_other_tables_do_not() {
        let (data, live, sink) = setup(Some(2), vec![note(1, "a"), note(2, "b"), note(3, "c")]);
        let (sub, first) =
            live.subscribe(&SourceId("notes".into()), Params::None, None, sink.clone()).await.unwrap();
        let SourceUpdate::Rows(first) = first else { panic!("rows") };
        assert_eq!(first.keys, vec![json!(1), json!(2)], "page of 2");
        assert_eq!(first.total, Some(3));

        // unrelated table: nothing is re-run
        assert_eq!(live.refresh(&ChangeBatch::of(["tags"])).await, 0);
        assert!(sink.take().is_empty());

        data.set_rows("from notes", vec![note(0, "z"), note(1, "a"), note(2, "b"), note(3, "c")]);
        assert_eq!(live.refresh(&ChangeBatch::of(["Notes"])).await, 1, "table names are case-insensitive");
        let got = sink.take();
        assert_eq!(got.len(), 1);
        let (id, SourceUpdate::Rows(d)) = &got[0] else { panic!("rows") };
        assert_eq!(*id, sub);
        assert_eq!(d.keys, vec![json!(0), json!(1)], "the window slid: 2 left it");
        assert_eq!(d.changed, vec![(json!(0), note(0, "z"))], "row 1 was already there");
        assert_eq!(d.dropped, vec![json!(2)]);
        assert_eq!(d.total, Some(4));

        // same data again: no delta
        assert_eq!(live.refresh(&ChangeBatch::of(["notes"])).await, 0);
    }

    #[tokio::test]
    async fn a_refresh_reads_every_affected_source_in_one_transaction() {
        let (data, live, sink) = setup(None, vec![note(1, "a")]);
        live.subscribe(&SourceId("notes".into()), Params::None, None, sink.clone()).await.unwrap();
        live.subscribe(&SourceId("notes".into()), Params::None, None, sink.clone()).await.unwrap();
        data.set_rows("from notes", vec![note(1, "a2")]);
        assert_eq!(live.refresh(&ChangeBatch::of(["notes"])).await, 2);
        assert_eq!(data.transactions_begun(), 1, "one snapshot for both subscribers");
    }

    #[tokio::test]
    async fn update_changes_the_window_and_returns_the_delta_against_what_the_view_has() {
        let (_, live, sink) = setup(Some(2), vec![note(1, "a"), note(2, "b"), note(3, "c")]);
        let (sub, _) = live.subscribe(&SourceId("notes".into()), Params::None, None, sink).await.unwrap();
        let update = live.update(sub, None, Some(Window { from: 1, to: 3 })).await.unwrap().unwrap();
        let SourceUpdate::Rows(d) = update else { panic!("rows") };
        assert_eq!(d.keys, vec![json!(2), json!(3)]);
        assert_eq!(d.changed, vec![(json!(3), note(3, "c"))], "row 2 was in the previous window");
        assert_eq!(d.dropped, vec![json!(1)]);
        assert_eq!(d.from, 1);
    }

    #[tokio::test]
    async fn params_are_checked_against_the_sql() {
        let data = Arc::new(ScriptedData::default());
        data.set_rows("from notes", vec![]);
        let mut def = rows_def(None);
        def.info.named_params = vec!["folder".into()];
        let live = LiveSources::new(data, vec![def]);
        let sink = Arc::new(CollectingSink::default());
        let id = SourceId("notes".into());
        let missing = live.subscribe(&id, Params::None, None, sink.clone()).await.unwrap_err();
        assert!(missing.to_string().contains(":folder"), "{missing}");
        let extra = live
            .subscribe(&id, Params::named([("folder", json!(1)), ("q", json!("x"))]), None, sink.clone())
            .await
            .unwrap_err();
        assert!(extra.to_string().contains(":q"), "{extra}");
        assert!(live.subscribe(&id, Params::named([("folder", json!(1))]), None, sink).await.is_ok());
        assert!(matches!(
            live.subscribe(&SourceId("nope".into()), Params::None, None, Arc::new(CollectingSink::default()))
                .await,
            Err(PortError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn unsubscribed_views_get_nothing_and_a_failing_query_is_reported_not_fatal() {
        let (data, live, sink) = setup(None, vec![note(1, "a")]);
        let (sub, _) =
            live.subscribe(&SourceId("notes".into()), Params::None, None, sink.clone()).await.unwrap();
        data.fail_queries("boom");
        assert_eq!(live.refresh(&ChangeBatch::of(["notes"])).await, 1);
        let got = sink.take();
        assert!(matches!(&got[0].1, SourceUpdate::Failed(m) if m.contains("boom")), "{got:?}");
        data.fail_queries("");
        live.unsubscribe(sub);
        assert_eq!(live.subscription_count(), 0);
        assert_eq!(live.refresh(&ChangeBatch::of(["notes"])).await, 0);
    }

    #[tokio::test]
    async fn row_and_scalar_sources_diff_by_content() {
        let data = Arc::new(ScriptedData::default());
        data.set_rows("from notes where id", vec![json!({"id": 7, "body": "hi", "rev": 1})]);
        data.set_rows("count", vec![json!({"c": 3})]);
        let row = SourceDef {
            id: SourceId("note".into()),
            kind: SourceKind::Row {
                key: "id".into(),
                rev: Some("rev".into()),
                writable: None,
                has_write: true,
                conflict: Conflict::HumanWins,
            },
            sql: "select id, body, rev from notes where id = :id".into(),
            info: QueryInfo {
                tables: ["notes".to_owned()].into(),
                columns: vec!["id".into(), "body".into(), "rev".into()],
                named_params: vec!["id".into()],
                read_only: true,
            },
        };
        let scalar = SourceDef {
            id: SourceId("n".into()),
            kind: SourceKind::Scalar,
            sql: "select count(*) as c from notes".into(),
            info: QueryInfo {
                tables: ["notes".to_owned()].into(),
                columns: vec!["c".into()],
                named_params: vec![],
                read_only: true,
            },
        };
        let live = LiveSources::new(data.clone(), vec![row, scalar]);
        let sink = Arc::new(CollectingSink::default());
        let (_, first) = live
            .subscribe(&SourceId("note".into()), Params::named([("id", json!(7))]), None, sink.clone())
            .await
            .unwrap();
        assert_eq!(
            first,
            SourceUpdate::Row(RowDelta {
                row: Some(json!({"id":7,"body":"hi","rev":1})),
                rev: Some(json!(1))
            })
        );
        let (_, first) =
            live.subscribe(&SourceId("n".into()), Params::None, None, sink.clone()).await.unwrap();
        assert_eq!(first, SourceUpdate::Scalar(json!(3)));

        data.set_rows("from notes where id", vec![json!({"id": 7, "body": "hi!", "rev": 2})]);
        assert_eq!(live.refresh(&ChangeBatch::of(["notes"])).await, 1, "the count did not change");
        let got = sink.take();
        assert_eq!(
            got[0].1,
            SourceUpdate::Row(RowDelta {
                row: Some(json!({"id":7,"body":"hi!","rev":2})),
                rev: Some(json!(2))
            })
        );

        data.set_rows("from notes where id", vec![]);
        assert_eq!(live.refresh(&ChangeBatch::of(["notes"])).await, 1);
        assert_eq!(sink.take()[0].1, SourceUpdate::Row(RowDelta { row: None, rev: None }), "deleted row");
    }

    #[tokio::test]
    async fn the_pump_turns_a_change_feed_into_refreshes() {
        let (data, live, sink) = setup(None, vec![note(1, "a")]);
        let live = Arc::new(live);
        live.subscribe(&SourceId("notes".into()), Params::None, None, sink.clone()).await.unwrap();
        let feed = data.feed();
        let task = tokio::spawn(pump_changes(feed, live));
        data.set_rows("from notes", vec![note(1, "a"), note(2, "b")]);
        data.commit(["notes"]);
        for _ in 0..100 {
            if !sink.peek().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(sink.take().len(), 1);
        data.close_feed();
        task.await.unwrap();
    }
}
