//! One sandboxed Lua 5.1 VM per app.
//!
//! The VM is a bare `agentos-luavm` engine with the app's own environment table (see
//! `agentos_luavm::lua::app_env`): the prelude's polyfills, the `app` capability table and a `require` that
//! knows only the host modules. It is limited (memory, a CPU budget enforced by the VM's count hook, a
//! host-call timeout) and then runs the app's entry. After that the host calls into it: tools, write-back
//! callbacks, events. One call runs in a VM at a time (an app is single-threaded by design); the VM lives
//! as long as its `AppVm`.

use crate::{
    api,
    budget::{Allowance, CpuBudget, HOOK_INSTRUCTIONS},
    convert::{json_to_lua, lua_to_json},
    error::{ScriptError, ToolFailure},
    state::{DepKey, SharedTracker, Tracker},
    view::{RenderScope, ViewInstance},
};
use agentos_application::{
    data::DataTx,
    live::{SourceDef, SourceId},
    ports::{AgentCard, AgentRoster, AppData, AppPackage, CallContext, MenuCommand, ViewEffect},
};
use agentos_domain::{time, tool_schema, AppDecl, AppId, Capabilities, Notification, ToolSpec};
use agentos_luavm::mlua;
use agentos_luavm::{
    lua::{
        app_env, patterns,
        patterns::PatternLimits,
        sandbox::{load_source, make_bare_engine},
    },
    mlua::{Function, HookTriggers, Lua, Table, Value as LuaValue, VmState},
};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

/// The Lua 5.1 polyfills of the Luau standard library (`lua/prelude.luau`, built by `bun run codegen`): run
/// once in each app's environment before the app.
const PRELUDE: &str = include_str!("../../../types/generated/prelude.lua");

#[derive(Debug, thiserror::Error)]
pub enum VmError {
    /// The entry point (or a module it requires) failed while loading.
    #[error("app `{app}` failed to load: {error}")]
    Load { app: String, error: ScriptError },
    /// App code raised an error, or exceeded its budget.
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

#[derive(Clone, Debug)]
pub struct Limits {
    /// Hard cap on the Lua heap (allocations past this raise a Lua `MemoryError`). It also caps what the
    /// host-side string functions (`gsub`, `rep`) may build.
    pub memory_bytes: usize,
    /// Max ticks (one per `HOOK_INSTRUCTIONS` VM instructions; the pattern matcher is charged in the same
    /// unit) between host awaits: a `while true do end` is killed after this many, however loaded the
    /// machine is (see `budget`).
    pub cpu_ticks: u64,
    /// Wall-clock backstop on the same slice: far above an honest slice, it only catches what the count
    /// cannot see.
    pub cpu_wall: Duration,
    /// Max ticks in one whole top-level call (a tool, an event, a render), host calls included at
    /// `budget::HOST_CALL_TICKS` each. Host awaits refill the slice (`cpu_ticks`) but never this, so
    /// `while true do app.kv:get("k") end` is killed. 2.5 million is five slices: an honest call is a few
    /// hundred ticks and a handful of host calls.
    pub call_ticks: u64,
    /// Wall-clock limit of one top-level call, host waits included (a call also holds the VM's gate, and
    /// inside `db:tx` the database's write gate). Thirty seconds is three host-call timeouts: far above any
    /// honest call.
    pub call_wall: Duration,
    /// Max effects (`app.notify`, `app:openWindow`, window commands) one call may queue. Effects are
    /// broadcast to every connected client, so an app must not be able to flood them. 32 is far above what an
    /// honest call queues (one or two); shell declarations replace each other and do not count.
    pub max_effects: usize,
    /// Max wall-clock time of one host call (a database call that never returns).
    pub host_call_timeout: Duration,
}
impl Limits {
    pub(crate) fn allowance(&self) -> Allowance {
        Allowance {
            ticks: self.cpu_ticks,
            wall: self.cpu_wall,
            call_ticks: self.call_ticks,
            call_wall: self.call_wall,
        }
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: 16 * 1024 * 1024,
            cpu_ticks: 500_000,
            cpu_wall: Duration::from_secs(10),
            call_ticks: 2_500_000,
            call_wall: Duration::from_secs(30),
            max_effects: 32,
            host_call_timeout: Duration::from_secs(10),
        }
    }
}

/// How a VM is built.
#[derive(Clone)]
pub struct VmConfig {
    pub capabilities: Capabilities,
    pub limits: Limits,
    /// Lua 5.1 source of host modules `require("@agentos/<name>")` can load besides `@agentos/app`
    /// (the generated `@agentos/ui` constructors), by module name.
    pub host_modules: Arc<HashMap<String, Vec<u8>>>,
    /// The agent contacts `app.agents:list()` shows (empty unless the host gives a roster).
    pub roster: Arc<dyn AgentRoster>,
}

impl std::fmt::Debug for VmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VmConfig")
            .field("capabilities", &self.capabilities)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
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
            capabilities: Capabilities::all(),
            limits: Limits::default(),
            host_modules: Arc::default(),
            roster: Arc::new(NoRoster),
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
    pub caps: Capabilities,
    pub package: AppPackage,
    pub db: Arc<dyn AppData>,
    pub budget: CpuBudget,
    pub limits: Limits,
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
    /// Effects queued by the running call, against `limits.max_effects` (reset when a call starts).
    pub effects_in_call: AtomicUsize,
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

    /// Count one more effect against the call's cap.
    pub fn spend_effect(&self, what: &str) -> mlua::Result<()> {
        let max = self.limits.max_effects;
        if self.effects_in_call.fetch_add(1, Ordering::Relaxed) >= max {
            return Err(mlua::Error::runtime(format!(
                "{what}: too many effects in one call (at most {max}: notifications, windows, window commands)"
            )));
        }
        Ok(())
    }

    /// Queue something for the host to do after the call (counted against the cap).
    pub fn push_effect(&self, what: &str, effect: Effect) -> mlua::Result<()> {
        self.spend_effect(what)?;
        self.effects.lock().push(effect);
        Ok(())
    }

    /// Queue what a window asked of its view (counted against the cap). A window context outlives the call
    /// that rendered it (an app can keep `w` in a variable), so this is refused inside a `readOnly` tool like
    /// every other change to the world.
    pub fn push_view_effect(&self, what: &str, view: &str, effect: ViewEffect) -> mlua::Result<()> {
        self.writable(what)?;
        self.spend_effect(what)?;
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

/// A top-level call in progress: it holds the VM's gate. Ending it, or abandoning it (its caller gave up), rolls
/// back a transaction it left open, so the database's write gate is free the moment the call is over rather than
/// when some later call happens to start.
pub(crate) struct CallGuard<'a> {
    shared: &'a Shared,
    _gate: tokio::sync::MutexGuard<'a, ()>,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        // before the gate is released (fields drop after this body): dropping the transaction rolls it back
        drop(self.shared.tx.lock().take());
    }
}

pub struct AppVm {
    pub app: AppId,
    lua: Lua,
    shared: Arc<Shared>,
    /// Serialises calls: one runs at a time, so `db:tx`, the CPU budget and state tracking are unambiguous.
    gate: tokio::sync::Mutex<()>,
}

impl Drop for AppVm {
    fn drop(&mut self) {
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
        let VmConfig { capabilities, limits, host_modules, roster } = config;
        let load_err =
            |e: &mlua::Error| VmError::Load { app: app.to_string(), error: ScriptError::from_lua(e) };
        let lua = make_bare_engine().map_err(|e| load_err(&e))?;
        let budget = CpuBudget::new();
        let read_only = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared {
            app: app.clone(),
            caps: capabilities,
            package,
            db,
            budget: budget.clone(),
            limits: limits.clone(),
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
            effects_in_call: AtomicUsize::new(0),
        });

        // The CPU budget. The count hook runs every HOOK_INSTRUCTIONS VM instructions, on the main thread and
        // (set globally) on every coroutine mlua creates; a coroutine the app creates inherits it from its
        // creator. Returning Err raises in the running code; the kill is sticky so a `pcall` cannot swallow it.
        let allowance = limits.allowance();
        let hook_budget = budget.clone();
        lua.set_global_hook(HookTriggers::new().every_nth_instruction(HOOK_INSTRUCTIONS), move |_, _| {
            match hook_budget.exceeded(allowance) {
                Some(stop) => Err(mlua::Error::runtime(stop.message())),
                None => Ok(VmState::Continue),
            }
        })
        .map_err(|e| load_err(&e))?;
        // The host-side pattern functions (the C matcher never polls the hook) pay into the same budget.
        let pattern_budget = budget.clone();
        patterns::install(
            &lua,
            PatternLimits {
                charge: Arc::new(move |ticks| match pattern_budget.spend(allowance, ticks) {
                    Some(stop) => Err(mlua::Error::runtime(stop.message())),
                    None => Ok(()),
                }),
                max_output: limits.memory_bytes,
            },
        )
        .map_err(|e| load_err(&e))?;
        lua.set_memory_limit(limits.memory_bytes).map_err(|e| load_err(&e))?;

        let env = app_env::new_env(&lua).map_err(|e| load_err(&e))?;
        let vm = Self { app: app.clone(), lua, shared, gate: tokio::sync::Mutex::new(()) };
        let call = vm.enter().await;
        // the polyfills go in before the environment is sealed, and the host's own `app` and `require` after
        load_source(&vm.lua, PRELUDE)
            .set_name("=prelude")
            .set_environment(env.clone())
            .call::<()>(env.clone())
            .map_err(|e| load_err(&e))?;
        app_env::freeze(&vm.lua, &env).map_err(|e| load_err(&e))?;
        *vm.shared.env.lock() = Some(env.clone());
        api::install(&vm.lua, &vm.shared, &env).map_err(|e| load_err(&e))?;
        let entry = vm.shared.package.entry.clone();
        let source = vm
            .shared
            .package
            .files
            .get(&entry)
            .cloned()
            .ok_or_else(|| VmError::Convert(format!("the package has no entry `{entry}`")))?;
        load_source(&vm.lua, source.as_slice())
            .set_name(vm.shared.chunk_name(&entry))
            .set_environment(env)
            .exec_async()
            .await
            .map_err(|e| load_err(&e))?;
        drop(call);
        Ok(vm)
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }

    pub(crate) async fn enter_call(&self) -> CallGuard<'_> {
        self.enter().await
    }

    /// Start a top-level call: wait for the previous one, then fresh CPU budgets and a fresh effect count. The
    /// returned guard ends the call (see [`CallGuard`]).
    async fn enter(&self) -> CallGuard<'_> {
        let gate = self.gate.lock().await;
        self.shared.budget.begin();
        self.shared.effects_in_call.store(0, Ordering::Relaxed);
        CallGuard { shared: &self.shared, _gate: gate }
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
        let _call = self.enter().await;
        let _read_only = ReadOnlyCall::enter(&self.shared, read_only);
        let convert = |e: mlua::Error| VmError::Convert(e.to_string());
        let arg = json_to_lua(&self.lua, &input).map_err(convert)?;
        let ctx = self.ctx_table(ctx).map_err(convert)?;
        let out: LuaValue = run.call_async((arg, ctx)).await.map_err(|e| script(&e))?;
        lua_to_json(&out).map_err(|e| VmError::Convert(e.to_string()))
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
        let _call = self.enter().await;
        let convert = |e: mlua::Error| VmError::Convert(e.to_string());
        let args = (
            json_to_lua(&self.lua, key).map_err(convert)?,
            field.to_owned(),
            json_to_lua(&self.lua, value).map_err(convert)?,
            self.ctx_table(ctx).map_err(convert)?,
        );
        write.call_async::<()>(args).await.map_err(|e| script(&e))
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
        let _call = self.enter().await;
        let arg = json_to_lua(&self.lua, &payload).map_err(|e| VmError::Convert(e.to_string()))?;
        if track {
            self.shared.tracker.lock().begin(false);
        }
        let result = handler.call_async::<LuaValue>(arg).await;
        let deps = if track { self.shared.tracker.lock().end() } else { BTreeSet::new() };
        let out = result.map_err(|e| script(&e))?;
        Ok((lua_to_json(&out).map_err(|e| VmError::Convert(e.to_string()))?, deps))
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

    /// The app is going away: whatever is running in it stops at its next tick or host call (a host
    /// call it is waiting on is abandoned), and nothing new runs. Call this before dropping the last `Arc`,
    /// so a runaway call cannot keep the VM alive after an uninstall.
    pub fn retire(&self) {
        self.shared.budget.retirement().retire();
    }

    /// The app was unloaded or replaced: nothing it is still running may change the world.
    pub fn is_retired(&self) -> bool {
        self.shared.budget.retirement().is_retired()
    }

    /// Unload: release every Rust-held reference to Lua values, then the VM. Returns the heap size freed.
    pub fn unload(self) -> usize {
        let used = self.lua.used_memory();
        self.shared.release();
        used
    }
}
