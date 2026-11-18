//! Driven ports. Each is a trait the application owns; an adapter crate implements it.

use crate::live::{SourceDef, SourceId};
use agentos_domain::{
    ui::{protocol::Command, ClientDecl, Node, SourceDecl},
    Actor, AgentId, AppDecl, AppId, Capabilities, Device, DeviceId, Notification, ToolSpec,
};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

pub use crate::data::{
    AppData, ChangeBatch, ChangeFeed, DataStore, DataTx, ExecOutcome, Params, PortResult, QueryInfo,
    SqlMigration,
};

// ---- running apps -------------------------------------------------------------------------

/// The files of an app package handed to the runtime: the entry point and every `.luau` module, by path
/// inside the app ref. There is no filesystem: `require` resolves only within this set.
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
    /// Compile + run the package's entry with only the capabilities the host hands over. Loading an app
    /// that is already loaded replaces it (hot reload): the database handle is the caller's, so data is kept.
    async fn load(
        &self,
        app: &AppId,
        package: AppPackage,
        data: Arc<dyn AppData>,
        capabilities: Capabilities,
    ) -> PortResult<LoadedApp>;
    async fn unload(&self, app: &AppId) -> PortResult<()>;
    async fn call_tool(&self, app: &AppId, tool: &str, args: Value, ctx: CallContext) -> PortResult<Value>;
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
