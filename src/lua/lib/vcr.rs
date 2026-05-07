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

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(
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
    Ok(t)
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

struct Tape {
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

impl serde::Serialize for Tape {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.entries.serialize(s)
    }
}

// ---------------------------------------------------------------------------
// off (pass-through)
// ---------------------------------------------------------------------------

#[instrument(name = "vcr.off", target = "luau::lib::vcr", skip_all)]
fn passthrough(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(
        "wrap",
        lua.create_function(|_, (_, _key, f): (mlua::Value, String, mlua::Function)| Ok(f))?,
    )?;
    t.set("dump", lua.create_function(|_, _: mlua::Value| Ok(()))?)?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// record
// ---------------------------------------------------------------------------

#[instrument(name = "vcr.flush", target = "luau::lib::vcr", skip_all)]
fn flush(tape: &Tape, path: &str) -> mlua::Result<()> {
    let json = serde_json::to_string_pretty(tape).map_err(mlua::Error::external)?;
    std::fs::write(path, json).map_err(mlua::Error::external)?;
    Ok(())
}

#[instrument(name = "vcr.record", target = "luau::lib::vcr", skip_all)]
fn recording(lua: &Lua, path: String) -> mlua::Result<Table> {
    let tape: Arc<Mutex<Tape>> = Arc::new(Mutex::new(Tape {
        registered: BTreeSet::new(),
        entries: BTreeMap::new(),
    }));
    flush(&tape.lock(), &path)?;

    let t = lua.create_table()?;

    {
        let tape = tape.clone();
        let path = path.clone();
        t.set(
            "wrap",
            lua.create_function(
                move |lua, (_, key, f): (mlua::Value, String, mlua::Function)| {
                    if !tape.lock().register(&key) {
                        return Err(mlua::Error::runtime(format!(
                            "vcr: duplicate key \"{key}\""
                        )));
                    }
                    let reg_key = lua.create_registry_value(f)?;
                    let tape = tape.clone();
                    let path = path.clone();
                    lua.create_function(move |lua, args: MultiValue| {
                        let json_args = args.iter().map(to_json).collect();
                        let f: mlua::Function = lua.registry_value(&reg_key)?;
                        let result: MultiValue = f.call(args)?;
                        let json_result = result.iter().map(to_json).collect();
                        let mut locked = tape.lock();
                        locked.record(
                            key.clone(),
                            Entry {
                                args: json_args,
                                result: json_result,
                            },
                        );
                        flush(&locked, &path)?;
                        Ok(result)
                    })
                },
            )?,
        )?;
    }

    t.set("dump", lua.create_function(|_, _: mlua::Value| Ok(()))?)?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// replay
// ---------------------------------------------------------------------------

#[instrument(name = "vcr.replay", target = "luau::lib::vcr", skip_all)]
fn replay(lua: &Lua, path: &str) -> mlua::Result<Table> {
    let content = std::fs::read_to_string(path).map_err(mlua::Error::external)?;
    let cassette: BTreeMap<String, Entry> =
        serde_json::from_str(&content).map_err(mlua::Error::external)?;
    let cassette = Arc::new(cassette);
    let registered: Arc<Mutex<BTreeSet<String>>> = Arc::new(Mutex::new(BTreeSet::new()));

    let t = lua.create_table()?;

    {
        let cassette = cassette.clone();
        t.set(
            "wrap",
            lua.create_function(
                move |lua, (_, key, _f): (mlua::Value, String, mlua::Function)| {
                    if !registered.lock().insert(key.clone()) {
                        return Err(mlua::Error::runtime(format!(
                            "vcr: duplicate key \"{key}\""
                        )));
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
            )?,
        )?;
    }

    t.set("dump", lua.create_function(|_, _: mlua::Value| Ok(()))?)?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn to_json(v: &mlua::Value) -> Json {
    serde_json::to_value(v).unwrap_or_else(|_| Json::String(format!("<{}>", v.type_name())))
}
