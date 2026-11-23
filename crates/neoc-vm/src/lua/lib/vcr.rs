//! VCR recorder — wraps lib functions to capture or replay interactions.
//!
//! | Function          | behaviour                                          |
//! |-------------------|----------------------------------------------------|
//! | `lib.vcr.cassette({ path = "...", mode = "record" })` | records every call; writes cassette to disk   |
//! | `lib.vcr.cassette({ path = "...", mode = "replay" })` | returns stored results; real function not called |
//! | `lib.vcr.cassette({ mode = "off" })`                  | pass-through; zero overhead                   |
//!
//! Setting `NEOC_VCR=off` forces all constructors to return a
//! pass-through VCR regardless of the call site, making it safe to leave VCR
//! calls in production code.
//!
//! ```lua
//! local vcr = lib.vcr.on({ path = "cassettes/base64.json", mode = "record" })
//! local encode = vcr:wrap("base64.encode.hello", lib.base64.encode)
//! encode("hello")  -- recorded to disk immediately
//!
//! local vcr = lib.vcr.on({ path = "cassettes/base64.json", mode = "replay" })
//! local encode = vcr:wrap("base64.encode.hello", lib.base64.encode)
//! encode("anything")  -- returns the recorded result
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mlua::{Lua, LuaSerdeExt, MultiValue, Table};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::serde_guard::refuse_buffer;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:vcr` surface through the facade.
///
/// `cassette` is set verbatim with [`crate::lua::facade::Module::set`]: the
/// hand-written closure raises `mlua::Error` directly on an unknown mode and
/// carries no span of its own (the mode-specific constructors `passthrough` /
/// `recording` / `replay` open their own spans), so routing it through the
/// facade's `func` / `try_func` path would change its behaviour. Its signature
/// is recorded via
/// what `neoc_macros` reads from the implementation.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:vcr")?;

    /// Create a cassette in the given mode (`record` / `replay` / `off`). Returns a handle exposing `wrap` and `dump`. `NEOC_VCR=off` forces pass-through regardless of `opts.mode`.
    #[lua("LuaValue")]
    m.set(
        "cassette",
        lua.create_function(|lua, opts: Table| {
            if forced_off() {
                return passthrough(lua);
            }
            let mode: String = opts.get("mode")?;
            match mode.as_str() {
                "off" => passthrough(lua),
                "record" => {
                    let path: String = opts.get("path")?;
                    recording(lua, path)
                }
                "replay" => {
                    let path: String = opts.get("path")?;
                    replay(lua, &path)
                }
                other => Err(mlua::Error::runtime(format!(
                    "vcr: unknown mode \"{other}\""
                ))),
            }
        })?,
    )?;

    Ok(m)
}

fn forced_off() -> bool {
    std::env::var("NEOC_VCR").as_deref() == Ok("off")
}

// ---------------------------------------------------------------------------
// shared types
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    args: Vec<Json>,
    result: Vec<Json>,
}

// A tape serialises as just its `entries` map (the cassette-file shape replay
// reads back); `registered` is in-memory duplicate-key tracking only.
#[derive(Serialize)]
#[serde(transparent)]
struct Tape {
    #[serde(skip)]
    registered: BTreeSet<String>,
    entries: BTreeMap<String, Entry>,
}

impl Tape {
    fn register(&mut self, key: &str) -> bool {
        self.registered.insert(key.to_owned())
    }
    fn record(&mut self, key: String, entry: Entry) {
        self.entries.insert(key, entry);
    }
}

// ---------------------------------------------------------------------------
// off (pass-through)
// ---------------------------------------------------------------------------

/// Assemble a cassette table from a `wrap` implementation. Every mode exposes
/// the same surface — `wrap(target, key, fn)` plus a no-op `dump` — and differs
/// only in what `wrap` does per call.
fn build_cassette(lua: &Lua, wrap: mlua::Function) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("wrap", wrap)?;
    set_noop_dump(lua, &t)?;
    Ok(t)
}

/// The error every mode raises when the same `wrap` key is registered twice.
fn dup_key_err(key: &str) -> mlua::Error {
    mlua::Error::runtime(format!("vcr: duplicate key \"{key}\""))
}

#[instrument(name = "vcr.off", target = "lua::lib::vcr", skip_all)]
fn passthrough(lua: &Lua) -> mlua::Result<Table> {
    let wrap =
        lua.create_function(|_, (_, _key, f): (mlua::Value, String, mlua::Function)| Ok(f))?;
    build_cassette(lua, wrap)
}

// ---------------------------------------------------------------------------
// record
// ---------------------------------------------------------------------------

#[instrument(name = "vcr.flush", target = "lua::lib::vcr", skip_all)]
fn flush(tape: &Tape, path: &str) -> mlua::Result<()> {
    let json = serde_json::to_string_pretty(tape).map_err(mlua::Error::external)?;
    std::fs::write(path, json).map_err(mlua::Error::external)?;
    Ok(())
}

/// Run the wrapped function once and record the interaction: capture `args` and
/// the result as JSON, store them in the tape under `key`, then flush to `path`.
/// Returns the function's own result unchanged so the caller is transparent.
fn record_call(
    lua: &Lua,
    reg_key: &mlua::RegistryKey,
    tape: &Arc<Mutex<Tape>>,
    path: &str,
    key: &str,
    args: MultiValue,
) -> mlua::Result<MultiValue> {
    let json_args = to_json_all(&args)?;
    let f: mlua::Function = lua.registry_value(reg_key)?;
    let result: MultiValue = f.call(args)?;
    let json_result = to_json_all(&result)?;

    let mut locked = tape.lock();
    locked.record(
        key.to_owned(),
        Entry {
            args: json_args,
            result: json_result,
        },
    );
    flush(&locked, path)?;
    Ok(result)
}

#[instrument(name = "vcr.record", target = "lua::lib::vcr", skip_all)]
fn recording(lua: &Lua, path: String) -> mlua::Result<Table> {
    let tape: Arc<Mutex<Tape>> = Arc::new(Mutex::new(Tape {
        registered: BTreeSet::new(),
        entries: BTreeMap::new(),
    }));
    flush(&tape.lock(), &path)?;

    let wrap = {
        let tape = tape.clone();
        let path = path.clone();
        lua.create_function(
            move |lua, (_, key, f): (mlua::Value, String, mlua::Function)| {
                if !tape.lock().register(&key) {
                    return Err(dup_key_err(&key));
                }
                let reg_key = lua.create_registry_value(f)?;
                let tape = tape.clone();
                let path = path.clone();
                lua.create_function(move |lua, args: MultiValue| {
                    record_call(lua, &reg_key, &tape, &path, &key, args)
                })
            },
        )?
    };

    build_cassette(lua, wrap)
}

// ---------------------------------------------------------------------------
// replay
// ---------------------------------------------------------------------------

#[instrument(name = "vcr.replay", target = "lua::lib::vcr", skip_all)]
fn replay(lua: &Lua, path: &str) -> mlua::Result<Table> {
    let content = std::fs::read_to_string(path).map_err(mlua::Error::external)?;
    let cassette: BTreeMap<String, Entry> =
        serde_json::from_str(&content).map_err(mlua::Error::external)?;
    let cassette = Arc::new(cassette);
    let registered: Arc<Mutex<BTreeSet<String>>> = Arc::new(Mutex::new(BTreeSet::new()));

    let wrap = lua.create_function(
        move |lua, (_, key, _f): (mlua::Value, String, mlua::Function)| {
            if !registered.lock().insert(key.clone()) {
                return Err(dup_key_err(&key));
            }
            let entry = cassette.get(&key).cloned().ok_or_else(|| {
                mlua::Error::runtime(format!("vcr: no cassette entry for key \"{key}\""))
            })?;
            lua.create_function(move |lua, _args: MultiValue| {
                let values = entry
                    .result
                    .iter()
                    .map(|j| match j {
                        Json::Null => Ok(mlua::Value::Nil),
                        _ => lua.to_value(j),
                    })
                    .collect::<mlua::Result<Vec<_>>>()?;
                Ok(mlua::MultiValue::from_iter(values))
            })
        },
    )?;

    build_cassette(lua, wrap)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Register the no-op `dump` entry every cassette table exposes. All three
/// modes (pass-through, record, replay) flush eagerly, so `dump` has nothing to
/// do — it exists only so callers can call it unconditionally.
fn set_noop_dump(lua: &Lua, t: &Table) -> mlua::Result<()> {
    t.set("dump", lua.create_function(|_, _: mlua::Value| Ok(()))?)
}

/// `values` as JSON for the tape. A buffer raises (replay would hand back an array of numbers in its place); the
/// arguments are checked before the wrapped function runs, its results after.
fn to_json_all(values: &MultiValue) -> mlua::Result<Vec<Json>> {
    values
        .iter()
        .map(|v| {
            refuse_buffer(v, "vcr.record")?;
            Ok(to_json(v))
        })
        .collect()
}

fn to_json(v: &mlua::Value) -> Json {
    serde_json::to_value(v).unwrap_or_else(|_| Json::String(format!("<{}>", v.type_name())))
}
