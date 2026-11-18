//! Driven adapter: sandboxed Luau (mlua) behind the `AppRuntime` port.
//!
//! One sandboxed Luau VM per app, with a host-provided capability table (`app`), a memory limit,
//! a CPU budget enforced from the VM interrupt, async host calls and Rust -> Luau event dispatch.

pub mod reload;
mod runtime;

pub use runtime::LuauRuntime;

use agentos_application::ports::AppData;
use mlua::{serde::SerializeOptions, Function, Lua, LuaSerdeExt, Table, Value as LuaValue, VmState};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Debug, thiserror::Error)]
pub enum VmError {
    #[error("app `{app}` failed to load: {source}")]
    Load { app: String, source: mlua::Error },
    #[error("{0}")]
    Call(mlua::Error),
    #[error("no such tool `{0}`")]
    NoSuchTool(String),
    #[error("conversion: {0}")]
    Convert(#[from] mlua::Error),
}
pub type Result<T> = std::result::Result<T, VmError>;

#[derive(Clone, Debug)]
pub struct Limits {
    /// Hard cap on the Luau heap (allocations past this raise a Lua `MemoryError`).
    pub memory_bytes: usize,
    /// Max continuous CPU time between host awaits (a `while true do end` is killed after this).
    pub cpu_slice: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self { memory_bytes: 16 * 1024 * 1024, cpu_slice: Duration::from_millis(250) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

struct ToolDecl {
    info: ToolInfo,
    handler: Function,
}

#[derive(Default)]
struct Shared {
    tools: Mutex<Vec<ToolDecl>>,
    handlers: Mutex<HashMap<String, Function>>,
    notifications: Mutex<Vec<Value>>,
    state: Mutex<HashMap<String, Value>>,
}

/// Monotonic "last time the VM was handed control" marker used by the interrupt callback.
#[derive(Clone)]
struct Slice {
    base: Instant,
    started_ns: Arc<AtomicU64>,
    killed: Arc<AtomicBool>,
}
impl Slice {
    fn new() -> Self {
        Self { base: Instant::now(), started_ns: Arc::new(AtomicU64::new(0)), killed: Arc::default() }
    }
    fn now_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    fn reset(&self) {
        self.started_ns.store(self.now_ns(), Ordering::Relaxed);
    }
    fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.now_ns().saturating_sub(self.started_ns.load(Ordering::Relaxed)))
    }
}

pub struct AppVm {
    pub app_id: String,
    lua: Lua,
    shared: Arc<Shared>,
    slice: Slice,
}

fn to_lua(lua: &Lua, v: &Value) -> mlua::Result<LuaValue> {
    // JSON null -> Lua nil (not the truthy `null` lightuserdata)
    lua.to_value_with(v, SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false))
}

impl AppVm {
    /// Create the VM, install the capability table, then run the app's main chunk (loaded from bytes).
    pub async fn load(app_id: &str, source: &[u8], db: Arc<dyn AppData>, limits: Limits) -> Result<Self> {
        let lua = Lua::new();
        let shared = Arc::new(Shared::default());
        let slice = Slice::new();

        // mlua registers a filesystem `require` for Luau: remove it. (The real host provides its own
        // `require` that resolves only files from the app's verified bundle.)
        lua.globals().set("require", LuaValue::Nil)?;
        install_api(&lua, app_id, &shared, db, slice.clone())?;

        // Interrupt: runs at function calls and loop back-edges. Returning Err aborts the running
        // chunk. `killed` makes it sticky so a `pcall` can't swallow the abort in a loop.
        {
            let slice = slice.clone();
            let max = limits.cpu_slice;
            lua.set_interrupt(move |_| {
                if slice.killed.load(Ordering::Relaxed) || slice.elapsed() > max {
                    slice.killed.store(true, Ordering::Relaxed);
                    Err(mlua::Error::runtime("cpu budget exceeded"))
                } else {
                    Ok(VmState::Continue)
                }
            });
        }
        // Freeze the globals / builtins and give this app its own global table.
        lua.sandbox(true)?;
        lua.set_memory_limit(limits.memory_bytes)?;

        let vm = Self { app_id: app_id.into(), lua, shared, slice };
        vm.begin_call();
        vm.lua
            .load(source)
            .set_name(format!("={app_id}/main.luau"))
            .exec_async()
            .await
            .map_err(|source| VmError::Load { app: app_id.into(), source })?;
        Ok(vm)
    }

    fn begin_call(&self) {
        self.slice.killed.store(false, Ordering::Relaxed);
        self.slice.reset();
    }

    /// Dispatch an event to `app.on(name, fn)` with a JSON payload; returns the handler's JSON result.
    pub async fn emit(&self, event: &str, payload: Value) -> Result<Value> {
        let f = self.shared.handlers.lock().get(event).cloned();
        let Some(f) = f else { return Ok(Value::Null) };
        self.begin_call();
        let arg = to_lua(&self.lua, &payload)?;
        let out: LuaValue = f.call_async(arg).await.map_err(VmError::Call)?;
        Ok(self.lua.from_value(out)?)
    }

    pub fn tools(&self) -> Vec<ToolInfo> {
        self.shared.tools.lock().iter().map(|t| t.info.clone()).collect()
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        let f = self.shared.tools.lock().iter().find(|t| t.info.name == name).map(|t| t.handler.clone());
        let f = f.ok_or_else(|| VmError::NoSuchTool(name.to_owned()))?;
        self.begin_call();
        let out: LuaValue = f.call_async(to_lua(&self.lua, &args)?).await.map_err(VmError::Call)?;
        Ok(self.lua.from_value(out)?)
    }

    pub fn take_notifications(&self) -> Vec<Value> {
        std::mem::take(&mut *self.shared.notifications.lock())
    }

    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }

    pub fn gc(&self) {
        let _ = self.lua.gc_collect();
    }

    /// Unload: drop every Rust-held reference to Luau values, then the VM. Returns bytes freed.
    pub fn unload(self) -> usize {
        let used = self.lua.used_memory();
        self.shared.tools.lock().clear();
        self.shared.handlers.lock().clear();
        drop(self.lua); // last strong ref => lua_close
        used
    }
}

/// Build the `app` capability table: the only way a Luau app reaches the host.
fn install_api(
    lua: &Lua,
    app_id: &str,
    shared: &Arc<Shared>,
    db: Arc<dyn AppData>,
    slice: Slice,
) -> mlua::Result<()> {
    let app = lua.create_table()?;

    // app.on(event, fn)
    let s = shared.clone();
    app.set(
        "on",
        lua.create_function(move |_, (name, f): (String, Function)| {
            s.handlers.lock().insert(name, f);
            Ok(())
        })?,
    )?;

    // app.id, app.log
    app.set("id", app_id)?;
    let id = app_id.to_string();
    app.set(
        "log",
        lua.create_function(move |_, msg: String| {
            tracing::info!(app = %id, "{msg}");
            Ok(())
        })?,
    )?;

    // app.notify{ title=, body= }
    let s = shared.clone();
    app.set(
        "notify",
        lua.create_function(move |lua, t: LuaValue| {
            s.notifications.lock().push(lua.from_value(t)?);
            Ok(())
        })?,
    )?;

    app.set("tools", tools_table(lua, shared)?)?;
    app.set("state", state_table(lua, shared)?)?;
    app.set("data", data_table(lua, db, slice)?)?;

    lua.globals().set("app", app)?;
    Ok(())
}

/// `app.tools.register{ name=, description=, input_schema=, handler= }`
fn tools_table(lua: &Lua, shared: &Arc<Shared>) -> mlua::Result<Table> {
    let tools = lua.create_table()?;
    let s = shared.clone();
    tools.set(
        "register",
        lua.create_function(move |lua, t: Table| {
            let handler: Function = t.get("handler")?;
            let info = ToolInfo {
                name: t.get("name")?,
                description: t.get::<Option<String>>("description")?.unwrap_or_default(),
                input_schema: lua.from_value(t.get::<LuaValue>("input_schema")?)?,
            };
            let mut tools = s.tools.lock();
            tools.retain(|d| d.info.name != info.name);
            tools.push(ToolDecl { info, handler });
            Ok(())
        })?,
    )?;
    Ok(tools)
}

/// `app.state.get` / `app.state.set`: synchronous in-memory key/value.
fn state_table(lua: &Lua, shared: &Arc<Shared>) -> mlua::Result<Table> {
    let state = lua.create_table()?;
    let s = shared.clone();
    state.set(
        "get",
        lua.create_function(move |lua, k: String| match s.state.lock().get(&k) {
            Some(v) => to_lua(lua, v),
            None => Ok(LuaValue::Nil),
        })?,
    )?;
    let s = shared.clone();
    state.set(
        "set",
        lua.create_function(move |lua, (k, v): (String, LuaValue)| {
            s.state.lock().insert(k, lua.from_value(v)?);
            Ok(())
        })?,
    )?;
    Ok(state)
}

/// `app.data.query` / `app.data.exec`: ASYNC host functions; SQLite runs on tokio's blocking pool.
/// The CPU slice restarts when the host call returns (time spent awaiting isn't app CPU).
fn data_table(lua: &Lua, db: Arc<dyn AppData>, slice: Slice) -> mlua::Result<Table> {
    let data = lua.create_table()?;
    let (d, sl) = (db.clone(), slice.clone());
    data.set(
        "query",
        lua.create_async_function(move |lua, (sql, params): (String, Option<LuaValue>)| {
            let (d, sl) = (d.clone(), sl.clone());
            async move {
                let params = sql_params(&lua, params)?;
                let rows = d.query(sql, params).await.map_err(mlua::Error::external);
                sl.reset();
                to_lua(&lua, &Value::Array(rows?))
            }
        })?,
    )?;
    let (d, sl) = (db, slice);
    data.set(
        "exec",
        lua.create_async_function(move |lua, (sql, params): (String, Option<LuaValue>)| {
            let (d, sl) = (d.clone(), sl.clone());
            async move {
                let params = sql_params(&lua, params)?;
                let r = d.exec(sql, params).await.map_err(mlua::Error::external);
                sl.reset();
                let r = r?;
                to_lua(
                    &lua,
                    &serde_json::json!({"changes": r.changes, "last_insert_rowid": r.last_insert_rowid}),
                )
            }
        })?,
    )?;
    Ok(data)
}

fn sql_params(lua: &Lua, params: Option<LuaValue>) -> mlua::Result<Vec<Value>> {
    match params {
        Some(p) => lua.from_value(p),
        None => Ok(vec![]),
    }
}
