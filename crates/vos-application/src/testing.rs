//! In-memory fakes of every port (no frameworks, no mocks library).

use crate::{
    files::{AppFiles, FileStore},
    live::{SourceDef, SourceId, SourceUpdate, SubscriptionId},
    ports::{
        AgentDirectory, AgentStore, AppData, AppPackage, AppRuntime, Background, CallContext, ChangeBatch,
        ChangeFeed, Clock, DataStore, DataTx, DeviceRegistry, EventSink, ExecOutcome, Fetcher, InstalledApp,
        InstalledAppStore, LoadedApp, Params, PortResult, QueryInfo, RandomSource, RenderOutput, ServerEvent,
        Shell, SourceBinding, SqlMigration, StoredThread, TaskHandle, ThreadStore, ToolRegistry, ViewEffect,
        ViewEventCall,
    },
    services::{AppService, Ports},
    PortError,
};
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Poll, Waker},
};
use vos_domain::{
    ui::{ClientDecl, Node},
    Agent, AgentId, AppId, AppRef, Device, DeviceId, FileInfo, FileKind, FilePath, FileRef, FileResult,
    Location, Manifest, MigrationSpec, Revision, StoredRevision, ToolSpec,
};

#[derive(Default)]
pub struct MemFetcher(pub Mutex<HashMap<String, Vec<u8>>>);
#[async_trait]
impl Fetcher for MemFetcher {
    async fn fetch(&self, url: &str) -> PortResult<Vec<u8>> {
        self.0.lock().unwrap().get(url).cloned().ok_or_else(|| crate::PortError::NotFound(url.into()))
    }
}

// ---- data -----------------------------------------------------------------------------------------

#[derive(Default)]
struct FeedState {
    queue: VecDeque<ChangeBatch>,
    waker: Option<Waker>,
    closed: bool,
}

#[derive(Default)]
struct Inner {
    rows: Mutex<Vec<(String, Vec<Value>)>>,
    tables: Mutex<Vec<String>>,
    fail: Mutex<String>,
    begun: AtomicU64,
    migrations: Mutex<Vec<SqlMigration>>,
    execs: Mutex<Vec<(String, Params)>>,
    feed: Mutex<FeedState>,
    revs: Mutex<Vec<StoredRevision>>,
    ui_state: Mutex<HashMap<String, serde_json::Map<String, Value>>>,
}

/// A scripted database: rows are registered per SQL fragment and returned for any statement containing it
/// (the longest fragment wins). Windowed (`limit :__limit offset :__offset`) and `count(*) as n` wrappers
/// are interpreted, so a source behaves like it does over SQLite.
#[derive(Default, Clone)]
pub struct ScriptedData(Arc<Inner>);

impl ScriptedData {
    pub fn set_rows(&self, sql_fragment: &str, rows: Vec<Value>) {
        let mut all = self.0.rows.lock().unwrap();
        all.retain(|(k, _)| k != sql_fragment);
        all.push((sql_fragment.to_owned(), rows));
    }
    /// Every query fails with this message while it is non-empty.
    pub fn fail_queries(&self, message: &str) {
        message.clone_into(&mut self.0.fail.lock().unwrap());
    }
    pub fn transactions_begun(&self) -> u64 {
        self.0.begun.load(Ordering::SeqCst)
    }
    pub fn migrations(&self) -> Vec<SqlMigration> {
        self.0.migrations.lock().unwrap().clone()
    }
    pub fn execs(&self) -> Vec<(String, Params)> {
        self.0.execs.lock().unwrap().clone()
    }
    /// A commit touching these tables, as the database's commit hook would report it.
    pub fn commit<'a>(&self, tables: impl IntoIterator<Item = &'a str>) {
        let mut feed = self.0.feed.lock().unwrap();
        feed.queue.push_back(ChangeBatch::of(tables));
        if let Some(w) = feed.waker.take() {
            w.wake();
        }
    }
    /// End every change feed (the database closed).
    pub fn close_feed(&self) {
        let mut feed = self.0.feed.lock().unwrap();
        feed.closed = true;
        if let Some(w) = feed.waker.take() {
            w.wake();
        }
    }
    pub fn feed(&self) -> Box<dyn ChangeFeed> {
        Box::new(ScriptedFeed(self.0.clone()))
    }

    fn run(&self, sql: &str, params: &Params) -> PortResult<Vec<Value>> {
        let fail = self.0.fail.lock().unwrap().clone();
        if !fail.is_empty() {
            return Err(PortError::Rejected(fail));
        }
        let all = self.0.rows.lock().unwrap();
        let best = all.iter().filter(|(k, _)| sql.contains(k.as_str())).max_by_key(|(k, _)| k.len());
        let rows = best.map_or_default(|(_, r)| r.clone());
        if sql.contains("count(*) as n from (") {
            return Ok(vec![json!({ "n": rows.len() })]);
        }
        if let (Some(limit), Some(offset)) = (params.get("__limit"), params.get("__offset")) {
            let (limit, offset) = (
                usize::try_from(limit.as_u64().unwrap_or(0)).unwrap_or(0),
                usize::try_from(offset.as_u64().unwrap_or(0)).unwrap_or(0),
            );
            return Ok(rows.into_iter().skip(offset).take(limit).collect());
        }
        Ok(rows)
    }
}

struct ScriptedFeed(Arc<Inner>);
#[async_trait]
impl ChangeFeed for ScriptedFeed {
    async fn next_batch(&mut self) -> Option<ChangeBatch> {
        let inner = self.0.clone();
        std::future::poll_fn(move |cx| {
            let mut feed = inner.feed.lock().unwrap();
            if let Some(b) = feed.queue.pop_front() {
                Poll::Ready(Some(b))
            } else if feed.closed {
                Poll::Ready(None)
            } else {
                feed.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

struct ScriptedTx(ScriptedData);
#[async_trait]
impl DataTx for ScriptedTx {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        self.0.run(&sql, &params)
    }
    async fn exec(&mut self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        AppData::exec(&self.0, sql, params).await
    }
    async fn commit(self: Box<Self>) -> PortResult<()> {
        Ok(())
    }
    async fn rollback(self: Box<Self>) -> PortResult<()> {
        Ok(())
    }
}

/// Named parameters (`:name`) in a SQL string, in order of first use.
fn named_params(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' && i + 1 < bytes.len() && bytes[i + 1] != b':' && (i == 0 || bytes[i - 1] != b':')
        {
            let end = bytes[i + 1..]
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
                .map_or(bytes.len(), |p| i + 1 + p);
            let name = &sql[i + 1..end];
            if !name.is_empty() && !out.iter().any(|n| n == name) && !name.starts_with("__") {
                out.push(name.to_owned());
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

#[async_trait]
impl AppData for ScriptedData {
    async fn migrate(&self, migrations: Vec<SqlMigration>) -> PortResult<()> {
        self.0.migrations.lock().unwrap().extend(migrations);
        Ok(())
    }
    async fn query(&self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        self.run(&sql, &params)
    }
    async fn exec(&self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        let mut execs = self.0.execs.lock().unwrap();
        execs.push((sql, params));
        Ok(ExecOutcome { changes: 1, last_insert_id: i64::try_from(execs.len()).unwrap_or(0) })
    }
    async fn analyze(&self, sql: String) -> PortResult<QueryInfo> {
        let mut tables: std::collections::BTreeSet<String> =
            self.0.tables.lock().unwrap().iter().cloned().collect();
        if tables.is_empty() {
            tables.insert("notes".into());
        }
        let columns = self
            .run(&sql, &Params::None)
            .ok()
            .and_then(|r| r.first().and_then(|v| v.as_object().map(|o| o.keys().cloned().collect())))
            .unwrap_or_default();
        Ok(QueryInfo {
            tables,
            columns,
            named_params: named_params(&sql),
            read_only: sql.trim_start().to_lowercase().starts_with("select"),
        })
    }
    async fn begin(&self) -> PortResult<Box<dyn DataTx>> {
        self.0.begun.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(ScriptedTx(self.clone())))
    }
    fn changes(&self) -> Box<dyn ChangeFeed> {
        self.feed()
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        Arc::new(ReadOnlyData(self.clone()))
    }
    async fn ui_state(&self, window: &str) -> PortResult<serde_json::Map<String, Value>> {
        Ok(self.0.ui_state.lock().unwrap().get(window).cloned().unwrap_or_default())
    }
    async fn set_ui_state(&self, window: &str, entries: serde_json::Map<String, Value>) -> PortResult<()> {
        self.0.ui_state.lock().unwrap().entry(window.to_owned()).or_default().extend(entries);
        Ok(())
    }
    async fn clear_ui_state(&self) -> PortResult<()> {
        self.0.ui_state.lock().unwrap().clear();
        Ok(())
    }
    async fn record_revision(&self, revision: Revision) -> PortResult<i64> {
        let mut all = self.0.revs.lock().unwrap();
        let id = i64::try_from(all.len()).unwrap_or(0) + 1;
        all.push(StoredRevision { id, revision });
        Ok(id)
    }
    async fn revision(&self, id: i64) -> PortResult<Option<StoredRevision>> {
        Ok(self.0.revs.lock().unwrap().iter().find(|r| r.id == id).cloned())
    }
    async fn revisions(&self, source: &str, key: &Value, field: &str) -> PortResult<Vec<StoredRevision>> {
        let mut v: Vec<StoredRevision> = self
            .0
            .revs
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.revision.source == source && &r.revision.key == key && r.revision.field == field)
            .cloned()
            .collect();
        v.reverse();
        Ok(v)
    }
    async fn purge_revisions(&self, cutoff_ms: u64) -> PortResult<u64> {
        let mut all = self.0.revs.lock().unwrap();
        let before = all.len();
        all.retain(|r| r.revision.recorded_at_ms >= cutoff_ms);
        Ok(u64::try_from(before - all.len()).unwrap_or(0))
    }
}

/// The read-only face of a [`ScriptedData`]: reads pass through, every write is refused.
struct ReadOnlyData(ScriptedData);

fn refused<T>() -> PortResult<T> {
    Err(PortError::Rejected("attempt to write a readonly database".into()))
}

struct ReadOnlyTx(ScriptedTx);
#[async_trait]
impl DataTx for ReadOnlyTx {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        self.0.query(sql, params).await
    }
    async fn exec(&mut self, _sql: String, _params: Params) -> PortResult<ExecOutcome> {
        refused()
    }
    async fn commit(self: Box<Self>) -> PortResult<()> {
        Ok(())
    }
    async fn rollback(self: Box<Self>) -> PortResult<()> {
        Ok(())
    }
}

#[async_trait]
impl AppData for ReadOnlyData {
    async fn migrate(&self, _migrations: Vec<SqlMigration>) -> PortResult<()> {
        refused()
    }
    async fn query(&self, sql: String, params: Params) -> PortResult<Vec<Value>> {
        self.0.query(sql, params).await
    }
    async fn exec(&self, _sql: String, _params: Params) -> PortResult<ExecOutcome> {
        refused()
    }
    async fn analyze(&self, sql: String) -> PortResult<QueryInfo> {
        self.0.analyze(sql).await
    }
    async fn begin(&self) -> PortResult<Box<dyn DataTx>> {
        Ok(Box::new(ReadOnlyTx(ScriptedTx(self.0.clone()))))
    }
    fn changes(&self) -> Box<dyn ChangeFeed> {
        self.0.feed()
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        Arc::new(ReadOnlyData(self.0.clone()))
    }
    async fn ui_state(&self, window: &str) -> PortResult<serde_json::Map<String, Value>> {
        self.0.ui_state(window).await
    }
    async fn set_ui_state(&self, _window: &str, _entries: serde_json::Map<String, Value>) -> PortResult<()> {
        refused()
    }
    async fn clear_ui_state(&self) -> PortResult<()> {
        refused()
    }
    async fn record_revision(&self, _revision: Revision) -> PortResult<i64> {
        refused()
    }
    async fn revision(&self, id: i64) -> PortResult<Option<StoredRevision>> {
        self.0.revision(id).await
    }
    async fn revisions(&self, source: &str, key: &Value, field: &str) -> PortResult<Vec<StoredRevision>> {
        self.0.revisions(source, key, field).await
    }
    async fn purge_revisions(&self, _cutoff_ms: u64) -> PortResult<u64> {
        refused()
    }
}

/// Collects what a refresh delivers.
#[derive(Default)]
pub struct CollectingSink(Mutex<Vec<(SubscriptionId, SourceUpdate)>>);
impl CollectingSink {
    pub fn take(&self) -> Vec<(SubscriptionId, SourceUpdate)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
    pub fn peek(&self) -> Vec<(SubscriptionId, SourceUpdate)> {
        self.0.lock().unwrap().clone()
    }
}
#[async_trait]
impl crate::live::LiveSink for CollectingSink {
    async fn deliver(&self, subscription: SubscriptionId, update: SourceUpdate) {
        self.0.lock().unwrap().push((subscription, update));
    }
}

#[derive(Default)]
pub struct MemStore {
    opened: Mutex<HashMap<String, usize>>,
    dbs: Mutex<HashMap<String, ScriptedData>>,
    purged: Mutex<Vec<String>>,
}
impl MemStore {
    pub fn opened_with_migrations(&self, app: &str) -> Option<usize> {
        self.opened.lock().unwrap().get(app).copied()
    }
    pub fn purged(&self, app: &str) -> bool {
        self.purged.lock().unwrap().iter().any(|a| a == app)
    }
    /// The app's scripted database (created on first open).
    pub fn data(&self, app: &str) -> ScriptedData {
        self.dbs.lock().unwrap().entry(app.to_owned()).or_default().clone()
    }
}
#[async_trait]
impl DataStore for MemStore {
    async fn open(&self, app: &AppId, m: Vec<SqlMigration>) -> PortResult<Arc<dyn AppData>> {
        self.opened.lock().unwrap().insert(app.to_string(), m.len());
        let data = self.data(app.as_str());
        data.migrate(m).await?;
        Ok(Arc::new(data))
    }
    async fn purge(&self, app: &AppId) -> PortResult<()> {
        self.purged.lock().unwrap().push(app.to_string());
        self.dbs.lock().unwrap().remove(app.as_str());
        Ok(())
    }
}

// ---- runtime --------------------------------------------------------------------------------------

/// (view, window, params, restored values) of a window the fake runtime opened.
pub type OpenedView = (String, String, serde_json::Map<String, Value>, serde_json::Map<String, Value>);
/// (source, key, field, value, context) of a write-back the fake runtime ran.
pub type WriteRecord = (String, Value, String, Value, CallContext);
type WriteHook = Box<dyn Fn(&str, &Value, &str, &Value) + Send>;

#[derive(Default)]
pub struct FakeRuntime {
    apps: Mutex<HashMap<String, Vec<ToolSpec>>>,
    calls: Mutex<Vec<(String, String, CallContext)>>,
    /// `(app, handler, args)` of every client `rpc` call, in order.
    handler_calls: Mutex<Vec<(String, String, Value)>>,
    /// The `read_only` requirement each tool call arrived with, in call order.
    read_only_calls: Mutex<Vec<bool>>,
    loads: Mutex<HashMap<String, usize>>,
    packages: Mutex<HashMap<String, AppPackage>>,
    sources: Mutex<HashMap<String, Vec<SourceDef>>>,
    fail_load: Mutex<HashSet<String>>,
    writes: Mutex<Vec<WriteRecord>>,
    on_write: Mutex<Option<WriteHook>>,
    view: Mutex<ViewScript>,
}

/// What the fake runtime renders and records for windows.
#[derive(Default)]
struct ViewScript {
    tree: Option<Node>,
    sources: BTreeMap<String, SourceBinding>,
    decls: BTreeMap<String, ClientDecl>,
    seq: u64,
    fail_render: Option<String>,
    stale: HashSet<String>,
    dirty: Vec<String>,
    effects: Vec<(String, ViewEffect)>,
    opened: Vec<OpenedView>,
    events: Vec<(String, ViewEventCall)>,
    closed: Vec<String>,
    client_values: Vec<(String, serde_json::Map<String, Value>)>,
    /// Makes the next `render_view` wait until the paired sender fires or is dropped.
    hold_render: Option<futures_channel::oneshot::Receiver<()>>,
}
impl FakeRuntime {
    pub fn loaded(&self) -> Vec<String> {
        self.apps.lock().unwrap().keys().cloned().collect()
    }
    pub fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
    /// Whether each tool call asked the runtime for a read-only run, in call order.
    pub fn read_only_calls(&self) -> Vec<bool> {
        self.read_only_calls.lock().unwrap().clone()
    }
    /// The context of the most recent tool call.
    pub fn last_ctx(&self) -> Option<CallContext> {
        self.calls.lock().unwrap().last().map(|c| c.2.clone())
    }
    /// How many times the app was loaded (install, enable, reload, delete data).
    pub fn loads(&self, app: &str) -> usize {
        self.loads.lock().unwrap().get(app).copied().unwrap_or(0)
    }
    pub fn package(&self, app: &str) -> Option<AppPackage> {
        self.packages.lock().unwrap().get(app).cloned()
    }
    pub fn set_sources(&self, app: &str, defs: Vec<SourceDef>) {
        self.sources.lock().unwrap().insert(app.to_owned(), defs);
    }
    /// The next loads of this app fail (until `fail_load(app, false)`).
    pub fn fail_load(&self, app: &str, fail: bool) {
        let mut set = self.fail_load.lock().unwrap();
        if fail {
            set.insert(app.to_owned());
        } else {
            set.remove(app);
        }
    }
    /// What every window renders next (open and re-render).
    pub fn set_render(
        &self,
        tree: Node,
        sources: BTreeMap<String, SourceBinding>,
        decls: BTreeMap<String, ClientDecl>,
    ) {
        let mut v = self.view.lock().unwrap();
        v.tree = Some(tree);
        v.sources = sources;
        v.decls = decls;
    }
    /// The next `render_view` call (arm it after the open) waits until the returned sender is used or dropped:
    /// a render that takes a while.
    pub fn hold_next_render(&self) -> futures_channel::oneshot::Sender<()> {
        let (release, held) = futures_channel::oneshot::channel();
        self.view.lock().unwrap().hold_render = Some(held);
        release
    }
    /// The next renders fail with this message.
    pub fn fail_render(&self, message: Option<&str>) {
        self.view.lock().unwrap().fail_render = message.map(str::to_owned);
    }
    /// `path#event` handlers that no longer exist (the event is stale).
    pub fn set_stale(&self, handler: &str) {
        self.view.lock().unwrap().stale.insert(handler.to_owned());
    }
    /// The next `dirty_views` call reports this view.
    pub fn mark_dirty(&self, view: &str) {
        self.view.lock().unwrap().dirty.push(view.to_owned());
    }
    pub fn push_effect(&self, view: &str, effect: ViewEffect) {
        self.view.lock().unwrap().effects.push((view.to_owned(), effect));
    }
    pub fn opened_views(&self) -> Vec<OpenedView> {
        self.view.lock().unwrap().opened.clone()
    }
    pub fn view_events(&self) -> Vec<(String, ViewEventCall)> {
        self.view.lock().unwrap().events.clone()
    }
    pub fn closed_views(&self) -> Vec<String> {
        self.view.lock().unwrap().closed.clone()
    }
    pub fn client_values(&self) -> Vec<(String, serde_json::Map<String, Value>)> {
        self.view.lock().unwrap().client_values.clone()
    }
    fn render(&self) -> PortResult<RenderOutput> {
        let mut v = self.view.lock().unwrap();
        if let Some(message) = &v.fail_render {
            return Err(PortError::Rejected(message.clone()));
        }
        let tree = v.tree.clone().ok_or_else(|| PortError::NotFound("no render scripted".into()))?;
        v.seq += 1;
        Ok(RenderOutput {
            tree,
            client_decls: v.decls.clone(),
            sources: v.sources.clone(),
            client: serde_json::Map::new(),
            seq: v.seq,
        })
    }
    /// `(app, handler, args)` of every client `rpc` call so far.
    pub fn handler_calls(&self) -> Vec<(String, String, Value)> {
        self.handler_calls.lock().unwrap().clone()
    }
    pub fn writes(&self) -> Vec<WriteRecord> {
        self.writes.lock().unwrap().clone()
    }
    /// Called with (source, key, field, value) when a write-back runs (to change the scripted rows).
    pub fn on_write(&self, hook: impl Fn(&str, &Value, &str, &Value) + Send + 'static) {
        *self.on_write.lock().unwrap() = Some(Box::new(hook));
    }
}
#[async_trait]
impl AppRuntime for FakeRuntime {
    async fn load(&self, app: &AppId, package: AppPackage, _: Arc<dyn AppData>) -> PortResult<LoadedApp> {
        *self.loads.lock().unwrap().entry(app.to_string()).or_default() += 1;
        if self.fail_load.lock().unwrap().contains(app.as_str()) {
            return Err(PortError::Rejected(format!("{app}/app.lua:3: boom")));
        }
        // the fake "app source" is a comma separated list of tool names
        let main = package.files.get(&package.entry).cloned().unwrap_or_default();
        let tools: Vec<ToolSpec> = String::from_utf8_lossy(&main)
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|n| ToolSpec {
                // a trailing `?` declares the tool readOnly: "search?,add"
                name: n.trim_end_matches('?').into(),
                title: None,
                description: String::new(),
                input_schema: json!({"type":"object"}),
                read_only: n.ends_with('?'),
            })
            .collect();
        self.apps.lock().unwrap().insert(app.to_string(), tools.clone());
        self.packages.lock().unwrap().insert(app.to_string(), package);
        let sources = self.sources.lock().unwrap().get(app.as_str()).cloned().unwrap_or_default();
        Ok(LoadedApp { decl: None, tools, sources })
    }
    async fn unload(&self, app: &AppId) -> PortResult<()> {
        self.apps.lock().unwrap().remove(app.as_str());
        Ok(())
    }
    async fn call_tool(
        &self,
        app: &AppId,
        tool: &str,
        args: Value,
        ctx: CallContext,
        read_only: bool,
    ) -> PortResult<Value> {
        self.calls.lock().unwrap().push((app.to_string(), tool.to_owned(), ctx));
        self.read_only_calls.lock().unwrap().push(read_only);
        Ok(json!({ "echo": args }))
    }
    async fn call_handler(
        &self,
        app: &AppId,
        method: &str,
        args: Value,
        _: CallContext,
    ) -> PortResult<Value> {
        self.handler_calls.lock().unwrap().push((app.to_string(), method.to_owned(), args.clone()));
        Ok(json!({ "app": app.as_str(), "method": method, "args": args }))
    }
    async fn write_row(
        &self,
        _: &AppId,
        source: &SourceId,
        key: Value,
        field: &str,
        value: Value,
        ctx: CallContext,
    ) -> PortResult<()> {
        if let Some(hook) = self.on_write.lock().unwrap().as_ref() {
            hook(&source.0, &key, field, &value);
        }
        self.writes.lock().unwrap().push((source.0.clone(), key, field.to_owned(), value, ctx));
        Ok(())
    }
    async fn windows(&self, _: &AppId) -> PortResult<Vec<String>> {
        Ok(vec!["main".into()])
    }
    async fn open_view(
        &self,
        _: &AppId,
        view: &str,
        window: &str,
        params: serde_json::Map<String, Value>,
        restored: serde_json::Map<String, Value>,
    ) -> PortResult<RenderOutput> {
        let mut out = self.render()?;
        restored.clone_into(&mut out.client);
        self.view.lock().unwrap().opened.push((view.to_owned(), window.to_owned(), params, restored));
        Ok(out)
    }
    async fn render_view(&self, _: &AppId, _: &str) -> PortResult<RenderOutput> {
        let held = self.view.lock().unwrap().hold_render.take();
        if let Some(held) = held {
            let _ = held.await;
        }
        self.render()
    }
    async fn dispatch_view_event(
        &self,
        _: &AppId,
        view: &str,
        call: ViewEventCall,
        _: CallContext,
    ) -> PortResult<bool> {
        let mut v = self.view.lock().unwrap();
        let stale = v.stale.contains(&format!("{}#{}", call.path, call.event));
        v.events.push((view.to_owned(), call));
        Ok(!stale)
    }
    async fn set_client_values(
        &self,
        _: &AppId,
        view: &str,
        values: serde_json::Map<String, Value>,
    ) -> PortResult<()> {
        self.view.lock().unwrap().client_values.push((view.to_owned(), values));
        Ok(())
    }
    async fn close_view(&self, _: &AppId, view: &str) -> PortResult<()> {
        self.view.lock().unwrap().closed.push(view.to_owned());
        Ok(())
    }
    async fn dirty_views(&self, _: &AppId) -> PortResult<Vec<String>> {
        Ok(std::mem::take(&mut self.view.lock().unwrap().dirty))
    }
    async fn take_view_effects(&self, _: &AppId) -> PortResult<Vec<(String, ViewEffect)>> {
        Ok(std::mem::take(&mut self.view.lock().unwrap().effects))
    }
}

#[derive(Default)]
pub struct MemRegistry(Mutex<HashMap<String, Vec<String>>>, Mutex<Vec<String>>);
impl MemRegistry {
    /// The agents whose tool lists were reported changed, in order.
    pub fn visibility_changes(&self) -> Vec<String> {
        self.1.lock().unwrap().clone()
    }
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .0
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(a, t)| t.iter().map(move |t| format!("{a}__{t}")))
            .collect();
        v.sort();
        v
    }
}
#[async_trait]
impl ToolRegistry for MemRegistry {
    async fn publish(&self, app: &AppId, tools: Vec<ToolSpec>) -> PortResult<()> {
        self.0.lock().unwrap().insert(app.to_string(), tools.into_iter().map(|t| t.name).collect());
        Ok(())
    }
    async fn retract(&self, app: &AppId) -> PortResult<()> {
        self.0.lock().unwrap().remove(app.as_str());
        Ok(())
    }
    async fn visibility_changed(&self, agent: &AgentId) -> PortResult<()> {
        self.1.lock().unwrap().push(agent.0.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct RecordingSink(Mutex<Vec<ServerEvent>>);
impl RecordingSink {
    pub fn take(&self) -> Vec<ServerEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}
#[async_trait]
impl EventSink for RecordingSink {
    async fn emit(&self, e: ServerEvent) {
        self.0.lock().unwrap().push(e);
    }
}

pub struct FixedClock(pub u64);
impl Clock for FixedClock {
    fn now_unix_ms(&self) -> u64 {
        self.0
    }
}

/// A clock tests move by hand.
pub struct ManualClock(AtomicU64);
impl ManualClock {
    pub fn new(start_ms: u64) -> Self {
        Self(AtomicU64::new(start_ms))
    }
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// In-memory `InstalledAppStore`.
#[derive(Default)]
pub struct MemInstalled(Mutex<Vec<InstalledApp>>);
impl MemInstalled {
    pub fn records(&self) -> Vec<InstalledApp> {
        self.0.lock().unwrap().clone()
    }
}
#[async_trait]
impl InstalledAppStore for MemInstalled {
    async fn save(&self, app: &InstalledApp) -> PortResult<()> {
        let mut all = self.0.lock().unwrap();
        all.retain(|a| a.id != app.id);
        all.push(app.clone());
        Ok(())
    }
    async fn remove(&self, id: &AppId) -> PortResult<()> {
        self.0.lock().unwrap().retain(|a| a.id != *id);
        Ok(())
    }
    async fn set_enabled(&self, id: &AppId, enabled: bool) -> PortResult<()> {
        if let Some(a) = self.0.lock().unwrap().iter_mut().find(|a| a.id == *id) {
            a.enabled = enabled;
        }
        Ok(())
    }
    async fn list(&self) -> PortResult<Vec<InstalledApp>> {
        Ok(self.records())
    }
}

/// `(app, window, params)` of an `app:openWindow`.
pub type OpenedWindow = (String, String, Map<String, Value>);

/// Records what apps asked of the shell.
#[derive(Default)]
pub struct RecordingShell {
    pub links: Mutex<Vec<(String, Vec<String>)>>,
    /// `(app, window, params)` of every `app:openWindow`.
    pub windows: Mutex<Vec<OpenedWindow>>,
    /// `(app, name, data)` of every `app.emit`.
    pub events: Mutex<Vec<(String, String, Value)>>,
}
#[async_trait]
impl Shell for RecordingShell {
    async fn open_window(&self, app: &AppId, window: &str, params: Map<String, Value>) {
        self.windows.lock().unwrap().push((app.to_string(), window.to_owned(), params));
    }
    async fn set_deep_links(&self, app: &AppId, patterns: Vec<String>) {
        self.links.lock().unwrap().push((app.to_string(), patterns));
    }
    async fn emit_event(&self, app: &AppId, name: &str, data: Value) {
        self.events.lock().unwrap().push((app.to_string(), name.to_owned(), data));
    }
}

/// Collects what a view sends to its client.
#[derive(Default)]
pub struct RecordingViewSink(Mutex<Vec<vos_domain::ui::protocol::HostNotification>>);
impl RecordingViewSink {
    pub fn take(&self) -> Vec<vos_domain::ui::protocol::HostNotification> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
    pub fn methods(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().iter().map(|n| n.to_rpc().0).collect()
    }
}
impl crate::views::ViewSink for RecordingViewSink {
    fn send(&self, notification: vos_domain::ui::protocol::HostNotification) {
        self.0.lock().unwrap().push(notification);
    }
}

/// A `Background` that never runs the task but counts the live handles (the pump's lifetime).
#[derive(Default)]
pub struct CountingBackground(Arc<AtomicUsize>);
impl CountingBackground {
    pub fn alive(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}
struct CountedHandle(Arc<AtomicUsize>);
impl TaskHandle for CountedHandle {}
impl Drop for CountedHandle {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Background for CountingBackground {
    fn spawn(&self, _: &'static str, _: Pin<Box<dyn Future<Output = ()> + Send>>) -> Box<dyn TaskHandle> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::new(CountedHandle(self.0.clone()))
    }
}

/// Deterministic "randomness": a counter run through SHA-256, so values differ but tests are repeatable.
#[derive(Default)]
pub struct SeededRandom(AtomicU64);
impl RandomSource for SeededRandom {
    fn fill(&self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(32) {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            let block = Sha256::digest(n.to_le_bytes());
            chunk.copy_from_slice(&block[..chunk.len()]);
        }
    }
}

#[derive(Default)]
struct DeviceTables {
    /// code hash -> expiry
    codes: HashMap<String, u64>,
    /// token hash -> device
    sessions: HashMap<String, Device>,
}

/// In-memory `DeviceRegistry`.
#[derive(Default)]
pub struct MemDevices(Mutex<DeviceTables>);
impl MemDevices {
    /// Everything stored, as one string (to assert that no secret is in it).
    pub fn dump(&self) -> String {
        let t = self.0.lock().unwrap();
        format!("{:?} {:?}", t.codes, t.sessions)
    }
}
#[async_trait]
impl DeviceRegistry for MemDevices {
    async fn add_pairing_code(&self, code_hash: &str, expires_at_ms: u64) -> PortResult<()> {
        self.0.lock().unwrap().codes.insert(code_hash.to_owned(), expires_at_ms);
        Ok(())
    }
    async fn take_pairing_code(&self, code_hash: &str, now_ms: u64) -> PortResult<bool> {
        let expiry = self.0.lock().unwrap().codes.remove(code_hash);
        Ok(expiry.is_some_and(|e| e > now_ms))
    }
    async fn clear_pairing_codes(&self) -> PortResult<()> {
        self.0.lock().unwrap().codes.clear();
        Ok(())
    }
    async fn add_session(&self, device: &Device, token_hash: &str) -> PortResult<()> {
        self.0.lock().unwrap().sessions.insert(token_hash.to_owned(), device.clone());
        Ok(())
    }
    async fn find_session(&self, token_hash: &str) -> PortResult<Option<Device>> {
        Ok(self.0.lock().unwrap().sessions.get(token_hash).cloned())
    }
    async fn touch_session(&self, id: &DeviceId, now_ms: u64) -> PortResult<()> {
        if let Some(d) = self.0.lock().unwrap().sessions.values_mut().find(|d| d.id == *id) {
            d.last_seen_at = now_ms;
        }
        Ok(())
    }
    async fn list_devices(&self) -> PortResult<Vec<Device>> {
        let mut all: Vec<Device> = self.0.lock().unwrap().sessions.values().cloned().collect();
        all.sort_by_key(|d| d.created_at);
        Ok(all)
    }
    async fn revoke_device(&self, id: &DeviceId) -> PortResult<bool> {
        let mut t = self.0.lock().unwrap();
        let before = t.sessions.len();
        t.sessions.retain(|_, d| d.id != *id);
        Ok(t.sessions.len() < before)
    }
}

/// Files that record what reached them (`"write documents notes/a.txt"`) and answer with empty success; the
/// use-case tests check what the port is, and is not, asked to do. Also the [`FileStore`] of [`Fakes`]: it
/// records purges.
#[derive(Default)]
pub struct RecordingFiles {
    calls: Mutex<Vec<String>>,
    purged: Mutex<Vec<String>>,
    fail_purge: std::sync::atomic::AtomicBool,
}
impl RecordingFiles {
    /// Make every later purge fail (after recording it), as a tree the host cannot remove would.
    pub fn fail_purges(&self) {
        self.fail_purge.store(true, Ordering::SeqCst);
    }
    /// Every call so far, in order, and forget them.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
    pub fn purged(&self, app: &str) -> bool {
        self.purged.lock().unwrap().iter().any(|a| a == app)
    }
    fn record(&self, what: &str, at: Location, paths: &[&FilePath]) {
        let paths: Vec<String> = paths.iter().map(ToString::to_string).collect();
        self.calls.lock().unwrap().push(format!("{what} {} {}", at.as_str(), paths.join(" ")));
    }
}
#[async_trait]
impl AppFiles for RecordingFiles {
    async fn read(&self, at: Location, path: &FilePath) -> FileResult<Vec<u8>> {
        self.record("read", at, &[path]);
        Ok(Vec::new())
    }
    async fn write(&self, at: Location, path: &FilePath, _: Vec<u8>) -> FileResult<()> {
        self.record("write", at, &[path]);
        Ok(())
    }
    async fn list(&self, at: Location, path: &FilePath) -> FileResult<Vec<FileInfo>> {
        self.record("list", at, &[path]);
        Ok(Vec::new())
    }
    async fn info(&self, at: Location, path: &FilePath) -> FileResult<FileInfo> {
        self.record("info", at, &[path]);
        Ok(FileInfo {
            name: path.name().unwrap_or(at.as_str()).to_owned(),
            kind: FileKind::File,
            size: 0,
            modified: None,
        })
    }
    async fn create_dir(&self, at: Location, path: &FilePath) -> FileResult<()> {
        self.record("create_dir", at, &[path]);
        Ok(())
    }
    async fn remove(&self, at: Location, path: &FilePath, recursive: bool) -> FileResult<()> {
        self.record(if recursive { "remove_all" } else { "remove" }, at, &[path]);
        Ok(())
    }
    async fn move_to(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()> {
        self.record("move", at, &[from, to]);
        Ok(())
    }
    async fn copy(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()> {
        self.record("copy", at, &[from, to]);
        Ok(())
    }
}
#[async_trait]
impl FileStore for RecordingFiles {
    fn files(&self, _: &AppId) -> Arc<dyn AppFiles> {
        Arc::new(RecordingFiles::default())
    }
    async fn purge(&self, app: &AppId) -> FileResult<()> {
        self.purged.lock().unwrap().push(app.to_string());
        if self.fail_purge.load(Ordering::SeqCst) {
            return Err(vos_domain::FileError::new(
                vos_domain::FileErrorKind::PermissionDenied,
                "the app's files: denied",
            ));
        }
        Ok(())
    }
}

/// Bundle of all fakes + helpers to build the service under test.
pub struct Fakes {
    pub fetcher: Arc<MemFetcher>,
    pub store: Arc<MemStore>,
    pub files: Arc<RecordingFiles>,
    pub runtime: Arc<FakeRuntime>,
    pub registry: Arc<MemRegistry>,
    pub events: Arc<RecordingSink>,
    pub installed: Arc<MemInstalled>,
    pub clock: Arc<ManualClock>,
    pub background: Arc<CountingBackground>,
}
impl Default for Fakes {
    fn default() -> Self {
        Self::new()
    }
}
impl Fakes {
    pub fn new() -> Self {
        Self {
            fetcher: Arc::default(),
            store: Arc::default(),
            files: Arc::default(),
            runtime: Arc::default(),
            registry: Arc::default(),
            events: Arc::default(),
            installed: Arc::default(),
            clock: Arc::new(ManualClock::new(1_000)),
            background: Arc::default(),
        }
    }
    /// A fresh service over these fakes (call twice to simulate a restart: the fakes keep their state).
    pub fn service(&self) -> AppService {
        self.service_with_tools(self.registry.clone())
    }
    /// Like `service`, with another `ToolRegistry` (the real MCP adapter, in its own tests).
    pub fn service_with_tools(&self, tools: Arc<dyn ToolRegistry>) -> AppService {
        self.service_with(tools, vec![])
    }
    /// Like `service`, browsing these registries (`mem://` URLs: see [`Fakes::add_registry`]).
    pub fn service_with_registries(&self, registries: Vec<String>) -> AppService {
        self.service_with(self.registry.clone(), registries)
    }
    fn service_with(&self, tools: Arc<dyn ToolRegistry>, registries: Vec<String>) -> AppService {
        AppService::new(Ports {
            fetcher: self.fetcher.clone(),
            data: self.store.clone(),
            files: self.files.clone(),
            runtime: self.runtime.clone(),
            tools,
            events: self.events.clone(),
            installed: self.installed.clone(),
            clock: self.clock.clone(),
            background: self.background.clone(),
            registries,
        })
    }
    /// Publish a registry file at `url` listing app-ref URLs (relative ones resolve against `url`).
    pub fn add_registry(&self, url: &str, apps: &[&str]) {
        let body = serde_json::to_vec(&json!({ "apps": apps })).unwrap();
        self.fetcher.0.lock().unwrap().insert(url.to_owned(), body);
    }
    /// Publish an app ref at `mem://<id>/app-ref.json`; `main` content = comma-joined tool names.
    pub fn add_app(&self, id: &str, tools: &[(&str, &str)]) -> String {
        self.add_app_with(id, tools, &[])
    }
    /// Like `add_app`, for an app with a client of its own: `static/index.html` is the page `client_source` and the
    /// manifest names the directory (and a window), so the service keeps it for the frame.
    pub fn add_client_app(&self, id: &str, tools: &[(&str, &str)], client_source: &str) -> String {
        let url = self.add_app_with(id, tools, &[("static/index.html", client_source)]);
        let mut mem = self.fetcher.0.lock().unwrap();
        let mut app_ref: AppRef = serde_json::from_slice(&mem[&url]).unwrap();
        app_ref.manifest.ui = Some(vos_domain::AppUi {
            root: "static".into(),
            entry: "index.html".into(),
            csp: vos_domain::Csp::default(),
            permissions: vos_domain::Permissions::default(),
        });
        app_ref.manifest.window = Some(vos_domain::WindowDecl { w: 420, h: 480, min_w: 320, min_h: 320 });
        mem.insert(url.clone(), serde_json::to_vec(&app_ref).unwrap());
        url
    }
    /// Like `add_app`, with extra `.lua` modules (path, source).
    pub fn add_app_with(&self, id: &str, tools: &[(&str, &str)], modules: &[(&str, &str)]) -> String {
        let main = tools.iter().map(|t| t.0).collect::<Vec<_>>().join(",").into_bytes();
        let sql = b"CREATE TABLE t(a);".to_vec();
        let sha = |b: &[u8]| format!("{:x}", Sha256::digest(b));
        let mut files = vec![
            FileRef { path: "main.lua".into(), url: format!("mem://{id}/main.lua"), sha256: sha(&main) },
            FileRef {
                path: "001_init.sql".into(),
                url: format!("mem://{id}/001_init.sql"),
                sha256: sha(&sql),
            },
        ];
        let mut mem = self.fetcher.0.lock().unwrap();
        for (path, src) in modules {
            files.push(FileRef {
                path: (*path).into(),
                url: format!("mem://{id}/{path}"),
                sha256: sha(src.as_bytes()),
            });
            mem.insert(format!("mem://{id}/{path}"), src.as_bytes().to_vec());
        }
        let r = AppRef {
            manifest: Manifest {
                id: AppId::parse(id).unwrap(),
                name: id.into(),
                version: "1.0.0".into(),
                main: "main.lua".into(),
                migrations: vec![MigrationSpec {
                    version: 1,
                    name: "init".into(),
                    file: "001_init.sql".into(),
                }],
                icon: None,
                tint: None,
                description: None,
                requires: None,
                ui: None,
                window: None,
            },
            files,
        };
        mem.insert(format!("mem://{id}/main.lua"), main);
        mem.insert(format!("mem://{id}/001_init.sql"), sql);
        let url = format!("mem://{id}/app-ref.json");
        mem.insert(url.clone(), serde_json::to_vec(&r).unwrap());
        url
    }
    pub fn tamper(&self, id: &str) {
        self.fetcher.0.lock().unwrap().insert(format!("mem://{id}/main.lua"), b"evil".to_vec());
    }
}

/// Token -> identity table.
#[derive(Default)]
pub struct StaticDirectory(pub Vec<(String, vos_domain::AgentIdentity)>);
impl AgentDirectory for StaticDirectory {
    fn authenticate(&self, t: &str) -> Option<vos_domain::AgentIdentity> {
        self.0.iter().find(|(k, _)| k == t).map(|(_, v)| v.clone())
    }
}

/// In-memory agent contacts.
#[derive(Default)]
pub struct MemAgentStore {
    agents: Mutex<Vec<(Agent, Option<String>)>>,
}
#[async_trait]
impl AgentStore for MemAgentStore {
    async fn list(&self) -> PortResult<Vec<Agent>> {
        Ok(self.agents.lock().unwrap().iter().map(|(a, _)| a.clone()).collect())
    }
    async fn insert(&self, agent: &Agent, token_hash: Option<&str>) -> PortResult<bool> {
        let mut all = self.agents.lock().unwrap();
        if all.iter().any(|(a, _)| a.id == agent.id) {
            return Ok(false);
        }
        all.push((agent.clone(), token_hash.map(str::to_owned)));
        Ok(true)
    }
    async fn update(&self, agent: &Agent) -> PortResult<bool> {
        let mut all = self.agents.lock().unwrap();
        match all.iter_mut().find(|(a, _)| a.id == agent.id) {
            Some(slot) => {
                slot.0 = agent.clone();
                Ok(true)
            }
            None => Ok(false),
        }
    }
    async fn delete(&self, id: &AgentId) -> PortResult<bool> {
        let mut all = self.agents.lock().unwrap();
        let before = all.len();
        all.retain(|(a, _)| a.id != *id);
        Ok(all.len() < before)
    }
    async fn set_token_hash(&self, id: &AgentId, token_hash: &str) -> PortResult<bool> {
        let mut all = self.agents.lock().unwrap();
        match all.iter_mut().find(|(a, _)| a.id == *id) {
            Some(slot) => {
                slot.1 = Some(token_hash.to_owned());
                Ok(true)
            }
            None => Ok(false),
        }
    }
    async fn token_hashes(&self) -> PortResult<Vec<(AgentId, String)>> {
        Ok(self.agents.lock().unwrap().iter().filter_map(|(a, h)| Some((a.id.clone(), h.clone()?))).collect())
    }
}

/// In-memory transcripts.
#[derive(Default)]
pub struct MemThreadStore(Mutex<HashMap<String, StoredThread>>);
impl MemThreadStore {
    pub fn get(&self, id: &str) -> Option<StoredThread> {
        self.0.lock().unwrap().get(id).cloned()
    }
}
#[async_trait]
impl ThreadStore for MemThreadStore {
    async fn load(&self, thread_id: &str) -> PortResult<Option<StoredThread>> {
        Ok(self.get(thread_id))
    }
    async fn save(&self, thread_id: &str, thread: &StoredThread, _now_ms: u64) -> PortResult<()> {
        self.0.lock().unwrap().insert(thread_id.to_owned(), thread.clone());
        Ok(())
    }
    async fn delete(&self, thread_id: &str) -> PortResult<()> {
        self.0.lock().unwrap().remove(thread_id);
        Ok(())
    }
}
