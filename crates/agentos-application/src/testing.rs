//! In-memory fakes of every port (no frameworks, no mocks library).

use crate::{
    ports::{
        AgentConnector, AgentDirectory, AppData, AppRuntime, Clock, DataStore, DeviceRegistry, EventSink,
        ExecOutcome, Fetcher, LoadedApp, PortResult, RandomSource, RunId, ServerEvent, SqlMigration,
        ToolRegistry,
    },
    services::AppService,
};
use agentos_domain::{
    ui::{UiEvent, UiNode},
    AgentId, AppId, AppRef, Device, DeviceId, FileRef, Manifest, MigrationSpec, ToolSpec,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
pub struct MemFetcher(pub Mutex<HashMap<String, Vec<u8>>>);
#[async_trait]
impl Fetcher for MemFetcher {
    async fn fetch(&self, url: &str) -> PortResult<Vec<u8>> {
        self.0.lock().unwrap().get(url).cloned().ok_or_else(|| crate::PortError::NotFound(url.into()))
    }
}

struct NullData;
#[async_trait]
impl AppData for NullData {
    async fn query(&self, _: String, _: Vec<Value>) -> PortResult<Vec<Value>> {
        Ok(vec![])
    }
    async fn exec(&self, _: String, _: Vec<Value>) -> PortResult<ExecOutcome> {
        Ok(ExecOutcome { changes: 0, last_insert_rowid: 0 })
    }
}

#[derive(Default)]
pub struct MemStore {
    opened: Mutex<HashMap<String, usize>>,
    purged: Mutex<Vec<String>>,
}
impl MemStore {
    pub fn opened_with_migrations(&self, app: &str) -> Option<usize> {
        self.opened.lock().unwrap().get(app).copied()
    }
    pub fn purged(&self, app: &str) -> bool {
        self.purged.lock().unwrap().iter().any(|a| a == app)
    }
}
#[async_trait]
impl DataStore for MemStore {
    async fn open(&self, app: &AppId, m: Vec<SqlMigration>) -> PortResult<Arc<dyn AppData>> {
        self.opened.lock().unwrap().insert(app.to_string(), m.len());
        Ok(Arc::new(NullData))
    }
    async fn purge(&self, app: &AppId) -> PortResult<()> {
        self.purged.lock().unwrap().push(app.to_string());
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeRuntime {
    apps: Mutex<HashMap<String, Vec<ToolSpec>>>,
    calls: Mutex<usize>,
}
impl FakeRuntime {
    pub fn loaded(&self) -> Vec<String> {
        self.apps.lock().unwrap().keys().cloned().collect()
    }
    pub fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}
#[async_trait]
impl AppRuntime for FakeRuntime {
    async fn load(&self, app: &AppId, main: Vec<u8>, _: Arc<dyn AppData>) -> PortResult<LoadedApp> {
        // the fake "app source" is a comma separated list of tool names
        let tools: Vec<ToolSpec> = String::from_utf8_lossy(&main)
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|n| ToolSpec {
                name: n.into(),
                description: String::new(),
                input_schema: json!({"type":"object"}),
            })
            .collect();
        self.apps.lock().unwrap().insert(app.to_string(), tools.clone());
        Ok(LoadedApp { tools })
    }
    async fn unload(&self, app: &AppId) -> PortResult<()> {
        self.apps.lock().unwrap().remove(app.as_str());
        Ok(())
    }
    async fn call_tool(&self, _: &AppId, _: &str, args: Value) -> PortResult<Value> {
        *self.calls.lock().unwrap() += 1;
        Ok(json!({ "echo": args }))
    }
    async fn dispatch_event(&self, _: &AppId, e: UiEvent) -> PortResult<Option<UiNode>> {
        Ok(Some(UiNode::Text { text: e.name }))
    }
}

#[derive(Default)]
pub struct MemRegistry(Mutex<HashMap<String, Vec<String>>>);
impl MemRegistry {
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

#[derive(Default)]
pub struct FakeAgents {
    stopped: AtomicU64,
}
impl FakeAgents {
    /// How many times `stop_all` was called.
    pub fn stop_calls(&self) -> u64 {
        self.stopped.load(Ordering::SeqCst)
    }
}
#[async_trait]
impl AgentConnector for FakeAgents {
    async fn start_run(&self, a: &AgentId, _: &str) -> PortResult<RunId> {
        Ok(RunId(format!("run-{}", a.0)))
    }
    async fn stop_all(&self) {
        self.stopped.fetch_add(1, Ordering::SeqCst);
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

/// Bundle of all fakes + helpers to build the service under test.
pub struct Fakes {
    pub fetcher: Arc<MemFetcher>,
    pub store: Arc<MemStore>,
    pub runtime: Arc<FakeRuntime>,
    pub registry: Arc<MemRegistry>,
    pub events: Arc<RecordingSink>,
    pub agents: Arc<FakeAgents>,
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
            runtime: Arc::default(),
            registry: Arc::default(),
            events: Arc::default(),
            agents: Arc::default(),
        }
    }
    pub fn service(&self) -> AppService {
        AppService::new(
            self.fetcher.clone(),
            self.store.clone(),
            self.runtime.clone(),
            self.registry.clone(),
            self.events.clone(),
            self.agents.clone(),
        )
    }
    /// Publish an app ref at `mem://<id>/app-ref.json`; `main` content = comma-joined tool names.
    pub fn add_app(&self, id: &str, tools: &[(&str, &str)]) -> String {
        let main = tools.iter().map(|t| t.0).collect::<Vec<_>>().join(",").into_bytes();
        let sql = b"CREATE TABLE t(a);".to_vec();
        let sha = |b: &[u8]| format!("{:x}", Sha256::digest(b));
        let r = AppRef {
            manifest: Manifest {
                id: AppId::parse(id).unwrap(),
                name: id.into(),
                version: "1.0.0".into(),
                main: "main.luau".into(),
                permissions: vec![],
                migrations: vec![MigrationSpec {
                    version: 1,
                    name: "init".into(),
                    file: "001_init.sql".into(),
                }],
            },
            files: vec![
                FileRef {
                    path: "main.luau".into(),
                    url: format!("mem://{id}/main.luau"),
                    sha256: sha(&main),
                },
                FileRef {
                    path: "001_init.sql".into(),
                    url: format!("mem://{id}/001_init.sql"),
                    sha256: sha(&sql),
                },
            ],
        };
        let mut m = self.fetcher.0.lock().unwrap();
        m.insert(format!("mem://{id}/main.luau"), main);
        m.insert(format!("mem://{id}/001_init.sql"), sql);
        let url = format!("mem://{id}/app-ref.json");
        m.insert(url.clone(), serde_json::to_vec(&r).unwrap());
        url
    }
    pub fn tamper(&self, id: &str) {
        self.fetcher.0.lock().unwrap().insert(format!("mem://{id}/main.luau"), b"evil".to_vec());
    }
}

/// Token -> identity table.
#[derive(Default)]
pub struct StaticDirectory(pub Vec<(String, agentos_domain::AgentIdentity)>);
impl AgentDirectory for StaticDirectory {
    fn authenticate(&self, t: &str) -> Option<agentos_domain::AgentIdentity> {
        self.0.iter().find(|(k, _)| k == t).map(|(_, v)| v.clone())
    }
}
