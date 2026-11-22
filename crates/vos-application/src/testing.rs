//! In-memory fakes of every port (no frameworks, no mocks library).

use crate::{
    files::{AppFiles, FileStore},
    ports::{
        AgentDirectory, AgentStore, AppData, AppPackage, AppRuntime, CallContext, Clock, DataStore, DataTx,
        DeviceRegistry, EventSink, ExecOutcome, Fetcher, InstalledApp, InstalledAppStore, LoadedApp, Params,
        PortResult, RandomSource, ServerEvent, Shell, SqlMigration, ToolRegistry,
    },
    services::{AppService, Ports},
    PortError,
};
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use vos_domain::{
    Agent, AgentId, AppId, Device, DeviceId, FileInfo, FileKind, FilePath, FileRef, FileResult, Location,
    ToolSpec,
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
struct Inner {
    migrations: Mutex<Vec<SqlMigration>>,
    execs: Mutex<Vec<(String, Params)>>,
}

/// A recording database: it remembers the migrations and statements it is given, and every query returns no
/// rows.
#[derive(Default, Clone)]
pub struct ScriptedData(Arc<Inner>);

impl ScriptedData {
    pub fn migrations(&self) -> Vec<SqlMigration> {
        self.0.migrations.lock().unwrap().clone()
    }
    pub fn execs(&self) -> Vec<(String, Params)> {
        self.0.execs.lock().unwrap().clone()
    }
}

struct ScriptedTx(ScriptedData);
#[async_trait]
impl DataTx for ScriptedTx {
    async fn query(&mut self, _: String, _: Params) -> PortResult<Vec<Value>> {
        Ok(vec![])
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

#[async_trait]
impl AppData for ScriptedData {
    async fn migrate(&self, migrations: Vec<SqlMigration>) -> PortResult<()> {
        self.0.migrations.lock().unwrap().extend(migrations);
        Ok(())
    }
    async fn query(&self, _: String, _: Params) -> PortResult<Vec<Value>> {
        Ok(vec![])
    }
    async fn exec(&self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        let mut execs = self.0.execs.lock().unwrap();
        execs.push((sql, params));
        Ok(ExecOutcome { changes: 1, last_insert_id: i64::try_from(execs.len()).unwrap_or(0) })
    }
    async fn begin(&self) -> PortResult<Box<dyn DataTx>> {
        Ok(Box::new(ScriptedTx(self.clone())))
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        Arc::new(ReadOnlyData(self.clone()))
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
    async fn begin(&self) -> PortResult<Box<dyn DataTx>> {
        Ok(Box::new(ReadOnlyTx(ScriptedTx(self.0.clone()))))
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        Arc::new(ReadOnlyData(self.0.clone()))
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
    fail_load: Mutex<HashSet<String>>,
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
    /// The next loads of this app fail (until `fail_load(app, false)`).
    pub fn fail_load(&self, app: &str, fail: bool) {
        let mut set = self.fail_load.lock().unwrap();
        if fail {
            set.insert(app.to_owned());
        } else {
            set.remove(app);
        }
    }
    /// `(app, handler, args)` of every client `rpc` call so far.
    pub fn handler_calls(&self) -> Vec<(String, String, Value)> {
        self.handler_calls.lock().unwrap().clone()
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
        Ok(LoadedApp { tools })
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
    async fn emit_event(&self, app: &AppId, name: &str, data: Value) {
        self.events.lock().unwrap().push((app.to_string(), name.to_owned(), data));
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

/// The page of an app published by [`Fakes::add_app`].
pub const DEFAULT_PAGE: &str = "<!doctype html><title>app</title>";

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
            registries,
        })
    }
    /// Publish a registry file at `url` listing app-ref URLs (relative ones resolve against `url`).
    pub fn add_registry(&self, url: &str, apps: &[&str]) {
        let body = serde_json::to_vec(&json!({ "apps": apps })).unwrap();
        self.fetcher.0.lock().unwrap().insert(url.to_owned(), body);
    }
    /// Publish an app ref at `mem://<id>/app-ref.json`; `main` content = comma-joined tool names. Its page is
    /// `static/index.html` ([`DEFAULT_PAGE`]).
    pub fn add_app(&self, id: &str, tools: &[(&str, &str)]) -> String {
        self.add_app_with(id, tools, &[])
    }
    /// Like `add_app`, with `page` as the source of `static/index.html` and a window in the manifest.
    pub fn add_client_app(&self, id: &str, tools: &[(&str, &str)], page: &str) -> String {
        let url = self.add_app_with(id, tools, &[("static/index.html", page)]);
        self.edit_ref(&url, |app_ref| {
            app_ref["manifest"]["window"] = json!({ "w": 420, "h": 480, "minW": 320, "minH": 320 });
        });
        url
    }
    /// Like `add_app`, but the manifest has no `ui`: an app ref of the kind this host no longer runs (it drew its
    /// windows as a view tree).
    pub fn add_app_without_page(&self, id: &str) -> String {
        let url = self.add_app(id, &[]);
        self.edit_ref(&url, |app_ref| {
            app_ref["manifest"].as_object_mut().unwrap().remove("ui");
        });
        url
    }
    /// Rewrite the published app ref at `url` as JSON.
    pub fn edit_ref(&self, url: &str, edit: impl FnOnce(&mut Value)) {
        let mut mem = self.fetcher.0.lock().unwrap();
        let mut app_ref: Value = serde_json::from_slice(&mem[url]).unwrap();
        edit(&mut app_ref);
        mem.insert(url.to_owned(), serde_json::to_vec(&app_ref).unwrap());
    }
    /// Like `add_app`, with extra files (path, source): `.lua` modules, or the page's own (`static/...`). Without a
    /// `static/index.html` among them the page is [`DEFAULT_PAGE`].
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
        let mut modules = modules.to_vec();
        if !modules.iter().any(|(path, _)| *path == "static/index.html") {
            modules.push(("static/index.html", DEFAULT_PAGE));
        }
        let mut mem = self.fetcher.0.lock().unwrap();
        for (path, src) in modules {
            files.push(FileRef {
                path: path.into(),
                url: format!("mem://{id}/{path}"),
                sha256: sha(src.as_bytes()),
            });
            mem.insert(format!("mem://{id}/{path}"), src.as_bytes().to_vec());
        }
        let app_ref = json!({
            "manifest": {
                "id": id,
                "name": id,
                "version": "1.0.0",
                "main": "main.lua",
                "migrations": [{ "version": 1, "name": "init", "file": "001_init.sql" }],
                "ui": { "root": "static" },
            },
            "files": files,
        });
        mem.insert(format!("mem://{id}/main.lua"), main);
        mem.insert(format!("mem://{id}/001_init.sql"), sql);
        let url = format!("mem://{id}/app-ref.json");
        mem.insert(url.clone(), serde_json::to_vec(&app_ref).unwrap());
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
