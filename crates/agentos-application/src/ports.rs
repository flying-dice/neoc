//! Driven ports. Each is a trait the application owns; an adapter crate implements it.

use crate::live::{SourceDef, SourceId};
use agentos_domain::{
    thread::{
        AgentEvent, ConfigOption, ConfigValue, PermissionAnswer, PermissionRequest, SessionState, StopReason,
        ThreadMessage,
    },
    ui::{protocol::Command, ClientDecl, Node, SourceDecl},
    Actor, Agent, AgentId, AppDecl, AppId, Device, DeviceId, Notification, ToolSpec,
};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

pub use crate::data::{
    AppData, ChangeBatch, ChangeFeed, DataStore, DataTx, ExecOutcome, Params, PortResult, QueryInfo,
    SqlMigration,
};

// ---- running apps -------------------------------------------------------------------------

/// The files of an app package handed to the runtime: the entry point, by path inside the app ref. An app is
/// built into one self-contained Lua 5.1 file, so the entry is the only source; there is no filesystem and
/// `require` loads no package file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppPackage {
    pub entry: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Who is calling and when: `ctx.actor` and `ctx.now` in Luau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallContext {
    pub actor: Actor,
    pub now_ms: u64,
}

/// What the runtime learned from running an app's entry point.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedApp {
    /// The app's `app.manifest { ... }`, if it declared one.
    pub decl: Option<AppDecl>,
    pub tools: Vec<ToolSpec>,
    /// Live-source definitions (`db:query`, `db:row`, `db:scalar`), already validated.
    pub sources: Vec<SourceDef>,
}

/// A source a render used: which declaration it is an instance of, and what the client is told about it.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceBinding {
    pub def: SourceId,
    pub decl: SourceDecl,
}

/// What rendering a window produced.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOutput {
    /// The validated wire tree (handlers are `true` or client actions; the closures stay in the VM).
    pub tree: Node,
    pub client_decls: BTreeMap<String, ClientDecl>,
    /// By source id as it appears in the tree.
    pub sources: BTreeMap<String, SourceBinding>,
    /// Client-managed values that differ from their declared `init`: restored from the host's copy or the
    /// client's, seeded by `onOpen` or by deep-link params. Only filled by `open_view`.
    pub client: Map<String, Value>,
    /// Orders renders of one view: a result older than one already applied is dropped.
    pub seq: u64,
}

/// A user event on a node.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewEventCall {
    pub path: String,
    pub event: String,
    pub row_key: Option<Value>,
    /// The host's last-sent snapshot of that row (`e.row`).
    pub row: Option<Value>,
    pub payload: Option<Value>,
}

/// What a window's code asked of its view during a call.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewEffect {
    /// `w:toast`, `w:focus`, `w:scrollTo`, `w:announce`, `w:copy`, `w:close`.
    Command(Command),
    /// `w.client:set(name, value)`.
    ClientSet { path: String, value: Value, force: bool },
}

#[async_trait]
pub trait AppRuntime: Send + Sync {
    /// Compile + run the package's entry. Loading an app
    /// that is already loaded replaces it (hot reload): the database handle is the caller's, so data is kept.
    async fn load(&self, app: &AppId, package: AppPackage, data: Arc<dyn AppData>) -> PortResult<LoadedApp>;
    /// Stop the app and free its VM. A call still running in it is stopped too (it does not keep the app
    /// alive), so an uninstalled app never runs on.
    async fn unload(&self, app: &AppId) -> PortResult<()>;
    /// Run one of the app's tools. `read_only` is the caller's requirement: the call runs with every write
    /// refused (data, `app.kv`, `app.state`, notifications, windows, shell) even if the tool itself is not
    /// declared `readOnly`. The use case sets it for a tool the app declared `readOnly`, so the guarantee that
    /// lets an agent run such a tool without a prompt holds whatever the adapter does with its own bookkeeping;
    /// an adapter may only tighten it (a tool it knows is `readOnly` runs read-only regardless).
    async fn call_tool(
        &self,
        app: &AppId,
        tool: &str,
        args: Value,
        ctx: CallContext,
        read_only: bool,
    ) -> PortResult<Value>;
    /// Run a row source's `write(key, field, value, ctx)` callback.
    async fn write_row(
        &self,
        app: &AppId,
        source: &SourceId,
        key: Value,
        field: &str,
        value: Value,
        ctx: CallContext,
    ) -> PortResult<()>;

    // ---- windows: one view instance per open window ----
    /// The window kinds the app declared.
    async fn windows(&self, app: &AppId) -> PortResult<Vec<String>>;
    /// Create the instance, run `onOpen`, render. `restored` holds stored client values by name.
    async fn open_view(
        &self,
        app: &AppId,
        view: &str,
        window: &str,
        params: Map<String, Value>,
        restored: Map<String, Value>,
    ) -> PortResult<RenderOutput>;
    /// Render again (state or data it read changed). A render that fails keeps the last good tree.
    async fn render_view(&self, app: &AppId, view: &str) -> PortResult<RenderOutput>;
    /// Run the handlers of `path#event`. `false`: no such handler any more (stale).
    async fn dispatch_view_event(
        &self,
        app: &AppId,
        view: &str,
        call: ViewEventCall,
        ctx: CallContext,
    ) -> PortResult<bool>;
    /// The client mirrored synced or persisted entries (`w.client:get`).
    async fn set_client_values(&self, app: &AppId, view: &str, values: Map<String, Value>) -> PortResult<()>;
    async fn close_view(&self, app: &AppId, view: &str) -> PortResult<()>;
    /// Views whose last render read state that has since changed.
    async fn dirty_views(&self, app: &AppId) -> PortResult<Vec<String>>;
    /// What views asked for since the last call (toasts, `client.set`), by view.
    async fn take_view_effects(&self, app: &AppId) -> PortResult<Vec<(String, ViewEffect)>>;
}

/// Told when an app changed data or went away, so open views can follow. The view service implements it.
#[async_trait]
pub trait AppObserver: Send + Sync {
    /// A tool call (or anything else outside a view event) may have changed state or data.
    async fn mutated(&self, app: &AppId);
    /// The app is about to stop (disable, delete data, reload, uninstall, shutdown).
    async fn stopping(&self, app: &AppId, reason: &str);
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

/// Where an agent finds the tools: the host's `/mcp` and the bearer token that identifies this agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpAccess {
    pub url: String,
    pub token: String,
}

/// What a session reported when it opened.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionOpened {
    pub session_id: String,
    /// Modes, settings and the agent's identity as `initialize` and `session/new` answered them.
    pub session: SessionState,
}

/// How the person's policy answers a permission request without asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Answer on the agent's behalf (an option id), no prompt.
    Auto(String),
    /// Ask the person: the request becomes an interrupt.
    Ask,
}

/// Receives what one agent session reports. Called from the connector's tasks, so it must not block.
pub trait AgentEventSink: Send + Sync {
    fn on_event(&self, event: AgentEvent);
    /// A permission request arrived: decide now whether the person is asked.
    fn on_permission(&self, request: &PermissionRequest) -> PermissionDecision;
    /// The agent process ended (the session is gone).
    fn on_exit(&self, reason: String);
}

/// Long-lived agent processes and their sessions (ACP lives behind this).
#[async_trait]
pub trait AgentConnector: Send + Sync {
    /// Start the agent process if it is not running, initialize it, and open a session. `mcp` (when the
    /// agent supports HTTP MCP) is how the session reaches the host's tools.
    async fn open_session(
        &self,
        agent: &Agent,
        mcp: Option<McpAccess>,
        sink: Arc<dyn AgentEventSink>,
    ) -> PortResult<SessionOpened>;
    /// Send a prompt and wait for the turn to end. Everything the agent reports meanwhile goes to the sink.
    async fn prompt(&self, agent: &AgentId, session_id: &str, text: &str) -> PortResult<StopReason>;
    /// Ask the agent to stop the running turn (`session/cancel`).
    async fn cancel(&self, agent: &AgentId, session_id: &str) -> PortResult<()>;
    /// The person's answer to a permission request that [`PermissionDecision::Ask`] left open.
    async fn answer_permission(&self, request_id: &str, answer: PermissionAnswer);
    async fn set_mode(&self, agent: &AgentId, session_id: &str, mode: &str) -> PortResult<()>;
    /// Change a setting; answers every option's new state.
    async fn set_config(
        &self,
        agent: &AgentId,
        session_id: &str,
        id: &str,
        value: ConfigValue,
    ) -> PortResult<Vec<ConfigOption>>;
    /// Forget a session (its thread was deleted); the process keeps running for other sessions.
    async fn close_session(&self, agent: &AgentId, session_id: &str);
    /// Stop every agent process this connector started (host shutdown). Idempotent.
    async fn stop_all(&self);
}

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

/// What the host keeps of a chat thread between runs and restarts.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredThread {
    pub agent_id: AgentId,
    pub messages: Vec<ThreadMessage>,
}

#[async_trait]
pub trait ThreadStore: Send + Sync {
    async fn load(&self, thread_id: &str) -> PortResult<Option<StoredThread>>;
    /// Insert or replace the whole transcript.
    async fn save(&self, thread_id: &str, thread: &StoredThread, now_ms: u64) -> PortResult<()>;
    async fn delete(&self, thread_id: &str) -> PortResult<()>;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuCommand {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
}

/// What an app asks of the desktop shell: `app:openWindow` (an agent's tool bringing an app forward) is
/// delivered to the clients; the Dock badge, menu commands and deep links of `app.shell` are accepted and
/// dropped until the shell shows them (the API shape is fixed so apps can be written against it).
#[async_trait]
pub trait Shell: Send + Sync {
    /// `app:openWindow(name, params)`: the connected shells open the window or bring the app's forward.
    async fn open_window(&self, app: &AppId, window: &str, params: Map<String, Value>);
    async fn set_badge(&self, app: &AppId, badge: Option<String>);
    async fn set_menu_commands(&self, app: &AppId, commands: Vec<MenuCommand>);
    async fn set_deep_links(&self, app: &AppId, patterns: Vec<String>);
}

// ---- background tasks ---------------------------------------------------------------------------

/// A running background task; dropping the handle stops it (release on dispose).
pub trait TaskHandle: Send {}

/// Spawns long-running work (the live-source pump) without the core knowing the async runtime.
pub trait Background: Send + Sync {
    fn spawn(
        &self,
        name: &'static str,
        task: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Box<dyn TaskHandle>;
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

/// The processes the host runs for agents. When an agent is deleted, or the way it is launched changes, the
/// process it runs in must not outlive that: a deleted agent keeps no process, and an edited command or
/// environment is never ignored because an old process is still serving.
#[async_trait]
pub trait AgentProcesses: Send + Sync {
    /// Stop this agent's process and tell its sessions it ended. A no-op when none is running; the next
    /// session starts a new one from the agent's current settings.
    async fn stop_agent(&self, agent: &AgentId);
}

/// For a host whose connector launches no processes (a scripted one in tests): nothing to stop.
pub struct NoAgentProcesses;

#[async_trait]
impl AgentProcesses for NoAgentProcesses {
    async fn stop_agent(&self, _agent: &AgentId) {}
}
