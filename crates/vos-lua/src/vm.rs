//! One sandboxed Lua VM (Luau, running the app's Luau source) per app.
//!
//! The VM is a bare `neoc-vm` engine with the app's own environment table (see
//! `neoc_vm::lua::script_env`): the prelude's polyfills, the `app` capability table and a `require` that
//! knows only the host modules. Nothing in it is budgeted (no CPU, memory or time limits: the host is a
//! homelab's own and apps are meant to do real work); it then runs the app's entry. After that the host
//! calls into it: tools, write-back callbacks, events. One call runs in a VM at a time (an app is
//! single-threaded by design); the VM lives as long as its `AppVm`.

use crate::{
    api,
    convert::{json_to_lua, lua_to_json},
    error::{ScriptError, ToolFailure},
    state::{DepKey, SharedTracker, TrackGuard, Tracker, TrackerScope},
    tx::OpenTx,
    view::{RenderScope, ViewInstance},
};
use neoc_vm::mlua;
use neoc_vm::{
    lua::{
        lib::log::set_log_sink,
        lifecycle::{self, set_entry, Baton, BoxFuture, CallEntry, IdleKind, Inherited, Life},
        panics,
        sandbox::{install_runtime_state, load_source, make_bare_engine, set_worker_engine},
        script_env,
    },
    mlua::{Function, Lua, Table, Value as LuaValue},
};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::sync::Notify;
use vos_application::{
    live::{SourceDef, SourceId},
    ports::{AgentCard, AgentRoster, AppData, AppPackage, CallContext, ViewEffect},
    AppFiles, Files, NoFiles,
};
use vos_domain::{time, tool_schema, AppDecl, AppId, Notification, ToolSpec};

/// The prelude (`lua/prelude.lua`, copied by `bun run codegen`): `typeof` and `utf8`, which Luau has and the app's
/// environment does not copy from the engine. Run once in each app's environment before the app.
const PRELUDE: &str = include_str!("../../../types/generated/prelude.lua");

/// A value's type as Luau's `type()` names it (the engine tells integers from other numbers; a script does not).
fn lua_type(value: &LuaValue) -> &'static str {
    match value {
        LuaValue::Integer(_) | LuaValue::Number(_) => "number",
        other => other.type_name(),
    }
}

/// The longest name a handler may have (and so the longest `rpc` method a client may call).
pub const MAX_HANDLER_NAME: usize = 128;

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
    /// A client called a handler the backend does not have.
    #[error("the app has no handler `{0}`")]
    NoSuchHandler(String),
    /// The entry returned something that is not a table of handler functions.
    #[error("the entry must return nothing or a table of functions by name: {0}")]
    BadHandlers(String),
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
    /// A host function the call reached (a module's Rust code, a port behind the `app` API) panicked, and the
    /// script did not catch the error it raised. The call is over; the app keeps running.
    #[error("a host function panicked: {0}")]
    Panicked(String),
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
    /// Luau source of host modules `require("vos:<name>")` can load besides `vos:app`
    /// (the generated `vos:ui` constructors), by module name.
    pub host_modules: Arc<HashMap<String, Vec<u8>>>,
    /// The agent contacts `app.agents:list()` shows (empty unless the host gives a roster).
    pub roster: Arc<dyn AgentRoster>,
    /// The app's managed files (`app.files`); every call is `unsupported` unless the host gives a store.
    pub files: Arc<dyn AppFiles>,
    /// Library modules this VM does not offer, on top of [`crate::NOT_FOR_APPS`] (its `std:thread` workers neither).
    /// The host leaves it empty and there is no setting for it: a test uses it to show that a capability works
    /// for an app that cannot `require` a module (`app.files` without `std:fs`).
    pub refused_modules: Vec<String>,
    /// Give the app's chunks a `hook(name, fn)` global that [`AppVm::emit`] calls: how the adapter's tests reach
    /// app code without declaring a tool. It is not part of the app API (`app` has no event hook, and the host
    /// emits nothing), the host leaves it off and there is no setting for it.
    pub test_hooks: bool,
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
        Self {
            host_modules: Arc::default(),
            roster: Arc::new(NoRoster),
            files: Arc::new(NoFiles),
            refused_modules: Vec::new(),
            test_hooks: false,
        }
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
    /// `app.emit(name, data)`: an event for the app's open clients.
    Emit {
        name: String,
        data: Value,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellEffect {
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
    /// `app.files`, through the use case that checks every path.
    pub files: Files,
    /// The library modules this app is offered (see [`crate::require::offered_modules`]).
    pub offered: Arc<Vec<&'static str>>,
    pub host_modules: Arc<HashMap<String, Vec<u8>>>,
    pub roster: Arc<dyn AgentRoster>,
    pub tracker: SharedTracker,
    pub tools: Mutex<Vec<ToolDecl>>,
    /// What `hook(name, fn)` registered (only with [`VmConfig::test_hooks`]).
    pub handlers: Mutex<HashMap<String, Function>>,
    /// The backend of an app with a client: the functions its entry returned, by name. A client's `rpc(name, args)`
    /// calls one.
    pub rpc_handlers: Mutex<HashMap<String, Function>>,
    pub test_hooks: bool,
    pub sources: Mutex<Vec<SourceDef>>,
    pub writers: Mutex<HashMap<String, Function>>,
    pub windows: Mutex<HashMap<String, Table>>,
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
    /// The open `db:tx(fn)` transaction, if any. It belongs to the turn that opened it (see [`crate::tx`]);
    /// nothing holds it across an await, so a call that was cut off cannot wedge the next one.
    pub tx: Mutex<Option<OpenTx>>,
    /// The call that holds the VM's gate is read-only: its database handle refuses writes and every other
    /// change to the world raises. It is the holder's own state (see [`Turn`]): set when a call takes the gate,
    /// cleared when it lets it go.
    pub read_only: Arc<AtomicBool>,
    /// How many read-only calls are *active*: a `readOnly` tool, a task it started (`task.spawn`, and a
    /// `task.delay` or `task.defer` once it has begun) or a request that was itself made read-only by this count,
    /// that is running, queued for the VM, or waiting at something in flight (the network, a worker, a lock),
    /// but not one that is asleep at a timer (`task.wait`, a `std:time` sleep), which sends nothing and starts
    /// nothing until the timer ends and it has taken its turn back ([`Turn::quiet`]). A request that arrives
    /// from outside while this is above zero (a request to the app's own listener, say, which carries no origin)
    /// runs read-only too, because whoever sent it may be one of them. The taint lasts exactly as long as some
    /// read-only call is acting, not as long as one is alive, so a read-only tool that leaves a poller behind
    /// does not make the app's webhooks read-only for the poller's life; and it applies to nothing else: a task
    /// or a tool carries its own origin ([`Inherited`]), so a long-lived read-only poller does not make an
    /// unrelated writable tool's `task.delay` read-only.
    pub read_only_active: AtomicUsize,
    /// The next [`Baton::id`]: unique among this VM's turns.
    pub next_turn: AtomicU64,
    /// Everything the app spawned (tasks, servers, workers): aborted when the app is unloaded.
    pub life: Life,
    /// Woken when a call that nobody made through the host (a request to the app's own server, a task's next
    /// slice) ends or pauses at a wait, so the runtime forwards the effects it queued (`app.notify`, `app.shell`)
    /// and tells the application that state may have changed.
    pub effects_signal: Arc<Notify>,
    /// The app's background work has been stopped (a reload began, or the app is being unloaded): its listeners are
    /// gone, so what still reaches its own server is a straggler of work that was cut off, and a straggler whose
    /// sender was a read-only call must not run writable. Set before anything is aborted, never cleared.
    pub background_stopped: AtomicBool,
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

    /// The app's files as the running call may use them: a handle that refuses writes inside a `readOnly` tool.
    pub fn files(&self) -> Files {
        if self.read_only.load(Ordering::SeqCst) {
            self.files.read_only()
        } else {
            self.files.clone()
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

    /// A window named by `openWindow` (or listed by `app:windows`) must be one the app declared: an app cannot
    /// ask the shell for anything else (the request is broadcast to every client). `call` is the call that
    /// named it (`app:windows`), which the error says.
    pub fn declared_window(&self, call: &str, name: &str) -> mlua::Result<()> {
        let windows = self.windows.lock();
        if windows.contains_key(name) {
            return Ok(());
        }
        let mut declared: Vec<&str> = windows.keys().map(String::as_str).collect();
        declared.sort_unstable();
        Err(mlua::Error::runtime(format!(
            "{call}: no window `{name}` is declared (declared: {})",
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
        self.rpc_handlers.lock().clear();
        self.writers.lock().clear();
        self.windows.lock().clear();
        self.deep_links.lock().clear();
        self.modules.lock().clear();
        self.views.lock().clear();
        *self.env.lock() = None;
        *self.app_table.lock() = None;
        *self.fail.lock() = None;
    }
}

/// What the VM carries from a call to the work it starts (`Inherited`): the read-only restriction, which is
/// the adapter's policy, not the library's.
#[derive(Clone, Copy)]
struct ReadOnly(bool);

fn inherits_read_only(from: &Inherited) -> bool {
    from.get::<ReadOnly>().is_some_and(|r| r.0)
}

/// Where a call came from, which decides what restricts it.
enum Via {
    /// Through the host (a tool, an event, a write-back): `read_only` is what the caller requires.
    Host { read_only: bool },
    /// Work a call started (a deferred or delayed function): it is as restricted as that call was.
    Task(Inherited),
    /// A request from outside to the app's own listener: it has no origin, so it is read-only while any
    /// read-only call is active ([`Shared::read_only_active`]).
    Request,
}

/// What a call keeps while it waits without using the VM (see [`Baton::park`]).
struct Parked {
    scope: TrackerScope,
}

/// A call in progress: it holds the VM's gate, so one call runs in the VM at a time and `db:tx`, the
/// `readOnly` flag and state tracking are unambiguous. Ending a call, or abandoning it (its caller gave up),
/// rolls back a transaction it left open, so the database's write gate is free the moment the call is over.
///
/// It is also the call's *baton* (see `neoc_vm::lua::lifecycle`): at a wait that does not use the VM
/// (`task.wait`, the network, a worker) the call sets its own state aside and lets the next call in, then
/// takes the gate and its state back.
pub(crate) struct Turn {
    id: u64,
    shared: Arc<Shared>,
    gate: Arc<tokio::sync::Mutex<()>>,
    held: Option<tokio::sync::OwnedMutexGuard<()>>,
    /// The place in the gate's queue of a call that is waiting to take it (see [`Baton::poll_unpark`]). It lives
    /// here, not in the host call that is waiting, so it stays with this call whichever call polls that host
    /// call later.
    waiting: Option<BoxFuture<'static, tokio::sync::OwnedMutexGuard<()>>>,
    parked: Option<Parked>,
    /// The call came from inside the VM (a request to the app's server, a task), not through the host: its
    /// effects are forwarded by the runtime's pump when it ends.
    inbound: bool,
    /// The call may not change anything. This belongs to the call, and everything it starts inherits it: a
    /// task a `readOnly` tool spawns stays read-only for as long as it lives, whenever it runs. While the
    /// call holds the gate this is [`Shared::read_only`]; while it waits it is not (the next call's is). A
    /// coroutine continuation another call resumes (a host call left waiting by the task's first slice) is that
    /// call's: it takes the resumer's restrictions, as `coroutine.resume` runs the coroutine in the resumer.
    /// Counted in [`Shared::read_only_active`] for as long as the call lives, except while it is quiet.
    read_only: bool,
    /// The call is not running and is asleep at a timer (see [`Baton::idle_at`]): it can have nothing in flight,
    /// so it is not counted in [`Shared::read_only_active`] until its wait ends (it asks for its turn back,
    /// [`Baton::poll_unpark`]). Every way back to running script code goes through that, so a call that is
    /// quiet is never the one executing.
    quiet: bool,
}

impl Turn {
    /// A call that waits for the gate and takes it.
    async fn take(shared: Arc<Shared>, gate: Arc<tokio::sync::Mutex<()>>, via: Via) -> Self {
        let held = gate.clone().lock_owned().await;
        let (inbound, read_only) = match via {
            Via::Host { read_only } => (false, read_only),
            Via::Task(from) => (true, inherits_read_only(&from)),
            // The count is read before the flag. Stopping sets the flag before it aborts anything, and aborting is
            // what removes a read-only call from the count, so a request that finds the count empty because its
            // caller was cut off finds the flag set.
            Via::Request => {
                let acting = shared.read_only_active.load(Ordering::SeqCst) > 0;
                (true, acting || shared.background_stopped.load(Ordering::SeqCst))
            }
        };
        // An app that is shut down (unloaded, or replaced by a reload) runs no more script code on its own behalf:
        // what was queued for the VM is dropped here instead of running. Aborting a task is not enough, because a
        // task that is being polled when it is aborted still runs on to its next wait, and this is where it asks
        // for its turn. The order matters: `Life::shutdown` sets `closed` before it abandons the calls waiting at
        // something in flight, and that is what ends a read-only call and drops `read_only_active` to nothing, so
        // a request that saw that count at zero because its read-only caller was cut off by the shutdown sees
        // `closed` here (the count is read first, this second). It would otherwise run as an ordinary, writable
        // request, for a call that was read-only.
        if inbound && shared.life.is_closed() {
            drop(held);
            match std::future::pending::<std::convert::Infallible>().await {}
        }
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
            shared.read_only_active.fetch_add(1, Ordering::SeqCst);
        }
        Self {
            id: shared.next_turn.fetch_add(1, Ordering::Relaxed),
            shared,
            gate,
            held,
            waiting: None,
            parked: None,
            inbound,
            read_only,
            quiet: false,
        }
    }

    /// Set whether the call is quiet, keeping [`Shared::read_only_active`] in step.
    fn set_quiet(&mut self, quiet: bool) {
        if self.read_only && self.quiet != quiet {
            if quiet {
                self.shared.read_only_active.fetch_sub(1, Ordering::SeqCst);
            } else {
                self.shared.read_only_active.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.quiet = quiet;
    }
}

impl Baton for Turn {
    fn id(&self) -> u64 {
        self.id
    }

    fn holding(&self) -> bool {
        self.held.is_some()
    }

    fn inherited(&self) -> Inherited {
        Inherited::new(ReadOnly(self.read_only))
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
        // a background call that pauses has run some script since the last time it let go of the VM: its state
        // changes (and a window's `w:toast`) are for the host to see now, not when the call is finally over,
        // which for a loop is never. The gate is free by now.
        if self.inbound {
            self.shared.effects_signal.notify_one();
        }
    }

    fn idle_at(&mut self, at: IdleKind) {
        self.set_quiet(at == IdleKind::Timer);
    }

    fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        // only asked of a call that does not hold the gate (see `lifecycle::hold`). Its wait is over (or it was
        // never at a timer): from here it may run script code once it has the gate, so it counts again, before
        // it can send anything
        self.set_quiet(false);
        let gate = &self.gate;
        let waiting = self.waiting.get_or_insert_with(|| Box::pin(gate.clone().lock_owned()));
        let Poll::Ready(held) = waiting.as_mut().poll(cx) else { return Poll::Pending };
        self.waiting = None;
        self.held = Some(held);
        self.shared.read_only.store(self.read_only, Ordering::SeqCst);
        if let Some(parked) = self.parked.take() {
            self.shared.tracker.lock().resume(parked.scope);
        }
        Poll::Ready(())
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        {
            // before the gate is released (fields drop after this body): a transaction this call left open is
            // rolled back, so the database is free the moment the call is over
            let mut open = self.shared.tx.lock();
            if open.as_ref().is_some_and(|tx| tx.owner() == self.id) {
                drop(open.take());
            }
        }
        if self.held.is_some() {
            self.shared.read_only.store(false, Ordering::SeqCst);
        }
        if self.read_only && !self.quiet {
            self.shared.read_only_active.fetch_sub(1, Ordering::SeqCst);
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
        Box::new(Turn::detached(self.shared.clone(), self.gate.clone(), inherits_read_only(&inherited)))
    }

    fn enter(&self, inherited: Inherited) -> BoxFuture<'static, Box<dyn Baton>> {
        let (shared, gate) = (self.shared.clone(), self.gate.clone());
        Box::pin(
            async move { Box::new(Turn::take(shared, gate, Via::Task(inherited)).await) as Box<dyn Baton> },
        )
    }

    fn request(&self) -> BoxFuture<'static, Box<dyn Baton>> {
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

/// A script's error as the VM reports it: `ctx:fail` is a failed tool call, anything else a script error.
pub(crate) fn script(e: &mlua::Error) -> VmError {
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
        let VmConfig { host_modules, roster, files, refused_modules, test_hooks } = config;
        let offered = Arc::new(crate::require::offered_modules(&refused_modules));
        let load_err =
            |e: &mlua::Error| VmError::Load { app: app.to_string(), error: ScriptError::from_lua(e) };
        let lua = make_bare_engine().map_err(|e| load_err(&e))?;
        let life = Life::new();
        // what the `std:`/`lib:`/`vnd:` modules keep per VM (the task scheduler, the registry of what the app
        // spawns), and where `lib:log` writes: the host's tracing, with the app's id
        install_runtime_state(&lua, life.clone());
        // a `std:thread` worker an app starts is offered what the app is offered: the library's module list
        // without the ones an app is not given (`require::NOT_FOR_APPS`)
        let worker_offered = offered.clone();
        set_worker_engine(
            &lua,
            Arc::new(move |life| {
                let lua = make_bare_engine()?;
                install_runtime_state(&lua, life);
                neoc_vm::lua::modules::install_allowed(&lua, &worker_offered)?;
                Ok(lua)
            }),
        );
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
            files: Files::new(files),
            offered,
            host_modules,
            roster,
            tracker: Arc::new(Mutex::new(Tracker::new(read_only.clone()))),
            tools: Mutex::default(),
            handlers: Mutex::default(),
            rpc_handlers: Mutex::default(),
            test_hooks,
            sources: Mutex::default(),
            writers: Mutex::default(),
            windows: Mutex::default(),
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
            read_only_active: AtomicUsize::new(0),
            next_turn: AtomicU64::new(1),
            life,
            effects_signal: Arc::new(Notify::new()),
            background_stopped: AtomicBool::new(false),
        });
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        // a request to the app's own server or the next slice of one of its tasks takes its turn like any call
        set_entry(&lua, Arc::new(Entry { shared: shared.clone(), gate: gate.clone() }));

        let env = script_env::new_env(&lua).map_err(|e| load_err(&e))?;
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
        script_env::freeze(&self.lua, &env)?;
        *self.shared.env.lock() = Some(env.clone());
        api::install(&self.lua, &self.shared, &env)?;
        let entry = self.shared.package.entry.clone();
        let source =
            self.shared.package.files.get(&entry).cloned().ok_or_else(|| VmError::NoEntry(entry.clone()))?;
        let returned: LuaValue = load_source(&self.lua, source.as_slice())
            .set_name(self.shared.chunk_name(&entry))
            .set_environment(env)
            .call_async(())
            .await?;
        self.keep_handlers(returned)?;
        Ok(())
    }

    /// What the entry returned is the backend: a table of functions by name (or nothing, for an app that only
    /// declares tools and windows). Anything else is a mistake the author should hear about at load.
    fn keep_handlers(&self, returned: LuaValue) -> std::result::Result<(), VmError> {
        let table = match returned {
            LuaValue::Nil => return Ok(()),
            LuaValue::Table(t) => t,
            other => return Err(VmError::BadHandlers(format!("it returned a {}", lua_type(&other)))),
        };
        let mut handlers = HashMap::new();
        for pair in table.pairs::<LuaValue, LuaValue>() {
            let (key, value) = pair.map_err(|e| VmError::BadHandlers(e.to_string()))?;
            let LuaValue::String(name) = key else {
                return Err(VmError::BadHandlers(format!("a key is a {}, not a name", lua_type(&key))));
            };
            let name = name.to_string_lossy();
            let LuaValue::Function(handler) = value else {
                return Err(VmError::BadHandlers(format!(
                    "`{name}` is a {}, not a function",
                    lua_type(&value)
                )));
            };
            if name.is_empty() || name.len() > MAX_HANDLER_NAME {
                return Err(VmError::BadHandlers(format!(
                    "a handler name is 1 to {MAX_HANDLER_NAME} characters"
                )));
            }
            handlers.insert(name, handler);
        }
        *self.shared.rpc_handlers.lock() = handlers;
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

    /// Woken when a call that came from inside the VM ends or pauses (see [`Shared::effects_signal`]).
    pub fn effects_signal(&self) -> Arc<Notify> {
        self.shared.effects_signal.clone()
    }

    /// Run a top-level call: wait for the previous one, then run `body` holding the VM's gate (see [`Turn`]).
    ///
    /// `read_only`: the call may not change anything (a `readOnly` tool); whatever it starts may not either.
    ///
    /// A panic in a host function the call reached, which the script did not catch, ends the call with
    /// [`VmError::Panicked`] instead of unwinding into the caller (`neoc_vm::lua::panics`). It stops the call
    /// where script code called into Rust, where a caller that gives up may drop the call too, so it leaves what a
    /// dropped call leaves: the turn handed on, a `db:tx` it opened rolled back, tracking and the render scope
    /// closed (the guards of [`Turn`], `TrackGuard` and `RenderScopeGuard`). The app stays loaded.
    pub(crate) async fn in_call<T, E: From<VmError>>(
        &self,
        read_only: bool,
        body: impl Future<Output = std::result::Result<T, E>>,
    ) -> std::result::Result<T, E> {
        let turn = Turn::take(self.shared.clone(), self.gate.clone(), Via::Host { read_only }).await;
        match panics::caught(lifecycle::scoped(Box::new(turn), body)).await {
            Ok(out) => out,
            Err(message) => Err(VmError::Panicked(message).into()),
        }
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

    /// A client's `rpc(method, args)`: run the handler the entry returned under that name as `handler(ctx, args)`,
    /// as a call of its own. It is the person's own client asking, so the call may write (an agent's tool is the
    /// one that can be `readOnly`).
    pub async fn call_handler(&self, name: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        let handler = self
            .shared
            .rpc_handlers
            .lock()
            .get(name)
            .cloned()
            .ok_or_else(|| VmError::NoSuchHandler(name.chars().take(MAX_HANDLER_NAME).collect()))?;
        self.in_call(false, async {
            let convert = |e: mlua::Error| VmError::BadValue(e.to_string());
            let arg = json_to_lua(&self.lua, &args).map_err(convert)?;
            let ctx = self.ctx_table(ctx).map_err(convert)?;
            let out: LuaValue = handler.call_async((ctx, arg)).await.map_err(|e| script(&e))?;
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

    /// Call what the app registered with `hook(name, fn)` (a VM loaded with [`VmConfig::test_hooks`]) with a JSON
    /// payload, as one call of its own; returns the handler's JSON result, or null when nothing is registered.
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
            let deps = scope.map_or_default(TrackGuard::finish);
            let out = result.map_err(|e| script(&e))?;
            Ok((lua_to_json(&out).map_err(|e| VmError::BadValue(e.to_string()))?, deps))
        })
        .await
    }

    /// Has a window something to show that no call through the host has delivered: an `app.state` field changed
    /// or a window asked for a toast or a client value since the last settle?
    pub fn has_unsettled_views(&self) -> bool {
        self.shared.tracker.lock().has_dirty() || !self.shared.view_effects.lock().is_empty()
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
        self.shared.background_stopped.store(true, Ordering::SeqCst);
        self.shared.life.stop_background();
    }

    /// The app is going away: everything it started (tasks, servers and their connections, workers) is aborted,
    /// and a host call a running call is waiting on is abandoned. Call this before dropping the last `Arc`, so
    /// an app cannot keep a listener or a task alive after an uninstall or a reload.
    pub fn shutdown(&self) {
        self.shared.background_stopped.store(true, Ordering::SeqCst);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use vos_application::testing::ScriptedData;

    async fn vm() -> AppVm {
        let package = AppPackage {
            entry: "app.lua".into(),
            files: [("app.lua".to_owned(), b"-- an app that declares nothing".to_vec())].into(),
            ..AppPackage::default()
        };
        AppVm::load(
            &AppId::parse("probe").unwrap(),
            package,
            Arc::new(ScriptedData::default()),
            VmConfig::default(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_request_that_arrives_once_the_background_work_is_stopped_is_read_only() {
        let vm = vm().await;
        let entry = Entry { shared: vm.shared.clone(), gate: vm.gate.clone() };
        let read_only = || async { inherits_read_only(&entry.request().await.inherited()) };
        assert!(!read_only().await, "control: with no read-only call acting a request is an ordinary one");

        // a reload begins: the listeners are gone, and what is left reaching the server is the straggler of a call
        // that was cut off (which may have been read-only, with nothing left to say so)
        vm.stop_background();
        assert!(read_only().await);
    }

    #[tokio::test]
    async fn a_request_or_task_that_asks_for_its_turn_after_the_shutdown_never_runs() {
        let vm = vm().await;
        let entry = Entry { shared: vm.shared.clone(), gate: vm.gate.clone() };
        let patience = Duration::from_millis(150);
        // control: before the shutdown both take their turn at once
        drop(tokio::time::timeout(patience, entry.request()).await.expect("a request takes its turn"));
        drop(
            tokio::time::timeout(patience, entry.enter(Inherited::new(ReadOnly(false))))
                .await
                .expect("a task takes its turn"),
        );

        vm.shutdown();

        // a request that was queued for the VM (a read-only call's own request, whose caller the shutdown cut
        // off) would otherwise run as an ordinary one: it is dropped instead, and it does not keep the gate
        assert!(
            tokio::time::timeout(patience, entry.request()).await.is_err(),
            "a request ran after the shutdown"
        );
        assert!(
            tokio::time::timeout(patience, entry.enter(Inherited::new(ReadOnly(false)))).await.is_err(),
            "a task ran after the shutdown"
        );
        tokio::time::timeout(patience, vm.in_call(false, async { Ok::<_, VmError>(()) }))
            .await
            .expect("the gate is free")
            .expect("the call ran");
    }
}
