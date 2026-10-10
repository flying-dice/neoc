//! YAML encode / decode — `lib.serde_yaml` (mirrors `serde_yaml`).
//!
//! | Function                   | Rust equivalent              |
//! |----------------------------|------------------------------|
//! | `decode(reader)`           | `serde_yaml::from_reader`    |
//! | `decode_string(s)`         | `serde_yaml::from_str`       |
//! | `encode(value)`            | `serde_yaml::to_string`      |
//! | `encode_to(value, writer)` | `serde_yaml::to_writer`      |
//!
//! Decode and encode both go through `serde_json::Value` as the
//! normalisation step. YAML-specific types (timestamps, tagged values)
//! are flattened to strings/objects in that step and are not preserved.

use mlua::{AnyUserData, Error, Lua, LuaSerdeExt, Table, Value};

use crate::lua::facade::Module;
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:serde_yaml` surface through the facade. Every function is
/// fallible; the `(value, error)` shape, the `serde_yaml.<fn>` error prefixes,
/// the exact error suffixes (handle, parse, serialize, and I/O failures), and
/// the `lua::lib::serde_yaml` spans are byte-identical to the hand-written
/// registration this replaces.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:serde_yaml")?;
    m.try_func(
        "decode",
        &[("reader", "LuaValue")],
        "Read a reader handle to completion and parse it as YAML.",
        decode,
    )?
    .try_func(
        "decode_string",
        &[("s", "string")],
        "Parse a YAML string into a Lua value.",
        decode_string,
    )?
    .try_func(
        "encode",
        &[("value", "LuaValue")],
        "Serialise a Lua value to a YAML string.",
        encode,
    )?
    .try_func(
        "encode_to",
        &[("value", "LuaValue"), ("writer", "LuaValue")],
        "Serialise a Lua value to YAML and write it to a writer handle.",
        encode_to,
    )?;
    Ok(m)
}

/// Borrow `ud` as handle type `T`, or return the standard "expected a `<kind>`
/// handle" error shared by `decode` and `encode_to`.
fn borrow_handle<T: 'static>(ud: &AnyUserData, kind: &str) -> Result<mlua::UserDataRef<T>, String> {
    ud.borrow::<T>()
        .map_err(|_| format!("expected a {kind} handle"))
}

fn decode(lua: &Lua, ud: AnyUserData) -> Result<Value, String> {
    let rh = borrow_handle::<ReaderHandle>(&ud, "reader")?;
    let mut locked = rh.inner.lock();
    let result = serde_yaml::from_reader::<_, serde_json::Value>(&mut **locked);
    drop(locked);
    drop(rh);
    json_result_to_value(lua, result)
}

fn decode_string(lua: &Lua, s: mlua::String) -> Result<Value, String> {
    let bytes = s.as_bytes();
    let result = serde_yaml::from_slice::<serde_json::Value>(&bytes);
    json_result_to_value(lua, result)
}

/// Materialise the `Result<serde_json::Value, _>` returned by every YAML
/// parser into a Lua value or a stringified error. Decode and decode_string
/// share the same parse → lua bridge → wrap tail.
fn json_result_to_value<E: std::fmt::Display>(
    lua: &Lua,
    parse_result: Result<serde_json::Value, E>,
) -> Result<Value, String> {
    let json = parse_result.map_err(|e| e.to_string())?;
    lua.to_value(&json).map_err(|e| e.to_string())
}

fn encode(_: &Lua, v: Value) -> Result<String, String> {
    let json = lua_to_json_value(&v).map_err(|e| e.to_string())?;
    serde_yaml::to_string(&json).map_err(|e| e.to_string())
}

fn encode_to(_: &Lua, (v, ud): (Value, AnyUserData)) -> Result<bool, String> {
    let wh = borrow_handle::<WriterHandle>(&ud, "writer")?;
    let json = lua_to_json_value(&v).map_err(|e| e.to_string())?;
    let mut locked = wh.0.lock();
    let result = serde_yaml::to_writer(&mut **locked, &json);
    drop(locked);
    drop(wh);
    match result {
        Ok(()) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

/// Bridge a Lua `Value` to a `serde_json::Value` so it can hand off to
/// `serde_yaml::to_*`. The YAML side uses JSON as its normalisation step
/// (see module docstring); both encode entry points need this conversion,
/// so the lossy-yaml note lives here too.
fn lua_to_json_value(v: &Value) -> Result<serde_json::Value, Error> {
    serde_json::to_value(v).map_err(Error::external)
}

// ---------------------------------------------------------------------------
// Benchmarks (three tiers per direction).
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    const PAYLOAD: &str = r#"
server:
  host: localhost
  port: 8080
  workers: 4
database:
  url: postgres://localhost/app
  pool_size: 10
  timeout: 30
app:
  features: [auth, logging, metrics]
  debug: false
  log_level: info
  max_retries: 3
"#;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload_lua = lua.create_string(PAYLOAD).expect("payload");

    let mut g = c.benchmark_group("serde_yaml");
    g.throughput(Throughput::Bytes(PAYLOAD.len() as u64));

    // -- decode (string -> Value) -------------------------------------------
    g.bench_function("decode_string/library", |b| {
        b.iter(|| {
            let _: serde_yaml::Value = serde_yaml::from_str(black_box(PAYLOAD)).unwrap();
        });
    });

    let ident_decode: mlua::Function = lua
        .create_function(|_, _s: mlua::String| Ok(Value::Nil))
        .expect("ident_decode");
    g.bench_function("decode_string/into_out_lua", |b| {
        b.iter(|| {
            let _: Value = ident_decode.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let decode_chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("vnd:serde_yaml")).decode_string(p) end"#)
        .eval()
        .expect("decode chunk");
    g.bench_function("decode_string/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = decode_chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    // -- encode (Value -> string) -------------------------------------------
    let parsed_json: serde_json::Value = serde_yaml::from_str(PAYLOAD).expect("seed");
    let parsed_lua: Value = {
        let chunk: mlua::Function = lua
            .load(r#"return function(p) return (require("vnd:serde_yaml")).decode_string(p) end"#)
            .eval()
            .expect("seed chunk");
        let (v, _): (Value, Value) = chunk.call(payload_lua.clone()).expect("seed eval");
        v
    };

    g.bench_function("encode/library", |b| {
        b.iter(|| {
            let _ = serde_yaml::to_string(black_box(&parsed_json)).unwrap();
        });
    });

    let ident_encode: mlua::Function = lua
        .create_function(|lua, _v: Value| lua.create_string(""))
        .expect("ident_encode");
    g.bench_function("encode/into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident_encode.call(black_box(parsed_lua.clone())).unwrap();
        });
    });

    let encode_chunk: mlua::Function = lua
        .load(r#"return function(v) return (require("vnd:serde_yaml")).encode(v) end"#)
        .eval()
        .expect("encode chunk");
    g.bench_function("encode/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = encode_chunk.call(black_box(parsed_lua.clone())).unwrap();
        });
    });

    g.finish();
}
