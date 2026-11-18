//! One sandboxed Luau VM per app.
//!
//! The VM is created, given the `app` capability table and a bundle-only `require`, sandboxed, limited
//! (memory, CPU slice per call, host-call timeout) and then runs the app's entry. After that the host
//! calls into it: tools, write-back callbacks, events. One call runs in a VM at a time (an app is
//! single-threaded by design); the VM lives as long as its `AppVm`.

use crate::{
    api,
    budget::Slice,
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
use mlua::{Function, Lua, Table, Value as LuaValue, VmState};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

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
    /// Hard cap on the Luau heap (allocations past this raise a Lua `MemoryError`).
    pub memory_bytes: usize,
    /// Max continuous CPU time between host awaits (a `while true do end` is killed after this).
    pub cpu_slice: Duration,
    /// Max wall-clock time of one host call (a database call that never returns).
    pub host_call_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: 16 * 1024 * 1024,
            cpu_slice: Duration::from_millis(250),
            host_call_timeout: Duration::from_secs(10),
        }
    }
}

/// How a VM is built.
#[derive(Clone)]
pub struct VmConfig {
    pub capabilities: Capabilities,
    pub limits: Limits,
    /// Luau source of host modules `require("@agentos/<name>")` can load besides `@agentos/app`
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
    pub slice: Slice,
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
    pub read_only: AtomicBool,
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

pub struct AppVm {
    pub app: AppId,
    lua: Lua,
    shared: Arc<Shared>,
    /// Serialises calls: one runs at a time, so `db:tx`, the CPU slice and state tracking are unambiguous.
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
        let lua = Lua::new();
        let slice = Slice::new();
        let shared = Arc::new(Shared {
            app: app.clone(),
            caps: capabilities,
            package,
            db,
            slice: slice.clone(),
            limits: limits.clone(),
            host_modules,
            roster,
            tracker: Arc::new(Mutex::new(Tracker::default())),
            tools: Mutex::default(),
            handlers: Mutex::default(),
            sources: Mutex::default(),
            writers: Mutex::default(),
            windows: Mutex::default(),
            commands: Mutex::default(),
            deep_links: Mutex::default(),
            decl: Mutex::default(),
            effects: Mutex::default(),
            modules: Mutex::default(),
            loading: Mutex::default(),
            app_table: Mutex::default(),
            fail: Mutex::default(),
            views: Mutex::default(),
            render_scope: Mutex::default(),
            view_effects: Mutex::default(),
            tx: Mutex::new(None),
            read_only: AtomicBool::new(false),
        });
        let load_err =
            |e: &mlua::Error| VmError::Load { app: app.to_string(), error: ScriptError::from_lua(e) };

        // mlua registers a filesystem `require` for Luau: replace it with the bundle-only one, and install
        // `app` BEFORE sandboxing (the sandbox freezes the globals).
        lua.globals().set("require", LuaValue::Nil).map_err(|e| load_err(&e))?;
        api::install(&lua, &shared).map_err(|e| load_err(&e))?;

        // Interrupt: runs at function calls and loop back-edges. Returning Err aborts the running chunk;
        // the kill is sticky so a `pcall` cannot swallow it.
        let max = limits.cpu_slice;
        lua.set_interrupt(move |_| {
            if slice.exceeded(max) {
                Err(mlua::Error::runtime("cpu budget exceeded"))
            } else {
                Ok(VmState::Continue)
            }
        });
        // Freeze the globals / builtins and give this app its own global table.
        lua.sandbox(true).map_err(|e| load_err(&e))?;
        lua.set_memory_limit(limits.memory_bytes).map_err(|e| load_err(&e))?;

        let vm = Self { app: app.clone(), lua, shared, gate: tokio::sync::Mutex::new(()) };
        let call = vm.enter().await;
        let entry = vm.shared.package.entry.clone();
        let source = vm
            .shared
            .package
            .files
            .get(&entry)
            .cloned()
            .ok_or_else(|| VmError::Convert(format!("the package has no entry `{entry}`")))?;
        vm.lua
            .load(source.as_slice())
            .set_name(vm.shared.chunk_name(&entry))
            .exec_async()
            .await
            .map_err(|e| load_err(&e))?;
        drop(call);
        Ok(vm)
    }

    /// Start a top-level call: wait for the previous one, fresh CPU slice, and discard a transaction a
    /// cancelled call left open (dropping it rolls it back and frees the database).
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }

    pub(crate) async fn enter_call(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.enter().await
    }

    async fn enter(&self) -> tokio::sync::MutexGuard<'_, ()> {
        let guard = self.gate.lock().await;
        self.shared.slice.begin();
        drop(self.shared.tx.lock().take());
        guard
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
    pub async fn call_tool(&self, name: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        let (schema, run, read_only) = {
            let tools = self.shared.tools.lock();
            let t = tools
                .iter()
                .find(|t| t.spec.name == name)
                .ok_or_else(|| VmError::NoSuchTool(name.to_owned()))?;
            (t.spec.input_schema.clone(), t.run.clone(), t.spec.read_only)
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

    /// Unload: release every Rust-held reference to Luau values, then the VM. Returns the heap size freed.
    pub fn unload(self) -> usize {
        let used = self.lua.used_memory();
        self.shared.release();
        used
    }
}
