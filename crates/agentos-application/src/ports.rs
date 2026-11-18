//! Driven ports. Each is a trait the application owns; an adapter crate implements it.

use crate::PortError;
use agentos_domain::{
    ui::{UiEvent, UiNode},
    AgentId, AppId, Notification, ToolSpec,
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

// ---- identity ---------------------------------------------------------------------------------

/// Resolves a bearer token to a per-agent identity (attribution + permissions). Constant-time compare
/// is the adapter's job.
pub trait AgentDirectory: Send + Sync {
    fn authenticate(&self, bearer_token: &str) -> Option<agentos_domain::AgentIdentity>;
}
