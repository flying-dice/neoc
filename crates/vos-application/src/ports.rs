//! Driven ports. Each is a trait the application owns; an adapter crate implements it.

use async_trait::async_trait;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc};
use vos_domain::{Actor, Agent, AgentId, AppId, Device, DeviceId, Notification, QualifiedToolName, ToolSpec};

pub use crate::data::{AppData, DataStore, DataTx, ExecOutcome, Params, PortResult, SqlMigration};

// ---- running apps -------------------------------------------------------------------------

/// The files of an app package handed to the runtime: the entry point, by path inside the app ref. An app is
/// built into one self-contained Luau file, so the entry is the only source; there is no filesystem and
/// `require` loads no package file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
// TODO: clean-code - 0.4 - KISS: `files` is always the one entry named by `entry`; a single `source: Vec<u8>` would do.
pub struct AppPackage {
    pub entry: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Who is calling and when: `ctx.actor` and `ctx.now` in Lua.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallContext {
    pub actor: Actor,
    pub now_ms: u64,
}

#[async_trait]
pub trait AppRuntime: Send + Sync {
    /// Compile + run the package's entry, which serves the app's methods on the bus (`RPC.on_request`). Loading an app
    /// that is already loaded replaces it (hot reload): the database handle is the caller's, so data is kept.
    ///
    /// A hot reload stops the old version's background work (listeners, tasks, workers) before the new version
    /// is known to load. If the new version fails to load, the old one keeps serving its handlers
    /// without that background work, and if it starts a server again, requests to that server run read-only
    /// until the next successful reload or a host restart. That fails safe and is the accepted behaviour
    /// (Jira AGOS-64); restarting the previous version in full is the option to revisit if failed reloads
    /// become common.
    async fn load(&self, app: &AppId, package: AppPackage, data: Arc<dyn AppData>) -> PortResult<()>;
    /// Stop the app and free its VM. A call still running in it is stopped too (it does not keep the app
    /// alive), so an uninstalled app never runs on.
    async fn unload(&self, app: &AppId) -> PortResult<()>;
    /// A request on the bus for the app's backend handler of `method`: `handler(params, ctx)`. The person's own client
    /// is calling, so unlike an agent's call of a read-only method the call may write.
    async fn call_handler(
        &self,
        app: &AppId,
        method: &str,
        args: Value,
        ctx: CallContext,
    ) -> PortResult<Value>;
    /// `call_handler` with every write refused (data, `app.kv`, `app.files`, notifications, windows): an agent's call of
    /// a handler the app described as read-only.
    async fn call_handler_read_only(
        &self,
        app: &AppId,
        method: &str,
        args: Value,
        ctx: CallContext,
    ) -> PortResult<Value>;
    /// Whether the app's backend has a handler for `method` on the bus (`RPC.on_request`).
    fn has_handler(&self, app: &AppId, method: &str) -> bool;
    /// The handlers the app's backend has on the bus, as each described itself when it registered.
    fn handlers(&self, app: &AppId) -> Vec<HandlerInfo>;
    /// An app's notification, from its page or its backend: to its open pages and every other app's that listens
    /// (through the shell), and to every backend that listens (`RPC.on_notification`). A `_name` never leaves the app.
    async fn deliver_notification(&self, from: &AppId, name: &str, params: Value) -> PortResult<()>;
}

// ---- the RPC bus ------------------------------------------------------------------------------

/// A handler on the bus, as it described itself when it registered: what an agent is shown of it when it is public (a
/// method without a leading `_` is an agent tool, `<app>__<method in snake case>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandlerInfo {
    pub method: String,
    /// The name a person is shown.
    pub title: Option<String>,
    /// What it does: an agent decides from it when to call it.
    pub description: Option<String>,
    /// The JSON Schema of its params.
    pub input_schema: Option<Value>,
    /// It changes nothing: an agent's call of the backend's handler runs with every write refused.
    pub read_only: bool,
}

/// The one RPC bus: every app request and notification, from a page or a backend, goes through it. A request reaches
/// every handler registered for its method, a backend's and the open pages' alike, and the first answer wins (the
/// others are cancelled); a notification reaches every subscriber, and nothing waits for it.
#[async_trait]
pub trait AppBus: Send + Sync {
    /// `send_request`: `caller` asks `app` for `method`. Method not found when no handler is registered anywhere.
    async fn request(&self, caller: Actor, app: &AppId, method: &str, params: Value) -> PortResult<Value>;
    /// `send_notification`: `from`'s notification, to whoever listens.
    async fn publish(&self, from: &AppId, name: &str, params: Value) -> PortResult<()>;
    /// The handlers of `app` on the bus changed (one registered or went away): its agent tools follow.
    async fn handlers_changed(&self, app: &AppId);
}

/// The handlers the open pages registered on the bus (`RPC.on_request` in a page). The `/rpc` adapter keeps them per
/// connection.
#[async_trait]
pub trait PageHandlers: Send + Sync {
    /// Whether some open page serves `method` of `app`.
    fn handles(&self, app: &AppId, method: &str) -> bool;
    /// The handlers the open pages registered for `app`, as each described itself (one per method).
    fn handlers(&self, app: &AppId) -> Vec<HandlerInfo>;
    /// Ask every page that serves it; the first answer wins and the others are told the request is settled.
    async fn request(&self, app: &AppId, method: &str, params: Value) -> PortResult<Value>;
}

// ---- tools / notifications / clients --------------------------------------------------------

/// The set of tools visible to agents. The MCP adapter implements this and emits `tools/list_changed`.
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn publish(&self, app: &AppId, tools: Vec<ToolSpec>) -> PortResult<()>;
    async fn retract(&self, app: &AppId) -> PortResult<()>;
    /// What one agent may see changed (its grants were edited): tell its sessions the tool list changed.
    async fn visibility_changed(&self, agent: &AgentId) -> PortResult<()>;
}

/// The tools of every running app (the public handlers on the bus), for an app that offers them to an agent of its
/// own (the Agents app gives an agent it talks to the same tools an MCP agent has). It is not checked against any
/// grant: an app is the person's own code, and the agent it speaks for is one the person added to it. The call is
/// attributed to that agent.
#[async_trait]
pub trait AppTools: Send + Sync {
    /// Every tool of every running app, under its qualified name (`<app>__<tool>`), in name order.
    fn all(&self) -> Vec<(QualifiedToolName, ToolSpec)>;
    /// Run a tool as `caller` (the agent the app speaks for), as an MCP `tools/call` does: a read-only tool runs
    /// read-only, and a tool of an app that is not running is an error.
    async fn call(&self, caller: Actor, name: &QualifiedToolName, args: Value) -> PortResult<Value>;
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
    AppEnabled(AppId),
    AppDisabled(AppId),
    /// An app failed to load (install, restore, enable or reload); other apps are unaffected.
    AppFailed {
        app: AppId,
        message: String,
    },
    Notification(Notification),
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, event: ServerEvent);
}

// ---- agents ---------------------------------------------------------------------------------

/// What an app may know of an agent contact: enough to show it, nothing it could act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCard {
    pub id: AgentId,
    pub name: String,
    pub avatar: Option<String>,
    pub role: String,
}

/// The agent roster as apps see it (`app.agents:list()`).
pub trait AgentRoster: Send + Sync {
    fn cards(&self) -> Vec<AgentCard>;
}

/// The agent contacts: who they are and the hash of their persistent MCP token. Only hashes of secrets cross
/// this port.
#[async_trait]
pub trait AgentStore: Send + Sync {
    async fn list(&self) -> PortResult<Vec<Agent>>;
    /// Insert a new contact; `false` when the id is taken.
    async fn insert(&self, agent: &Agent, token_hash: Option<&str>) -> PortResult<bool>;
    /// Replace the record of an existing contact; `false` if there is none.
    async fn update(&self, agent: &Agent) -> PortResult<bool>;
    async fn delete(&self, id: &AgentId) -> PortResult<bool>;
    /// `false` if there is no such agent.
    async fn set_token_hash(&self, id: &AgentId, token_hash: &str) -> PortResult<bool>;
    /// Every (agent, token hash) pair that has a persistent token.
    async fn token_hashes(&self) -> PortResult<Vec<(AgentId, String)>>;
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

// ---- installed apps ---------------------------------------------------------------------------

/// An installed app as the host remembers it across restarts (the system database).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledApp {
    pub id: AppId,
    /// The app-ref URL it was installed from: restart and reload re-fetch (and re-verify) from here.
    pub source_url: String,
    pub version: String,
    pub enabled: bool,
    pub installed_at_ms: u64,
    /// SHA-256 (hex) of the app-ref document that was installed or last loaded. It is what the person
    /// reviewed (install is pinned to it) and then a record of what is loaded: every successful load of a
    /// changed ref (a dev `app.reload`, a restart) replaces it, because a reload legitimately changes files.
    /// It is not compared on reload: the app id is what a load must keep.
    /// Empty on a record from before this was kept: the next load adopts what the manifest asks for.
    pub ref_sha256: String,
}

#[async_trait]
pub trait InstalledAppStore: Send + Sync {
    /// Insert or replace.
    async fn save(&self, app: &InstalledApp) -> PortResult<()>;
    async fn remove(&self, id: &AppId) -> PortResult<()>;
    async fn set_enabled(&self, id: &AppId, enabled: bool) -> PortResult<()>;
    async fn list(&self) -> PortResult<Vec<InstalledApp>>;
}

// ---- shell -------------------------------------------------------------------------------------

/// What an app asks of the desktop shell: `app.open_window` (an agent's tool bringing an app forward) and its
/// notifications (`RPC.send_notification`, for the app's open clients). Both are delivered to the connected clients.
#[async_trait]
pub trait Shell: Send + Sync {
    /// `app.open_window(params)`: the connected shells open the window or bring the app's forward.
    async fn open_window(&self, app: &AppId, window: &str, params: Map<String, Value>);
    /// An app's notification (`RPC.send_notification`, or its page's): the connected shells hand it to the frames of
    /// that app, and to those that subscribed to it.
    async fn emit_event(&self, app: &AppId, name: &str, data: Value);
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
/// The CLI (`vos pair`) and the running host share the same store, so every operation must be
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
    fn authenticate(&self, bearer_token: &str) -> Option<vos_domain::AgentIdentity>;
}
