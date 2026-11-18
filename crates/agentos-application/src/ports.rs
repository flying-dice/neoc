//! Driven ports. Each is a trait the application owns; an adapter crate implements it.

use crate::PortError;
use agentos_domain::{
    ui::{UiEvent, UiNode},
    AgentId, AppId, Device, DeviceId, Notification, ToolSpec,
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub type PortResult<T> = Result<T, PortError>;

// ---- per-app data -------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlMigration {
    pub version: u32,
    pub name: String,
    pub sql: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOutcome {
    pub changes: u64,
    pub last_insert_rowid: i64,
}

/// A single app's database (what `app.data.*` in Luau is backed by).
#[async_trait]
pub trait AppData: Send + Sync {
    async fn query(&self, sql: String, params: Vec<Value>) -> PortResult<Vec<Value>>;
    async fn exec(&self, sql: String, params: Vec<Value>) -> PortResult<ExecOutcome>;
}

/// Creates / destroys per-app databases (applies the app's declared migrations on open).
#[async_trait]
pub trait DataStore: Send + Sync {
    async fn open(&self, app: &AppId, migrations: Vec<SqlMigration>) -> PortResult<Arc<dyn AppData>>;
    /// Delete the app's data (uninstall with purge).
    async fn purge(&self, app: &AppId) -> PortResult<()>;
}

// ---- running apps -------------------------------------------------------------------------

/// What the runtime learned from running an app's main chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedApp {
    pub tools: Vec<ToolSpec>,
}

#[async_trait]
pub trait AppRuntime: Send + Sync {
    /// Compile + run `main` with only the capabilities the host hands over.
    async fn load(&self, app: &AppId, main: Vec<u8>, data: Arc<dyn AppData>) -> PortResult<LoadedApp>;
    async fn unload(&self, app: &AppId) -> PortResult<()>;
    async fn call_tool(&self, app: &AppId, tool: &str, args: Value) -> PortResult<Value>;
    /// Deliver a UI event; returns the new view tree if the app produced one.
    async fn dispatch_event(&self, app: &AppId, event: UiEvent) -> PortResult<Option<UiNode>>;
}

// ---- tools / notifications / clients --------------------------------------------------------

/// The set of tools visible to agents. The MCP adapter implements this and emits `tools/list_changed`.
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn publish(&self, app: &AppId, tools: Vec<ToolSpec>) -> PortResult<()>;
    async fn retract(&self, app: &AppId) -> PortResult<()>;
}

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, n: Notification) -> PortResult<()>;
}

/// Everything pushed to connected clients (the `/rpc` socket adapter forwards these as notifications).
#[derive(Clone, Debug, PartialEq)]
pub enum ServerEvent {
    AppInstalled(AppId),
    AppUninstalled(AppId),
    UiTree { app: AppId, root: UiNode },
    Notification(Notification),
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, event: ServerEvent);
}

// ---- agents ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunId(pub String);

#[async_trait]
pub trait AgentConnector: Send + Sync {
    /// Launch (or reuse) an agent process and send a prompt; ACP lives behind this.
    async fn start_run(&self, agent: &AgentId, prompt: &str) -> PortResult<RunId>;
    /// Stop every agent process this connector started (host shutdown). Idempotent.
    async fn stop_all(&self);
}

// ---- infrastructure ---------------------------------------------------------------------------

pub trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> u64;
}

/// Fetches bytes from file://, http:// or https:// URLs.
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> PortResult<Vec<u8>>;
}

// ---- paired devices ---------------------------------------------------------------------------

/// Cryptographically secure random bytes (pairing codes, session tokens, ids). A port so the core stays
/// runtime-free and tests can be deterministic.
pub trait RandomSource: Send + Sync {
    fn fill(&self, buf: &mut [u8]);
}

/// Storage for pairing codes and device sessions (the host's system database). Only hashes of secrets
/// cross this port: a leaked database cannot be replayed as a session or a pairing code.
///
/// The CLI (`agentos-host pair`) and the running host share the same store, so every operation must be
/// safe across processes.
#[async_trait]
pub trait DeviceRegistry: Send + Sync {
    /// Remember a pairing code (by hash) until `expires_at_ms`.
    async fn add_pairing_code(&self, code_hash: &str, expires_at_ms: u64) -> PortResult<()>;
    /// Atomically consume a pairing code: `true` once, if it exists and has not expired.
    async fn take_pairing_code(&self, code_hash: &str, now_ms: u64) -> PortResult<bool>;
    /// Forget every pairing code (too many wrong guesses burn them).
    async fn clear_pairing_codes(&self) -> PortResult<()>;
    async fn add_session(&self, device: &Device, token_hash: &str) -> PortResult<()>;
    async fn find_session(&self, token_hash: &str) -> PortResult<Option<Device>>;
    async fn touch_session(&self, id: &DeviceId, now_ms: u64) -> PortResult<()>;
    async fn list_devices(&self) -> PortResult<Vec<Device>>;
    /// Delete a session; `false` if there was none.
    async fn revoke_device(&self, id: &DeviceId) -> PortResult<bool>;
}

// ---- identity ---------------------------------------------------------------------------------

/// Resolves a bearer token to a per-agent identity (attribution + permissions). Constant-time compare
/// is the adapter's job.
pub trait AgentDirectory: Send + Sync {
    fn authenticate(&self, bearer_token: &str) -> Option<agentos_domain::AgentIdentity>;
}
