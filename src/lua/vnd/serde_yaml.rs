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
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("decode", lua.create_function(decode)?)?;
    t.set("decode_string", lua.create_function(decode_string)?)?;
    t.set("encode", lua.create_function(encode)?)?;
    t.set("encode_to", lua.create_function(encode_to)?)?;
    Ok(t)
}

#[instrument(name = "serde_yaml.decode", target = "luau::lib::serde_yaml", skip_all)]
// TODO: clean-code - 0.7 - DRY: reader-handle borrow + error mirrors encode_to's writer-handle pattern
fn decode(lua: &Lua, ud: AnyUserData) -> mlua::Result<(Value, Value)> {
    let rh = match ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "serde_yaml.decode", "expected a reader handle"),
    };
    let mut locked = rh.inner.lock();
    let result = serde_yaml::from_reader::<_, serde_json::Value>(&mut **locked);
    drop(locked);
    drop(rh);
    // TODO: clean-code - 0.65 - KISS: nested match-of-match; flatten with and_then/map_err + single dispatch
    match result {
        Ok(json) => match lua.to_value(&json) {
            Ok(v) => ok(lua, v),
            Err(e) => from_err(lua, "serde_yaml.decode", e),
        },
        Err(e) => from_err(lua, "serde_yaml.decode", e),
    }
}

#[instrument(
    name = "serde_yaml.decode_string",
    target = "luau::lib::serde_yaml",
    skip_all
)]
fn decode_string(lua: &Lua, s: mlua::String) -> mlua::Result<(Value, Value)> {
    let bytes = s.as_bytes();
    // TODO: clean-code - 0.75 - DRY: yaml-bytes -> json::Value -> lua::Value pipeline duplicates decode()'s tail
    match serde_yaml::from_slice::<serde_json::Value>(&bytes) {
        Ok(json) => match lua.to_value(&json) {
            Ok(v) => ok(lua, v),
            Err(e) => from_err(lua, "serde_yaml.decode_string", e),
        },
        Err(e) => from_err(lua, "serde_yaml.decode_string", e),
    }
}

#[instrument(name = "serde_yaml.encode", target = "luau::lib::serde_yaml", skip_all)]
fn encode(lua: &Lua, v: Value) -> mlua::Result<(Value, Value)> {
    // TODO: clean-code - 0.8 - DRY: lua::Value -> serde_json::Value bridge duplicated in encode_to
    match serde_json::to_value(&v).map_err(Error::external) {
        Ok(json) => match serde_yaml::to_string(&json) {
            Ok(s) => ok(lua, s),
            Err(e) => from_err(lua, "serde_yaml.encode", e),
        },
        Err(e) => from_err(lua, "serde_yaml.encode", e),
    }
}

#[instrument(
    name = "serde_yaml.encode_to",
    target = "luau::lib::serde_yaml",
    skip_all
)]
fn encode_to(lua: &Lua, (v, ud): (Value, AnyUserData)) -> mlua::Result<(Value, Value)> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "serde_yaml.encode_to", "expected a writer handle"),
    };
    // TODO: clean-code - 0.8 - DRY: lua::Value -> serde_json::Value bridge duplicated from encode
    let json = match serde_json::to_value(&v).map_err(Error::external) {
        Ok(j) => j,
        Err(e) => return from_err(lua, "serde_yaml.encode_to", e),
    };
    let mut locked = wh.0.lock();
    let result = serde_yaml::to_writer(&mut **locked, &json);
    drop(locked);
    drop(wh);
    match result {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "serde_yaml.encode_to", e),
    }
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
