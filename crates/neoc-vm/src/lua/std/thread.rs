//! Worker VMs — `std:thread`.
//!
//! Spawn a fresh Lua engine that runs on the same tokio runtime as the
//! parent, with full module access. Modeled after Node's `worker_threads`:
//! pass *source* (or a file) plus JSON-encodable args, talk to the worker
//! through a bidirectional `Channel` exposed as `thread.parent`.
//!
//! ## Spawn-time shared state (Sendable protocol)
//!
//! `thread.spawn` accepts an optional `shared = {…}` table. Every value
//! in the table is validated at spawn time against the set of known
//! Sendable types — non-conforming values are rejected with a clear
//! error. Workers receive the reconstructed table as `thread.shared`,
//! each entry wrapping the same `Arc<Inner>` as the parent.
//!
//! ```lua
//! local thread  = require("std:thread")
//! local Counter = require("std:collections").Counter
//!
//! local hits = Counter.new()
//! local w = thread.spawn({
//!     source = [[
//!         local shared = (require("std:thread")).shared
//!         shared.hits:increment()
//!     ]],
//!     shared = { hits = hits },
//! })
//! w:await()
//! assert(hits:get() == 1)
//! ```
//!
//! Closures can't move between VMs (Lua functions are tied to their
//! originating VM's bytecode + upvalues), so workers always start from a
//! source string. Messages and args travel as `serde_json::Value`.
//!
//! Forgotten workers are aborted on `WorkerHandle` drop so they don't leak
//! the tokio task indefinitely.

use std::any::Any;
use std::sync::Arc;

use mlua::{
    AnyUserData, FromLua, IntoLua, Lua, LuaSerdeExt, Table, UserData, UserDataMethods, Value,
};
use tokio::sync::{Mutex, mpsc};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, load_source, make_engine, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:thread` surface through the facade.
///
/// `spawn` is set verbatim with [`crate::lua::facade::Module::set`]: it carries
/// its own `#[instrument(name = "thread.spawn", target = "lua::lib::thread")]`
/// attribute and returns a `WorkerHandle` on success, so routing through the
/// facade would add a second span. `available_parallelism` carried no span and
/// returns the core count directly. `parent` / `shared` are dynamically
/// injected at spawn time (nil at the top level), so they are recorded as
/// constants via [`crate::lua::facade::Module::const_`]. The `WorkerHandle` and
/// `Channel` userdata keep their own `impl UserData` and are recorded here.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:thread")?;

    m.set("spawn", lua.create_function(spawn)?)?;
    m.record_fn(
        "spawn",
        &[("opts", "LuaValue")],
        "Result<WorkerHandle, string>",
        "Spawn a new Lua VM as a worker on the same Tokio runtime, given `{ source | file, name?, args?, shared? }`.",
    );

    m.set(
        "available_parallelism",
        lua.create_function(|_, _: ()| {
            Ok(std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1))
        })?,
    )?;
    m.record_fn(
        "available_parallelism",
        &[],
        "number",
        "Return the number of logical CPU cores available to the process. At least 1.",
    );

    // Overwritten by `run_worker` for actual workers; nil at the top level
    // so user code can branch on `if lib.thread.parent then …`.
    m.set("parent", Value::Nil)?;
    // `shared` is set to the reconstructed shared table in workers that
    // received `shared = {…}` at spawn time; nil at the top level and in
    // workers spawned without a shared table.
    m.set("shared", Value::Nil)?;

    m.const_(
        "parent",
        "LuaValue",
        "The `Channel` to the parent VM inside a spawned worker; nil at the top level.",
    )
    .const_(
        "shared",
        "LuaValue",
        "The reconstructed table of Sendable values passed via `shared` at spawn time; nil otherwise.",
    );

    m.userdata("WorkerHandle", |w| {
        w.method(
            "send",
            &[("value", "LuaValue")],
            "()",
            "Send a JSON-serialisable value to the worker; errors if the worker has exited.",
        )
        .method(
            "receive",
            &[],
            "Result<LuaValue, string>",
            "Wait for a message from the worker. Returns `(nil, error)` once the worker has exited.",
        )
        .method(
            "await",
            &[],
            "Result<LuaValue, string>",
            "Wait for the worker to finish and return its result, or `(nil, error)` on failure.",
        )
        .method(
            "terminate",
            &[],
            "()",
            "Abort the worker immediately. A subsequent `:await()` returns `(nil, \"thread.await: terminated\")`.",
        );
    });

    m.userdata("Channel", |c| {
        c.method(
            "send",
            &[("value", "LuaValue")],
            "()",
            "Send a JSON-serialisable value to the parent's message queue.",
        )
        .method(
            "receive",
            &[],
            "Result<LuaValue, string>",
            "Wait for a message from the parent. Returns `(nil, error)` once the parent has exited.",
        );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Sendable protocol — spawn-time validation for cross-engine state
// ---------------------------------------------------------------------------

/// The Sendable protocol.
///
/// Conforming types are safe to transfer across engine boundaries.
/// No string key. No global registry. The spawn-time check walks the
/// `shared` table and asks each value: "are you Sendable?" If yes,
/// extract the Arc. If no, spawn fails with a clear error.
pub trait Sendable: UserData + IntoLua + Sized + 'static {
    /// Extract the inner Arc for handoff. Cheap (Arc::clone).
    fn as_shared(&self) -> Arc<dyn Any + Send + Sync>;

    /// Reconstruct a fresh userdata wrapping the same Arc,
    /// bound to a different engine's Lua state.
    fn from_shared(arc: &Arc<dyn Any + Send + Sync>) -> Option<Self>;
}

// ---------------------------------------------------------------------------
// Shared-entry extraction machinery
// ---------------------------------------------------------------------------

/// A validated shared entry ready for cross-engine transfer. Carries the
/// erased Arc and a monomorphised rewrap function pointer so the target
/// engine can reconstruct the typed userdata without a runtime type
/// match.
struct SharedEntry {
    key: String,
    arc: Arc<dyn Any + Send + Sync>,
    rewrap: fn(&Arc<dyn Any + Send + Sync>, &Lua) -> mlua::Result<Value>,
}

// SharedEntry crosses into a tokio::spawn task — static assert Send.
const _: fn() = || {
    fn must_be_send<T: Send>() {}
    must_be_send::<SharedEntry>();
};

impl SharedEntry {
    /// Reconstruct this entry as a userdata in the target Lua engine.
    fn reconstruct(&self, lua: &Lua) -> mlua::Result<Value> {
        (self.rewrap)(&self.arc, lua)
    }
}

/// One entry in the static dispatch table. Carries a human-readable
/// label (for error messages) and a try-extract function pointer.
struct SendableType {
    label: &'static str,
    try_extract: fn(&str, &AnyUserData) -> Option<SharedEntry>,
}

/// Try to extract a `SharedEntry` from `ud` assuming it's a `T`.
fn try_extract_as<T: Sendable>(key: &str, ud: &AnyUserData) -> Option<SharedEntry> {
    if !ud.is::<T>() {
        return None;
    }
    let h = ud.borrow::<T>().ok()?;
    Some(SharedEntry {
        key: key.to_string(),
        arc: h.as_shared(),
        rewrap: |arc, lua| {
            T::from_shared(arc)
                .ok_or_else(|| {
                    mlua::Error::external(
                        "thread.spawn: shared entry held a foreign Arc \
                         (this should be unreachable)",
                    )
                })?
                .into_lua(lua)
        },
    })
}

/// Known Sendable types. Adding a new one: implement `Sendable` for it
/// in its module, then add an entry here.
static SENDABLE_TYPES: &[SendableType] = &[
    SendableType {
        label: "Map",
        try_extract: try_extract_as::<crate::lua::std::collections::Map>,
    },
    SendableType {
        label: "Counter",
        try_extract: try_extract_as::<crate::lua::std::collections::Counter>,
    },
    SendableType {
        label: "TcpListener",
        try_extract: try_extract_as::<crate::lua::std::net::Listener>,
    },
];

/// Comma-joined list of Sendable type labels for error messages.
fn sendable_type_labels() -> String {
    SENDABLE_TYPES
        .iter()
        .map(|t| t.label)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate and extract all entries from a `shared = {…}` table.
fn extract_shared(shared: &Table) -> mlua::Result<Vec<SharedEntry>> {
    let mut entries = Vec::new();
    for pair in shared.pairs::<String, Value>() {
        let (key, value) = pair?;
        match value {
            Value::UserData(ref ud) => {
                let entry = SENDABLE_TYPES
                    .iter()
                    .find_map(|t| (t.try_extract)(&key, ud));
                match entry {
                    Some(e) => entries.push(e),
                    None => {
                        let type_label = ud
                            .metatable()
                            .ok()
                            .and_then(|mt| mt.get::<String>("__name").ok())
                            .unwrap_or_else(|| "<userdata>".to_string());
                        return Err(mlua::Error::external(format!(
                            "shared.{key} is '{type_label}', which is not Sendable. \
                             Use args for by-value transfer, or use a Sendable type \
                             ({}).",
                            sendable_type_labels(),
                        )));
                    }
                }
            }
            other => {
                return Err(mlua::Error::external(format!(
                    "shared.{key} is '{}', which is not Sendable. \
                     Only Sendable userdata can be passed in shared. \
                     Use args for by-value transfer.",
                    other.type_name(),
                )));
            }
        }
    }
    Ok(entries)
}

/// Reconstruct all shared entries as a table in the target Lua engine.
fn reconstruct_shared(lua: &Lua, entries: &[SharedEntry]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for entry in entries {
        t.set(entry.key.as_str(), entry.reconstruct(lua)?)?;
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The validated inputs `spawn` needs once option parsing succeeds.
struct SpawnConfig {
    source: String,
    name: String,
    args: serde_json::Value,
    shared: Vec<SharedEntry>,
    runtime: tokio::runtime::Handle,
}

/// Parse and validate every `spawn` option, failing fast with a stringified
/// error the caller maps under `thread.spawn`.
fn prepare_spawn(opts: &Table) -> Result<SpawnConfig, String> {
    let (source, default_name) = read_source(opts)?;
    let name = opts
        .get::<Option<String>>("name")
        .ok()
        .flatten()
        .unwrap_or(default_name);
    let args = read_args(opts)?;
    let shared = read_shared(opts)?;
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| "no tokio runtime in scope".to_string())?;
    Ok(SpawnConfig {
        source,
        name,
        args,
        shared,
        runtime,
    })
}

#[instrument(name = "thread.spawn", target = "lua::lib::thread", skip_all)]
fn spawn(lua: &Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let SpawnConfig {
        source,
        name,
        args,
        shared,
        runtime,
    } = match prepare_spawn(&opts) {
        Ok(c) => c,
        Err(e) => return from_err(lua, "thread.spawn", e),
    };

    // Two unbounded mpscs: parent→worker and worker→parent. The worker's
    // Channel sees the inverse pair.
    let (p2w_tx, p2w_rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let (w2p_tx, w2p_rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let worker_channel = Channel {
        tx: w2p_tx,
        rx: Arc::new(Mutex::new(p2w_rx)),
    };

    let join = runtime.spawn(run_worker(name, source, args, worker_channel, shared));
    let abort = join.abort_handle();

    ok(
        lua,
        WorkerHandle {
            inner: Arc::new(WorkerInner {
                tx: p2w_tx,
                rx: Mutex::new(w2p_rx),
                join: Mutex::new(Some(join)),
                abort,
            }),
        },
    )
}

/// Read an optional typed field from the spawn `opts` table, stringifying any
/// conversion error. Shared by the `read_*` option readers.
fn opt<T: FromLua>(opts: &Table, key: &str) -> Result<Option<T>, String> {
    opts.get::<Option<T>>(key).map_err(|e| e.to_string())
}

/// Returns `(source, default_name)`. The default name is the file path when
/// `file` is used so stack traces and `set_name` output point at it; for
/// inline `source` it falls back to a generic label.
fn read_source(opts: &Table) -> Result<(String, String), String> {
    if let Some(s) = opt::<String>(opts, "source")? {
        return Ok((s, "thread:worker".to_string()));
    }
    if let Some(path) = opt::<String>(opts, "file")? {
        let source = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
        return Ok((source, path));
    }
    Err("opts must specify either `source` or `file`".into())
}

/// `args` is passed as a *single* value (any JSON-encodable Lua value) and
/// arrives in the worker as the first vararg, matching Node's `workerData`
/// convention. Pass a table for "named" args.
fn read_args(opts: &Table) -> Result<serde_json::Value, String> {
    match opt::<Value>(opts, "args")? {
        Some(v) => serde_json::to_value(&v)
            .map_err(mlua::Error::external)
            .map_err(|e| e.to_string()),
        None => Ok(serde_json::Value::Null),
    }
}

/// Read the optional `shared = {…}` table from spawn options.
/// Validates that every value is a Sendable userdata at spawn time.
fn read_shared(opts: &Table) -> Result<Vec<SharedEntry>, String> {
    match opt::<Table>(opts, "shared")? {
        Some(t) => extract_shared(&t).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

async fn run_worker(
    name: String,
    source: String,
    args: serde_json::Value,
    parent_channel: Channel,
    shared: Vec<SharedEntry>,
) -> mlua::Result<serde_json::Value> {
    let lua = make_engine()?;

    // Inject `parent` on the worker's `std:thread` module table so the
    // worker can talk back via `(require("std:thread")).parent`. We pull
    // the same Table the worker's require would hand out, then set the
    // field on it — every subsequent require call returns the same handle.
    let thread: Table = lua.load(r#"return require("std:thread")"#).eval()?;
    thread.set("parent", parent_channel)?;

    // Reconstruct shared entries in the worker's Lua state. Each Sendable
    // value is rebuilt as a fresh userdata wrapping the same Arc<Inner>,
    // then exposed via `thread.shared`.
    if !shared.is_empty() {
        let shared_table = reconstruct_shared(&lua, &shared)?;
        thread.set("shared", shared_table)?;
    }

    let arg_value = lua.to_value(&args)?;
    let result: Value = load_source(&lua, &source)
        .set_name(&name)
        .call_async(arg_value)
        .await?;
    serde_json::to_value(&result).map_err(mlua::Error::external)
}

// ---------------------------------------------------------------------------
// WorkerHandle (parent's view)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct WorkerHandle {
    inner: Arc<WorkerInner>,
}

struct WorkerInner {
    tx: mpsc::UnboundedSender<serde_json::Value>,
    rx: Mutex<mpsc::UnboundedReceiver<serde_json::Value>>,
    join: Mutex<Option<tokio::task::JoinHandle<mlua::Result<serde_json::Value>>>>,
    abort: tokio::task::AbortHandle,
}

impl Drop for WorkerInner {
    fn drop(&mut self) {
        // Best-effort cleanup so a worker handle going out of scope without
        // an explicit await/terminate doesn't leave the task running.
        self.abort.abort();
    }
}

impl UserData for WorkerHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("send", |_, this, v: Value| {
            send_json(&this.inner.tx, v, "thread.send: worker has exited")
        });

        methods.add_async_method("receive", |lua, this, _: ()| {
            let inner = this.inner.clone();
            async move {
                let received = inner.rx.lock().await.recv().await;
                handle_recv_result(&lua, received, "thread.receive: worker has exited")
            }
        });

        methods.add_async_method("await", |lua, this, _: ()| {
            let inner = this.inner.clone();
            async move {
                let join = {
                    let mut guard = inner.join.lock().await;
                    match guard.take() {
                        Some(j) => j,
                        None => return from_err(&lua, "thread.await", "already awaited"),
                    }
                };
                match join.await {
                    Ok(Ok(json)) => ok(&lua, lua.to_value(&json)?),
                    Ok(Err(e)) => from_err(&lua, "thread.await", e),
                    Err(e) if e.is_cancelled() => from_err(&lua, "thread.await", "terminated"),
                    Err(e) => from_err(&lua, "thread.await", e),
                }
            }
        });

        methods.add_method("terminate", |_, this, _: ()| {
            this.inner.abort.abort();
            Ok(())
        });
    }
}

// ---------------------------------------------------------------------------
// Channel (the worker's view of `lib.thread.parent`)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Channel {
    tx: mpsc::UnboundedSender<serde_json::Value>,
    rx: Arc<Mutex<mpsc::UnboundedReceiver<serde_json::Value>>>,
}

impl UserData for Channel {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("send", |_, this, v: Value| {
            send_json(&this.tx, v, "channel.send: peer has exited")
        });

        methods.add_async_method("receive", |lua, this, _: ()| {
            let rx = this.rx.clone();
            async move {
                let received = rx.lock().await.recv().await;
                handle_recv_result(&lua, received, "channel.receive: peer has exited")
            }
        });
    }
}

/// Encode `v` and push onto `tx`. `err_msg` is wrapped in the same
/// `mlua::Error::external` envelope used by every direct caller.
fn send_json(
    tx: &mpsc::UnboundedSender<serde_json::Value>,
    v: Value,
    err_msg: &'static str,
) -> mlua::Result<()> {
    let json = serde_json::to_value(&v).map_err(mlua::Error::external)?;
    tx.send(json).map_err(|_| mlua::Error::external(err_msg))?;
    Ok(())
}

/// Convert the result of an `UnboundedReceiver::recv().await` into the
/// `(value, error)` tuple shape every Lua-facing receive surface returns.
/// `Some(json)` → value, `None` (channel closed) → typed error string.
fn handle_recv_result(
    lua: &Lua,
    received: Option<serde_json::Value>,
    err_msg: &'static str,
) -> mlua::Result<(Value, Value)> {
    match received {
        Some(json) => Ok((lua.to_value(&json)?, Value::Nil)),
        None => Ok((Value::Nil, Value::String(lua.create_string(err_msg)?))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::lua;

    /// `thread.available_parallelism()` returns a positive integer.
    #[test]
    fn available_parallelism_returns_positive() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: i64 = e
                .load(
                    r#"
                    local thread = require("std:thread")
                    return thread.available_parallelism()
                "#,
                )
                .eval_async()
                .await
                .expect("available_parallelism");
            assert!(result >= 1, "should return at least 1, got: {result}");
        });
    }

    /// Spawn with `shared = { m = Map, c = Counter }` — worker sees the
    /// same Arc<Inner>: mutations in the worker are visible to the parent.
    #[test]
    fn shared_map_and_counter_across_spawn() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let (worker_saw, worker_wrote, counter_val): (String, String, i64) = e
                .load(
                    r#"
                    local thread      = require("std:thread")
                    local Map         = require("std:collections").Map
                    local Counter     = require("std:collections").Counter

                    local m = Map.new()
                    local c = Counter.new()
                    m:set("before", "parent")

                    local w = thread.spawn({
                        source = [[
                            local thread = require("std:thread")
                            local shared = thread.shared
                            shared.m:set("from_worker", "hello")
                            shared.c:increment()
                            shared.c:increment()
                            shared.c:add(5)
                            return shared.m:get("before")
                        ]],
                        shared = { m = m, c = c },
                    })

                    local worker_saw = w:await()
                    local worker_wrote = m:get("from_worker")
                    local counter_val = c:get()

                    return worker_saw, worker_wrote, counter_val
                "#,
                )
                .eval_async()
                .await
                .expect("shared spawn");
            assert_eq!(worker_saw, "parent", "worker should see parent's map value");
            assert_eq!(
                worker_wrote, "hello",
                "parent should see worker's map write"
            );
            assert_eq!(counter_val, 7, "counter should reflect worker ops");
        });
    }

    /// Passing a non-Sendable userdata in shared errors at spawn time
    /// with a clear message.
    #[test]
    fn non_sendable_rejected_at_spawn() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: bool = e
                .load(
                    r#"
                    local thread = require("std:thread")
                    local json   = require("lib:json")

                    local w, err = thread.spawn({
                        source = "return 1",
                        shared = { bad = json.object() },
                    })
                    return w == nil and tostring(err):find("not Sendable") ~= nil
                "#,
                )
                .eval_async()
                .await
                .expect("non-sendable rejection");
            assert!(result, "non-Sendable userdata should be rejected");
        });
    }

    /// Passing a plain Lua table in shared errors at spawn time.
    #[test]
    fn plain_table_rejected_at_spawn() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: bool = e
                .load(
                    r#"
                    local thread = require("std:thread")

                    local w, err = thread.spawn({
                        source = "return 1",
                        shared = { t = {} },
                    })
                    return w == nil and tostring(err):find("not Sendable") ~= nil
                "#,
                )
                .eval_async()
                .await
                .expect("table rejection");
            assert!(result, "plain table should be rejected");
        });
    }

    /// Empty `shared = {}` is valid — worker sees nil or empty thread.shared.
    #[test]
    fn empty_shared_is_valid() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: bool = e
                .load(
                    r#"
                    local thread = require("std:thread")

                    local w = thread.spawn({
                        source = [[
                            local shared = (require("std:thread")).shared
                            return shared == nil
                        ]],
                        shared = {},
                    })
                    return w:await() == true
                "#,
                )
                .eval_async()
                .await
                .expect("empty shared");
            assert!(result, "empty shared should leave thread.shared as nil");
        });
    }

    /// Omitting `shared` entirely is valid — backward compatible.
    #[test]
    fn no_shared_backward_compat() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: i64 = e
                .load(
                    r#"
                    local thread = require("std:thread")
                    local w = thread.spawn({
                        source = "return 42",
                    })
                    return w:await()
                "#,
                )
                .eval_async()
                .await
                .expect("no shared");
            assert_eq!(result, 42);
        });
    }

    /// Same Sendable passed to two workers — both see the same Arc.
    #[test]
    fn shared_to_multiple_workers() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: i64 = e
                .load(
                    r#"
                    local thread  = require("std:thread")
                    local Counter = require("std:collections").Counter

                    local c = Counter.new()

                    local w1 = thread.spawn({
                        source = [[
                            local shared = (require("std:thread")).shared
                            shared.c:increment()
                        ]],
                        shared = { c = c },
                    })
                    local w2 = thread.spawn({
                        source = [[
                            local shared = (require("std:thread")).shared
                            shared.c:increment()
                        ]],
                        shared = { c = c },
                    })

                    w1:await()
                    w2:await()
                    return c:get()
                "#,
                )
                .eval_async()
                .await
                .expect("multi-worker shared");
            assert_eq!(result, 2, "both workers should increment the same counter");
        });
    }

    /// ListenerHandle shared via spawn-time handoff — worker can call
    /// `:addr()` on the same socket.
    #[test]
    fn shared_listener_across_spawn() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let result: String = e
                .load(
                    r#"
                    local thread       = require("std:thread")
                    local TcpListener  = require("std:net").TcpListener

                    local listener = TcpListener.bind("127.0.0.1:0")
                    local addr = listener:addr()

                    local w = thread.spawn({
                        source = [[
                            local shared = (require("std:thread")).shared
                            return shared.listener:addr()
                        ]],
                        shared = { listener = listener },
                    })

                    local worker_addr = w:await()
                    -- Both should report the same bound address.
                    if worker_addr ~= addr then
                        error("addr mismatch: parent=" .. addr .. " worker=" .. worker_addr)
                    end
                    return worker_addr
                "#,
                )
                .eval_async()
                .await
                .expect("shared listener");
            assert!(
                result.starts_with("127.0.0.1:"),
                "worker should see the listener's address, got: {result}"
            );
        });
    }

    /// Passing a primitive (number) in shared errors at spawn time.
    #[test]
    fn primitive_rejected_at_spawn() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            let err_msg: String = e
                .load(
                    r#"
                    local thread = require("std:thread")

                    local w, err = thread.spawn({
                        source = "return 1",
                        shared = { n = 42 },
                    })
                    return tostring(err)
                "#,
                )
                .eval_async()
                .await
                .expect("primitive rejection");
            assert!(
                err_msg.contains("not Sendable"),
                "number should be rejected as non-Sendable, got: {err_msg}"
            );
            assert!(
                err_msg.contains("shared.n"),
                "error should name the offending key, got: {err_msg}"
            );
        });
    }

    /// Worker can forward shared state to its own child (DAG forwarding).
    #[test]
    fn dag_forwarding() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e = lua::sandbox::make_engine().expect("engine");

            // The inner worker source is a separate string to avoid
            // nested [[ ]] delimiters, which Lua doesn't support.
            let result: i64 = e
                .load(
                    r#"
                    local thread  = require("std:thread")
                    local Counter = require("std:collections").Counter

                    local c = Counter.new()

                    local CHILD_SOURCE = "local shared = (require('std:thread')).shared; shared.c:add(10)"

                    local w = thread.spawn({
                        source = [[
                            local thread = require("std:thread")
                            local shared = thread.shared
                            local parent = thread.parent

                            local child_src = parent:receive()
                            local child = thread.spawn({
                                source = child_src,
                                shared = { c = shared.c },
                            })
                            child:await()
                            shared.c:add(1)
                        ]],
                        shared = { c = c },
                    })

                    w:send(CHILD_SOURCE)
                    w:await()
                    return c:get()
                "#,
                )
                .eval_async()
                .await
                .expect("DAG forwarding");
            assert_eq!(
                result, 11,
                "child and parent worker both modify the same counter"
            );
        });
    }

    /// Spawn `opts` (a Lua table expression over the `bytecode` and `path`
    /// globals) and assert the worker refused to load its chunk.
    fn assert_spawn_rejects_bytecode(opts: &str) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("worker.lua");
            let bytecode = lua::sandbox::ascii_bytecode();
            std::fs::write(&path, &bytecode).unwrap();

            let e = lua::sandbox::make_engine().expect("engine");
            e.globals().set("bytecode", bytecode).unwrap();
            e.globals().set("path", path.to_str().unwrap()).unwrap();

            let (returned, err): (bool, String) = e
                .load(format!(
                    r#"
                    local thread = require("std:thread")
                    local w = assert(thread.spawn({opts}))
                    local v, err = w:await()
                    return v ~= nil, tostring(err)
                "#
                ))
                .eval_async()
                .await
                .expect("spawn");
            assert!(!returned, "{opts}: bytecode must not execute");
            assert!(err.contains("binary"), "{opts}: got {err}");
        });
    }

    /// Spawn rejects LuaJIT bytecode passed inline as `source`.
    #[test]
    fn spawn_source_rejects_bytecode() {
        assert_spawn_rejects_bytecode("{ source = bytecode }");
    }

    /// Spawn rejects a `file` whose contents are LuaJIT bytecode.
    #[test]
    fn spawn_file_rejects_bytecode() {
        assert_spawn_rejects_bytecode("{ file = path }");
    }
}
