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
    state::{DepKey, SharedTracker, TrackGuard, Tracker, TrackerScope},
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
        lifecycle::{self, set_entry, Baton, CallEntry, Inherited, Life},
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
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
    /// A value could not cross between Lua and JSON (a function, a cycle, a number with no JSON form).
    #[error("cannot convert a value between Lua and JSON: {0}")]
    BadValue(String),
    /// The app declared or rendered a window wrongly (no `render`, a malformed tree, an unknown source).
    #[error("invalid window: {0}")]
    InvalidWindow(String),
    /// The package does not contain the file its manifest names as the entry.
    #[error("the package has no entry `{0}`")]
    NoEntry(String),
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
    /// The call that holds the VM's gate is read-only: its database handle refuses writes and every other
    /// change to the world raises. It is the holder's own state (see [`Turn`]): set when a call takes the gate,
    /// cleared when it lets it go.
    pub read_only: Arc<AtomicBool>,
    /// How many read-only calls are alive: a `readOnly` tool (parked or running), a task it started that has
    /// not finished (`task.spawn`, and a `task.delay` or `task.defer` once it has begun), and a request that
    /// was itself made read-only by this count. A request that arrives from outside while this is above zero
    /// (a request to the app's own listener, say, which carries no origin) runs read-only too, because
    /// whoever sent it may be one of them. The taint ends when the last of them ends; it applies to nothing
    /// else: a task or a tool carries its own origin ([`Inherited`]), so a long-lived read-only poller does
    /// not make an unrelated writable tool's `task.delay` read-only.
    pub read_only_live: AtomicUsize,
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

/// Where a call came from, which decides what restricts it.
#[derive(Clone, Copy)]
enum Via {
    /// Through the host (a tool, an event, a write-back): `read_only` is what the caller requires.
    Host { read_only: bool },
    /// Work a call started (a deferred or delayed function): it is as restricted as that call was.
    Task(Inherited),
    /// A request from outside to the app's own listener: it has no origin, so it is read-only while any
    /// read-only call is alive ([`Shared::read_only_live`]).
    Request,
}

type LockWait = Pin<Box<dyn Future<Output = tokio::sync::OwnedMutexGuard<()>> + Send>>;

/// What a call keeps while it waits without using the VM (see [`Baton::park`]).
struct Parked {
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
    /// The place in the gate's queue of a call that is waiting to take it (see [`Baton::poll_unpark`]). It lives
    /// here, not in the host call that is waiting, so it stays with this call whichever call polls that host
    /// call later.
    waiting: Option<LockWait>,
    parked: Option<Parked>,
    /// The call came from inside the VM (a request to the app's server, a task), not through the host: its
    /// effects are forwarded by the runtime's pump when it ends.
    inbound: bool,
    /// The call may not change anything. This belongs to the call, and everything it starts inherits it: a
    /// task a `readOnly` tool spawns stays read-only for as long as it lives, whenever it runs. While the
    /// call holds the gate this is [`Shared::read_only`]; while it waits it is not (the next call's is). A
    /// coroutine continuation another call resumes (a host call left waiting by the task's first slice) is that
    /// call's: it takes the resumer's restrictions, as `coroutine.resume` runs the coroutine in the resumer.
    /// Counted in [`Shared::read_only_live`] for as long as the call lives.
    read_only: bool,
}

impl Turn {
    /// A call that waits for the gate and takes it.
    async fn take(shared: Arc<Shared>, gate: Arc<tokio::sync::Mutex<()>>, via: Via) -> Self {
        let held = gate.clone().lock_owned().await;
        let (inbound, read_only) = match via {
            Via::Host { read_only } => (false, read_only),
            Via::Task(from) => (true, from.read_only),
            Via::Request => (true, shared.read_only_live.load(Ordering::SeqCst) > 0),
        };
        let turn = Self::new(shared, gate, Some(held), inbound, read_only);
        turn.shared.read_only.store(read_only, Ordering::SeqCst);
        turn
    }

    /// A call that holds nothing yet (a coroutine's continuation): it takes the gate at its first wait's end.
    fn detached(shared: Arc<Shared>, gate: Arc<tokio::sync::Mutex<()>>, read_only: bool) -> Self {
        Self::new(shared, gate, None, true, read_only)
    }

    fn new(
        shared: Arc<Shared>,
        gate: Arc<tokio::sync::Mutex<()>>,
        held: Option<tokio::sync::OwnedMutexGuard<()>>,
        inbound: bool,
        read_only: bool,
    ) -> Self {
        if read_only {
            shared.read_only_live.fetch_add(1, Ordering::SeqCst);
        }
        Self { shared, gate, held, waiting: None, parked: None, inbound, read_only }
    }
}

impl Baton for Turn {
    fn holding(&self) -> bool {
        self.held.is_some()
    }

    fn inherited(&self) -> Inherited {
        Inherited { read_only: self.read_only }
    }

    fn can_park(&self) -> std::result::Result<(), String> {
        if self.shared.tx.lock().is_some() {
            return Err(
                "cannot wait inside db:tx: a transaction holds the database for as long as it is open, \
                        so finish it first (a call may wait before it starts one, or after it commits)"
                    .to_owned(),
            );
        }
        if self.shared.tracker.lock().is_pure() {
            return Err("cannot wait while rendering: render must be pure and quick".to_owned());
        }
        Ok(())
    }

    fn park(&mut self) {
        let scope = self.shared.tracker.lock().suspend();
        // the next call to hold the gate is not this one: its restrictions are its own
        self.shared.read_only.store(false, Ordering::SeqCst);
        self.parked = Some(Parked { scope });
        self.held = None;
    }

    fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.held.is_none() {
            let gate = &self.gate;
            let waiting = self.waiting.get_or_insert_with(|| Box::pin(gate.clone().lock_owned()));
            let Poll::Ready(held) = waiting.as_mut().poll(cx) else { return Poll::Pending };
            self.waiting = None;
            self.held = Some(held);
        }
        self.shared.read_only.store(self.read_only, Ordering::SeqCst);
        if let Some(parked) = self.parked.take() {
            self.shared.tracker.lock().resume(parked.scope);
        }
        Poll::Ready(())
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        if self.held.is_some() {
            // before the gate is released (fields drop after this body): dropping the transaction rolls it back
            drop(self.shared.tx.lock().take());
            self.shared.read_only.store(false, Ordering::SeqCst);
        }
        if self.read_only {
            self.shared.read_only_live.fetch_sub(1, Ordering::SeqCst);
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
    fn detached(&self, inherited: Inherited) -> Box<dyn Baton> {
        Box::new(Turn::detached(self.shared.clone(), self.gate.clone(), inherited.read_only))
    }

    fn enter(&self, inherited: Inherited) -> Pin<Box<dyn Future<Output = Box<dyn Baton>> + Send + 'static>> {
        let (shared, gate) = (self.shared.clone(), self.gate.clone());
        Box::pin(
            async move { Box::new(Turn::take(shared, gate, Via::Task(inherited)).await) as Box<dyn Baton> },
        )
    }

    fn request(&self) -> Pin<Box<dyn Future<Output = Box<dyn Baton>> + Send + 'static>> {
        let (shared, gate) = (self.shared.clone(), self.gate.clone());
        Box::pin(async move { Box::new(Turn::take(shared, gate, Via::Request).await) as Box<dyn Baton> })
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
            read_only_live: AtomicUsize::new(0),
            life,
            effects_signal: Arc::new(Notify::new()),
        });
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        // a request to the app's own server or the next slice of one of its tasks takes its turn like any call
        set_entry(&lua, Arc::new(Entry { shared: shared.clone(), gate: gate.clone() }));

        let env = app_env::new_env(&lua).map_err(|e| load_err(&e))?;
        let vm = Self { app: app.clone(), lua, shared, gate };
        vm.in_call(false, vm.boot(env)).await.map_err(|e| match e {
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
        let source =
            self.shared.package.files.get(&entry).cloned().ok_or_else(|| VmError::NoEntry(entry.clone()))?;
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
    ///
    /// `read_only`: the call may not change anything (a `readOnly` tool); whatever it starts may not either.
    pub(crate) async fn in_call<T>(&self, read_only: bool, body: impl Future<Output = T>) -> T {
        let turn = Turn::take(self.shared.clone(), self.gate.clone(), Via::Host { read_only }).await;
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

    // TODO: clean-code - 0.4 - DRY: builds the `ctx.actor` table that `view::ctx_actor` builds; call it.
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
        self.in_call(read_only, async {
            let convert = |e: mlua::Error| VmError::BadValue(e.to_string());
            let arg = json_to_lua(&self.lua, &input).map_err(convert)?;
            let ctx = self.ctx_table(ctx).map_err(convert)?;
            let out: LuaValue = run.call_async((arg, ctx)).await.map_err(|e| script(&e))?;
            lua_to_json(&out).map_err(|e| VmError::BadValue(e.to_string()))
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
        self.in_call(false, async {
            let convert = |e: mlua::Error| VmError::BadValue(e.to_string());
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
        self.in_call(false, async {
            let arg = json_to_lua(&self.lua, &payload).map_err(|e| VmError::BadValue(e.to_string()))?;
            // closed by the guard if this call is dropped at a host call
            let scope = track.then(|| TrackGuard::begin(&self.shared.tracker));
            let result = handler.call_async::<LuaValue>(arg).await;
            let deps = scope.map(TrackGuard::finish).unwrap_or_default();
            let out = result.map_err(|e| script(&e))?;
            Ok((lua_to_json(&out).map_err(|e| VmError::BadValue(e.to_string()))?, deps))
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

    /// Has the app been unloaded or replaced ([`AppVm::shutdown`])? What a call it still had in flight queues
    /// after that is dropped.
    pub fn is_shut_down(&self) -> bool {
        self.shared.life.is_closed()
    }

    /// Unload: release every Rust-held reference to Lua values, then the VM. Returns the heap size freed.
    pub fn unload(self) -> usize {
        let used = self.lua.used_memory();
        self.shared.life.shutdown();
        self.shared.release();
        used
    }
}
