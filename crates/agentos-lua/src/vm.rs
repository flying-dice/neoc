//! One sandboxed Lua 5.1 VM per app.
//!
//! The VM is a bare `agentos-luavm` engine with the app's own environment table (see
//! `agentos_luavm::lua::app_env`): the prelude's polyfills, the `app` capability table and a `require` that
//! knows only the host modules. Nothing in it is budgeted (no CPU, memory or time limits: the host is a
//! homelab's own and apps are meant to do real work); it then runs the app's entry. After that the host
//! calls into it: tools, write-back callbacks, events. One call runs in a VM at a time (an app is
//! single-threaded by design); the VM lives as long as its `AppVm`.

use crate::{
    api,
    convert::{json_to_lua, lua_to_json},
    error::{ScriptError, ToolFailure},
    state::{DepKey, SharedTracker, Tracker, TrackerScope},
    view::{RenderScope, ViewInstance},
};
use agentos_application::{
    data::DataTx,
    live::{SourceDef, SourceId},
    ports::{AgentCard, AgentRoster, AppData, AppPackage, CallContext, MenuCommand, ViewEffect},
};
use agentos_domain::{time, tool_schema, AppDecl, AppId, Notification, ToolSpec};
use agentos_luavm::mlua;
use agentos_luavm::{
    lua::{
        app_env,
        lib::log::set_log_sink,
        lifecycle::{self, set_entry, Baton, CallEntry, Life},
        sandbox::{install_runtime_state, load_source, make_bare_engine},
    },
    mlua::{Function, Lua, Table, Value as LuaValue},
};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::Notify;

/// The Lua 5.1 polyfills of the Lua 5.3 standard-library members 5.1 lacks (`lua/prelude.lua`, copied by `bun run codegen`): run
/// once in each app's environment before the app.
const PRELUDE: &str = include_str!("../../../types/generated/prelude.lua");

#[derive(Debug, thiserror::Error)]
pub enum VmError {
    /// The entry point (or a module it requires) failed while loading.
    #[error("app `{app}` failed to load: {error}")]
    Load { app: String, error: ScriptError },
    /// App code raised an error.
    #[error("{0}")]
    Script(ScriptError),
    /// The tool called `ctx:fail(message)`.
    #[error("{0}")]
    ToolFailed(String),
    #[error("no such tool `{0}`")]
    NoSuchTool(String),
    #[error("no such source `{0}`, or it has no write callback")]
    NoSuchWriter(String),
    #[error("no such window `{0}`")]
    NoSuchWindow(String),
    #[error("no such view `{0}`")]
    NoSuchView(String),
    /// A window rendered a tree that fails the component schema.
    #[error("the rendered tree is invalid:\n{}", .0.join("\n"))]
    InvalidTree(Vec<String>),
    /// The caller's arguments do not fit the tool's input schema.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("conversion: {0}")]
    Convert(String),
}
pub type Result<T> = std::result::Result<T, VmError>;

/// Why booting a VM failed: Lua refused something, or the package is not loadable.
enum BootError {
    Lua(mlua::Error),
    Vm(VmError),
}
impl From<mlua::Error> for BootError {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}
impl From<VmError> for BootError {
    fn from(e: VmError) -> Self {
        Self::Vm(e)
    }
}

/// How a VM is built.
#[derive(Clone)]
pub struct VmConfig {
    /// Lua 5.1 source of host modules `require("@agentos/<name>")` can load besides `@agentos/app`
    /// (the generated `@agentos/ui` constructors), by module name.
    pub host_modules: Arc<HashMap<String, Vec<u8>>>,
    /// The agent contacts `app.agents:list()` shows (empty unless the host gives a roster).
    pub roster: Arc<dyn AgentRoster>,
}

impl std::fmt::Debug for VmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VmConfig").finish_non_exhaustive()
    }
}

/// No agents: what an app sees when the host has no roster (tests of other things).
struct NoRoster;
impl AgentRoster for NoRoster {
    fn cards(&self) -> Vec<AgentCard> {
        vec![]
    }
}

impl Default for VmConfig {
    fn default() -> Self {
        Self { host_modules: Arc::default(), roster: Arc::new(NoRoster) }
    }
}

/// Something the app asked the host to do, collected during a call and taken afterwards.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Notify(Notification),
    Shell(ShellEffect),
    /// `app:openWindow(name, params)`.
    OpenWindow {
        name: String,
        params: Value,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellEffect {
    Badge(Option<String>),
    Commands(Vec<MenuCommand>),
    DeepLinks(Vec<String>),
}

pub(crate) struct ToolDecl {
    pub spec: ToolSpec,
    pub run: Function,
}

/// Everything the `app` API closures share with the VM's owner.
pub(crate) struct Shared {
    pub app: AppId,
    pub package: AppPackage,
    pub db: Arc<dyn AppData>,
    pub host_modules: Arc<HashMap<String, Vec<u8>>>,
    pub roster: Arc<dyn AgentRoster>,
    pub tracker: SharedTracker,
    pub tools: Mutex<Vec<ToolDecl>>,
    pub handlers: Mutex<HashMap<String, Function>>,
    pub sources: Mutex<Vec<SourceDef>>,
    pub writers: Mutex<HashMap<String, Function>>,
    pub windows: Mutex<HashMap<String, Table>>,
    pub commands: Mutex<Vec<(MenuCommand, Function)>>,
    pub deep_links: Mutex<Vec<(String, Function)>>,
    pub decl: Mutex<Option<AppDecl>>,
    pub effects: Mutex<Vec<Effect>>,
    /// The table the app's chunks (and the host modules it requires) run in as their globals.
    pub env: Mutex<Option<Table>>,
    pub modules: Mutex<HashMap<String, LuaValue>>,
    pub loading: Mutex<HashSet<String>>,
    pub app_table: Mutex<Option<Table>>,
    pub fail: Mutex<Option<Function>>,
    /// Open windows by view id.
    pub views: Mutex<HashMap<String, ViewInstance>>,
    /// The source handles of the render in progress.
    pub render_scope: Mutex<Option<RenderScope>>,
    /// What windows asked for during a call (toasts, `client.set`).
    pub view_effects: Mutex<Vec<(String, ViewEffect)>>,
    /// The open `db:tx(fn)` transaction, if any. A statement takes it out while it runs and puts it back, so
    /// no lock is held across an await and a call that was cut off cannot wedge the next one.
    pub tx: Mutex<Option<Box<dyn DataTx>>>,
    /// The running tool is declared `readOnly`: its database handle refuses writes and `app.kv` /
    /// `db:migrate` raise. Set only while that tool's call holds the VM's gate.
    pub read_only: Arc<AtomicBool>,
    /// Everything the app spawned (tasks, servers, workers): aborted when the app is unloaded.
    pub life: Life,
    /// Woken when a call that nobody made through the host (a request to the app's own server, a task's next
    /// slice) ends, so the runtime forwards the effects it queued (`app.notify`, `app.shell`).
    pub effects_signal: Arc<Notify>,
}

impl Shared {
    /// The database as the running call may use it: a read-only handle inside a `readOnly` tool.
    pub fn database(&self) -> Arc<dyn AppData> {
        if self.read_only.load(Ordering::SeqCst) {
            self.db.read_only()
        } else {
            self.db.clone()
        }
    }

    /// The call may change the world: refused inside a tool declared `readOnly`, which may only read. A
    /// `readOnly` tool that could open a window (whose `onOpen` writes), notify, change the shell or assign
    /// `app.state` would write behind the guarantee that lets agents run it without a prompt.
    pub fn writable(&self, what: &str) -> mlua::Result<()> {
        if self.read_only.load(Ordering::SeqCst) {
            return Err(mlua::Error::runtime(format!(
                "{what} is not allowed in a tool declared readOnly: a readOnly tool cannot write"
            )));
        }
        Ok(())
    }

    /// Queue something for the host to do after the call.
    #[allow(clippy::unnecessary_wraps)] // the same shape as the other effect pushes
    pub fn push_effect(&self, effect: Effect) -> mlua::Result<()> {
        self.effects.lock().push(effect);
        Ok(())
    }

    /// Queue what a window asked of its view. A window context outlives the call
    /// that rendered it (an app can keep `w` in a variable), so this is refused inside a `readOnly` tool like
    /// every other change to the world.
    pub fn push_view_effect(&self, what: &str, view: &str, effect: ViewEffect) -> mlua::Result<()> {
        self.writable(what)?;
        self.view_effects.lock().push((view.to_owned(), effect));
        Ok(())
    }

    /// An `openWindow` target must be a window the app declared: an app cannot ask the shell for anything
    /// else (the request is broadcast to every client).
    pub fn declared_window(&self, name: &str) -> mlua::Result<()> {
        let windows = self.windows.lock();
        if windows.contains_key(name) {
            return Ok(());
        }
        let mut declared: Vec<&str> = windows.keys().map(String::as_str).collect();
        declared.sort_unstable();
        Err(mlua::Error::runtime(format!(
            "openWindow: no window `{name}` is declared (declared: {})",
            if declared.is_empty() { "none".to_owned() } else { declared.join(", ") }
        )))
    }

    pub fn chunk_name(&self, path: &str) -> String {
        format!("={}/{path}", self.app)
    }

    /// Release every Lua value the host holds, before the VM goes away.
    fn release(&self) {
        self.tools.lock().clear();
        self.handlers.lock().clear();
        self.writers.lock().clear();
        self.windows.lock().clear();
        self.commands.lock().clear();
        self.deep_links.lock().clear();
        self.modules.lock().clear();
        self.views.lock().clear();
        *self.env.lock() = None;
        *self.app_table.lock() = None;
        *self.fail.lock() = None;
    }
}

/// Marks the VM as running a `readOnly` tool until dropped (the call's gate is held for that whole time).
struct ReadOnlyCall<'a>(Option<&'a Shared>);

impl<'a> ReadOnlyCall<'a> {
    fn enter(shared: &'a Shared, read_only: bool) -> Self {
        if read_only {
            shared.read_only.store(true, Ordering::SeqCst);
        }
        Self(read_only.then_some(shared))
    }
}

impl Drop for ReadOnlyCall<'_> {
    fn drop(&mut self) {
        if let Some(shared) = self.0 {
            shared.read_only.store(false, Ordering::SeqCst);
        }
    }
}

/// What a call keeps while it waits without using the VM (see [`Baton::park`]).
struct Parked {
    read_only: bool,
    scope: TrackerScope,
}

/// A call in progress: it holds the VM's gate, so one call runs in the VM at a time and `db:tx`, the
/// `readOnly` flag and state tracking are unambiguous. Ending a call, or abandoning it (its caller gave up),
/// rolls back a transaction it left open, so the database's write gate is free the moment the call is over.
///
/// It is also the call's *baton* (see `agentos_luavm::lua::lifecycle`): at a wait that does not use the VM
/// (`task.wait`, the network, a worker) the call sets its own state aside and lets the next call in, then
/// takes the gate and its state back.
pub(crate) struct Turn {
    shared: Arc<Shared>,
    gate: Arc<tokio::sync::Mutex<()>>,
    held: Option<tokio::sync::OwnedMutexGuard<()>>,
    parked: Option<Parked>,
    /// The call came from inside the VM (a request to the app's server, a task), not through the host: its
    /// effects are forwarded by the runtime's pump when it ends.
    inbound: bool,
}

impl Turn {
    async fn take(shared: Arc<Shared>, gate: Arc<tokio::sync::Mutex<()>>, inbound: bool) -> Self {
        let held = gate.clone().lock_owned().await;
        Self { shared, gate, held: Some(held), parked: None, inbound }
    }

    /// A call that holds nothing yet (a coroutine's continuation): it takes the gate at its first wait's end.
    fn detached(shared: Arc<Shared>, gate: Arc<tokio::sync::Mutex<()>>) -> Self {
        Self { shared, gate, held: None, parked: None, inbound: true }
    }
}

impl Baton for Turn {
    fn holding(&self) -> bool {
        self.held.is_some()
    }

    fn park(&mut self) -> std::result::Result<(), String> {
        if self.shared.tx.lock().is_some() {
            return Err(
                "cannot wait inside db:tx: a transaction holds the database for as long as it is open, \
                        so finish it first (a call may wait before it starts one, or after it commits)"
                    .to_owned(),
            );
        }
        let mut tracker = self.shared.tracker.lock();
        if tracker.is_pure() {
            return Err("cannot wait while rendering: render must be pure and quick".to_owned());
        }
        let scope = tracker.suspend();
        drop(tracker);
        let read_only = self.shared.read_only.swap(false, Ordering::SeqCst);
        self.parked = Some(Parked { read_only, scope });
        self.held = None;
        Ok(())
    }

    fn unpark(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if self.held.is_none() {
                self.held = Some(self.gate.clone().lock_owned().await);
            }
            if let Some(parked) = self.parked.take() {
                self.shared.read_only.store(parked.read_only, Ordering::SeqCst);
                self.shared.tracker.lock().resume(parked.scope);
            }
        })
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        if self.held.is_some() {
            // before the gate is released (fields drop after this body): dropping the transaction rolls it back
            drop(self.shared.tx.lock().take());
        }
        if self.inbound {
            self.shared.effects_signal.notify_one();
        }
    }
}

/// The queue of calls into this VM that the library's inbound callbacks join.
struct Entry {
    shared: Arc<Shared>,
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl CallEntry for Entry {
    fn detached(&self) -> Box<dyn Baton> {
        Box::new(Turn::detached(self.shared.clone(), self.gate.clone()))
    }

    fn enter(&self) -> Pin<Box<dyn Future<Output = Box<dyn Baton>> + Send + 'static>> {
        let (shared, gate) = (self.shared.clone(), self.gate.clone());
        Box::pin(async move { Box::new(Turn::take(shared, gate, true).await) as Box<dyn Baton> })
    }
}

pub struct AppVm {
    pub app: AppId,
    lua: Lua,
    shared: Arc<Shared>,
    /// Serialises calls: one runs at a time (see [`Turn`]).
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl Drop for AppVm {
    fn drop(&mut self) {
        // whatever the app started stops with it (a VM that failed to load included)
        self.shared.life.shutdown();
        self.shared.release();
    }
}

fn script(e: &mlua::Error) -> VmError {
    match e.downcast_ref::<ToolFailure>() {
        Some(f) => VmError::ToolFailed(f.0.clone()),
        None => VmError::Script(ScriptError::from_lua(e)),
    }
}

impl AppVm {
    /// Create the VM, install the capability table, then run the package's entry.
    pub async fn load(
        app: &AppId,
        package: AppPackage,
        db: Arc<dyn AppData>,
        config: VmConfig,
    ) -> Result<Self> {
        let VmConfig { host_modules, roster } = config;
        let load_err =
            |e: &mlua::Error| VmError::Load { app: app.to_string(), error: ScriptError::from_lua(e) };
        let lua = make_bare_engine().map_err(|e| load_err(&e))?;
        let life = Life::new();
        // what the `std:`/`lib:`/`vnd:` modules keep per VM (the task scheduler, the registry of what the app
        // spawns), and where `lib:log` writes: the host's tracing, with the app's id
        install_runtime_state(&lua, life.clone());
        let log_app = app.clone();
        set_log_sink(
            &lua,
            Arc::new(move |level, line| match level {
                "error" => tracing::error!(app = %log_app, "{line}"),
                "warn" => tracing::warn!(app = %log_app, "{line}"),
                "info" => tracing::info!(app = %log_app, "{line}"),
                "debug" => tracing::debug!(app = %log_app, "{line}"),
                _ => tracing::trace!(app = %log_app, "{line}"),
            }),
        );
        let read_only = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared {
            app: app.clone(),
            package,
            db,
            host_modules,
            roster,
            tracker: Arc::new(Mutex::new(Tracker::new(read_only.clone()))),
            tools: Mutex::default(),
            handlers: Mutex::default(),
            sources: Mutex::default(),
            writers: Mutex::default(),
            windows: Mutex::default(),
            commands: Mutex::default(),
            deep_links: Mutex::default(),
            decl: Mutex::default(),
            effects: Mutex::default(),
            env: Mutex::new(None),
            modules: Mutex::default(),
            loading: Mutex::default(),
            app_table: Mutex::default(),
            fail: Mutex::default(),
            views: Mutex::default(),
            render_scope: Mutex::default(),
            view_effects: Mutex::default(),
            tx: Mutex::new(None),
            read_only,
            life,
            effects_signal: Arc::new(Notify::new()),
        });
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        // a request to the app's own server or the next slice of one of its tasks takes its turn like any call
        set_entry(&lua, Arc::new(Entry { shared: shared.clone(), gate: gate.clone() }));

        let env = app_env::new_env(&lua).map_err(|e| load_err(&e))?;
        let vm = Self { app: app.clone(), lua, shared, gate };
        vm.in_call(vm.boot(env)).await.map_err(|e| match e {
            BootError::Lua(e) => load_err(&e),
            BootError::Vm(e) => e,
        })?;
        Ok(vm)
    }

    /// Run the prelude, seal the environment, install `app` and `require`, then run the package's entry.
    async fn boot(&self, env: Table) -> std::result::Result<(), BootError> {
        // the polyfills go in before the environment is sealed, and the host's own `app` and `require` after
        load_source(&self.lua, PRELUDE)
            .set_name("=prelude")
            .set_environment(env.clone())
            .call::<()>(env.clone())?;
        app_env::freeze(&self.lua, &env)?;
        *self.shared.env.lock() = Some(env.clone());
        api::install(&self.lua, &self.shared, &env)?;
        let entry = self.shared.package.entry.clone();
        let source = self
            .shared
            .package
            .files
            .get(&entry)
            .cloned()
            .ok_or_else(|| VmError::Convert(format!("the package has no entry `{entry}`")))?;
        load_source(&self.lua, source.as_slice())
            .set_name(self.shared.chunk_name(&entry))
            .set_environment(env)
            .exec_async()
            .await?;
        Ok(())
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }

    pub(crate) fn life(&self) -> &Life {
        &self.shared.life
    }

    /// Woken when a call that came from inside the VM ends (see [`Shared::effects_signal`]).
    pub fn effects_signal(&self) -> Arc<Notify> {
        self.shared.effects_signal.clone()
    }

    /// Run a top-level call: wait for the previous one, then run `body` holding the VM's gate (see [`Turn`]).
    pub(crate) async fn in_call<T>(&self, body: impl Future<Output = T>) -> T {
        let turn = Turn::take(self.shared.clone(), self.gate.clone(), false).await;
        lifecycle::scoped(Box::new(turn), body).await
    }

    // ---- what the app declared ------------------------------------------------------------------------

    pub fn decl(&self) -> Option<AppDecl> {
        self.shared.decl.lock().clone()
    }

    pub fn tools(&self) -> Vec<ToolSpec> {
        self.shared.tools.lock().iter().map(|t| t.spec.clone()).collect()
    }

    /// The live-source definitions (`db:query`, `db:row`, `db:scalar`), in declaration order.
    pub fn sources(&self) -> Vec<SourceDef> {
        self.shared.sources.lock().clone()
    }

    /// Windows declared with `app.window(name, spec)`, by name.
    pub fn window_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.shared.windows.lock().keys().cloned().collect();
        v.sort();
        v
    }

    /// Everything the app asked the host to do since the last call to this.
    pub fn take_effects(&self) -> Vec<Effect> {
        std::mem::take(&mut *self.shared.effects.lock())
    }

    // ---- calls into the app -----------------------------------------------------------------------------

    fn ctx_table(&self, ctx: &CallContext) -> mlua::Result<Table> {
        let t = self.lua.create_table()?;
        let actor = self.lua.create_table()?;
        actor.set("kind", ctx.actor.kind.as_str())?;
        actor.set("id", ctx.actor.id.as_str())?;
        actor.set("name", ctx.actor.name.as_str())?;
        t.set("actor", actor)?;
        t.set("now", time::iso8601_utc(ctx.now_ms))?;
        if let Some(fail) = self.shared.fail.lock().clone() {
            t.set("fail", fail)?;
        }
        Ok(t)
    }

    /// `app.tools.register`'s `run(input, ctx)`. Arguments are validated against the tool's schema first.
    ///
    /// The call is read-only when the tool is declared `readOnly` or when the caller requires it
    /// (`require_read_only`, from the `AppRuntime` port): the caller can only tighten, never loosen.
    pub async fn call_tool(
        &self,
        name: &str,
        args: Value,
        ctx: &CallContext,
        require_read_only: bool,
    ) -> Result<Value> {
        let (schema, run, read_only) = {
            let tools = self.shared.tools.lock();
            let t = tools
                .iter()
                .find(|t| t.spec.name == name)
                .ok_or_else(|| VmError::NoSuchTool(name.to_owned()))?;
            (t.spec.input_schema.clone(), t.run.clone(), t.spec.read_only || require_read_only)
        };
        let input = tool_schema::validate_args(&schema, &args).map_err(VmError::InvalidInput)?;
        self.in_call(async {
            let _read_only = ReadOnlyCall::enter(&self.shared, read_only);
            let convert = |e: mlua::Error| VmError::Convert(e.to_string());
            let arg = json_to_lua(&self.lua, &input).map_err(convert)?;
            let ctx = self.ctx_table(ctx).map_err(convert)?;
            let out: LuaValue = run.call_async((arg, ctx)).await.map_err(|e| script(&e))?;
            lua_to_json(&out).map_err(|e| VmError::Convert(e.to_string()))
        })
        .await
    }

    /// Run a row source's `write(key, field, value, ctx)` callback (a two-way bound field was edited).
    pub async fn write_row(
        &self,
        source: &SourceId,
        key: &Value,
        field: &str,
        value: &Value,
        ctx: &CallContext,
    ) -> Result<()> {
        let write = self
            .shared
            .writers
            .lock()
            .get(&source.0)
            .cloned()
            .ok_or_else(|| VmError::NoSuchWriter(source.0.clone()))?;
        self.in_call(async {
            let convert = |e: mlua::Error| VmError::Convert(e.to_string());
            let args = (
                json_to_lua(&self.lua, key).map_err(convert)?,
                field.to_owned(),
                json_to_lua(&self.lua, value).map_err(convert)?,
                self.ctx_table(ctx).map_err(convert)?,
            );
            write.call_async::<()>(args).await.map_err(|e| script(&e))
        })
        .await
    }

    /// Dispatch an event to `app.on(name, fn)` with a JSON payload; returns the handler's JSON result.
    pub async fn emit(&self, event: &str, payload: Value) -> Result<Value> {
        self.emit_inner(event, payload, false).await.map(|(v, _)| v)
    }

    /// Like [`AppVm::emit`], and report which `app.state` fields the handler read.
    pub async fn emit_tracked(&self, event: &str, payload: Value) -> Result<(Value, BTreeSet<DepKey>)> {
        self.emit_inner(event, payload, true).await
    }

    async fn emit_inner(
        &self,
        event: &str,
        payload: Value,
        track: bool,
    ) -> Result<(Value, BTreeSet<DepKey>)> {
        let handler = self.shared.handlers.lock().get(event).cloned();
        let Some(handler) = handler else { return Ok((Value::Null, BTreeSet::new())) };
        self.in_call(async {
            let arg = json_to_lua(&self.lua, &payload).map_err(|e| VmError::Convert(e.to_string()))?;
            if track {
                self.shared.tracker.lock().begin(false);
            }
            let result = handler.call_async::<LuaValue>(arg).await;
            let deps = if track { self.shared.tracker.lock().end() } else { BTreeSet::new() };
            let out = result.map_err(|e| script(&e))?;
            Ok((lua_to_json(&out).map_err(|e| VmError::Convert(e.to_string()))?, deps))
        })
        .await
    }

    /// The `app.state` fields assigned since the last call (views that read them are dirty).
    pub fn take_dirty_state(&self) -> BTreeSet<DepKey> {
        self.shared.tracker.lock().take_dirty()
    }

    // ---- accounting -------------------------------------------------------------------------------------

    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }

    pub fn gc(&self) {
        let _ = self.lua.gc_collect();
    }

    /// Stop what the app started (tasks, servers and their connections, workers; their sockets are closed)
    /// but leave the VM answering: a reload does this to the old version before the new one starts.
    pub fn stop_background(&self) {
        self.shared.life.stop_background();
    }

    /// The app is going away: everything it started (tasks, servers and their connections, workers) is aborted,
    /// and a host call a running call is waiting on is abandoned. Call this before dropping the last `Arc`, so
    /// an app cannot keep a listener or a task alive after an uninstall or a reload.
    pub fn shutdown(&self) {
        self.shared.life.shutdown();
    }

    /// Unload: release every Rust-held reference to Lua values, then the VM. Returns the heap size freed.
    pub fn unload(self) -> usize {
        let used = self.lua.used_memory();
        self.shared.life.shutdown();
        self.shared.release();
        used
    }
}
