//! TOML encode / decode — `lib.toml` (mirrors the `toml` crate).
//!
//! | Function                   | Rust equivalent        |
//! |----------------------------|------------------------|
//! | `decode(reader)`           | `toml::from_str`*      |
//! | `decode_string(s)`         | `toml::from_str`       |
//! | `encode(value)`            | `toml::to_string`      |
//! | `encode_pretty(value)`     | `toml::to_string_pretty` |
//! | `encode_to(value, writer)` | `toml::to_string` + write |
//!
//! *TOML has no streaming parser; `decode` reads the handle to completion
//! first, then parses. The root value must be a table for encoding.
//!
//! The `toml` 1.x crate implements TOML 1.1: inline tables may span lines and end
//! with a comma, `\e` is a string escape, and times may omit seconds
//! (`07:32`). Documents that 1.0 parsers (and `toml` 0.8) rejected now decode.

use mlua::{AnyUserData, Lua, LuaSerdeExt, Table, Value};

use crate::lua::facade::Module;
use crate::lua::serde_guard::refuse_buffer;
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:toml` surface through the facade. Every function is
/// fallible (and the encoders raise for a buffer, see `serde_guard`); the `(value, error)` shape, the `toml.<fn>` error prefixes, the
/// exact error suffixes (reader/writer-handle, UTF-8, parse, serialize, and
/// I/O failures), and the `lua::lib::toml` spans are byte-identical to the
/// hand-written registration this replaces.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:toml")?;
    #[lua]
    m.try_func("decode", decode)?;
    #[lua]
    m.try_func("decode_string", decode_string)?;
    #[lua]
    m.try_func_raising("encode", encode)?;
    #[lua]
    m.try_func_raising("encode_pretty", encode_pretty)?;
    #[lua]
    m.try_func_raising("encode_to", encode_to)?;
    Ok(m)
}

/// Read a reader handle to completion and parse it as TOML.
#[neoc_macros::lua_fn(names(ud = "reader"), types(ud = "std_io.ReaderHandle"))]
fn decode(lua: &Lua, ud: AnyUserData) -> Result<Value, String> {
    let rh = match ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => return Err("expected a reader handle".to_string()),
    };
    let mut buf = Vec::new();
    let result = std::io::Read::read_to_end(&mut **rh.inner.lock(), &mut buf);
    drop(rh);
    if let Err(e) = result {
        return Err(e.to_string());
    }
    let s = std::str::from_utf8(&buf).map_err(|e| e.to_string())?;
    parse_str(lua, s)
}

/// Parse a TOML string into a Lua value.
#[neoc_macros::lua_fn]
fn decode_string(lua: &Lua, s: mlua::LuaString) -> Result<Value, String> {
    let s = std::str::from_utf8(&s.as_bytes())
        .map(str::to_owned)
        .map_err(|e| e.to_string())?;
    parse_str(lua, &s)
}

/// Serialise a Lua value to a compact TOML string.
#[neoc_macros::lua_fn(names(v = "value"))]
fn encode(_: &Lua, v: Value) -> mlua::Result<Result<String, String>> {
    refuse_buffer(&v, "toml.encode")?;
    Ok(to_toml_string(&v, false))
}

/// Serialise a Lua value to a pretty-printed TOML string.
#[neoc_macros::lua_fn(names(v = "value"))]
fn encode_pretty(_: &Lua, v: Value) -> mlua::Result<Result<String, String>> {
    refuse_buffer(&v, "toml.encode_pretty")?;
    Ok(to_toml_string(&v, true))
}

/// Serialise a Lua value to TOML and write it to a writer handle.
#[neoc_macros::lua_fn(names(v = "value", ud = "writer"), types(ud = "std_io.WriterHandle"))]
fn encode_to(_: &Lua, (v, ud): (Value, AnyUserData)) -> mlua::Result<Result<bool, String>> {
    refuse_buffer(&v, "toml.encode_to")?;
    Ok(write_toml(&v, &ud))
}

fn write_toml(v: &Value, ud: &AnyUserData) -> Result<bool, String> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return Err("expected a writer handle".to_string()),
    };
    let s = to_toml_string(v, false)?;
    match wh.write_all(s.as_bytes()) {
        Ok(()) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

fn parse_str(lua: &Lua, s: &str) -> Result<Value, String> {
    let json = toml::from_str::<serde_json::Value>(s).map_err(|e| e.to_string())?;
    lua.to_value(&json).map_err(|e| e.to_string())
}

fn to_toml_string(v: &Value, pretty: bool) -> Result<String, String> {
    let json = serde_json::to_value(v).map_err(|e| e.to_string())?;
    let result = if pretty {
        toml::to_string_pretty(&json)
    } else {
        toml::to_string(&json)
    };
    result.map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Benchmarks (three tiers per direction).
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::Throughput;
    use std::hint::black_box;

    const PAYLOAD: &str = r#"
[server]
host = "localhost"
port = 8080
workers = 4

[database]
url = "postgres://localhost/app"
pool_size = 10
timeout = 30

[app]
features = ["auth", "logging", "metrics"]
debug = false
log_level = "info"
max_retries = 3
"#;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload_lua = lua.create_string(PAYLOAD).expect("payload");

    let mut g = c.benchmark_group("toml");
    g.throughput(Throughput::Bytes(PAYLOAD.len() as u64));

    // -- decode (string -> Value) -------------------------------------------
    g.bench_function("decode_string/library", |b| {
        b.iter(|| {
            let _: serde_json::Value = ::toml::from_str(black_box(PAYLOAD)).unwrap();
        });
    });

    let ident_decode: mlua::Function = lua
        .create_function(|_, _s: mlua::LuaString| Ok(Value::Nil))
        .expect("ident_decode");
    g.bench_function("decode_string/into_out_lua", |b| {
        b.iter(|| {
            let _: Value = ident_decode.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let decode_chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("vnd:toml")).decode_string(p) end"#)
        .eval()
        .expect("decode chunk");
    g.bench_function("decode_string/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = decode_chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    // -- encode (Value -> string) -------------------------------------------
    let parsed_json: serde_json::Value = ::toml::from_str(PAYLOAD).expect("seed");
    let parsed_lua: Value = {
        let chunk: mlua::Function = lua
            .load(r#"return function(p) return (require("vnd:toml")).decode_string(p) end"#)
            .eval()
            .expect("seed chunk");
        let (v, _): (Value, Value) = chunk.call(payload_lua.clone()).expect("seed eval");
        v
    };

    g.bench_function("encode/library", |b| {
        b.iter(|| {
            let _ = ::toml::to_string(black_box(&parsed_json)).unwrap();
        });
    });

    let ident_encode: mlua::Function = lua
        .create_function(|lua, _v: Value| lua.create_string(""))
        .expect("ident_encode");
    g.bench_function("encode/into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::LuaString = ident_encode.call(black_box(parsed_lua.clone())).unwrap();
        });
    });

    let encode_chunk: mlua::Function = lua
        .load(r#"return function(v) return (require("vnd:toml")).encode(v) end"#)
        .eval()
        .expect("encode chunk");
    g.bench_function("encode/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = encode_chunk.call(black_box(parsed_lua.clone())).unwrap();
        });
    });

    g.finish();
}
