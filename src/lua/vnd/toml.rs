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

use mlua::{AnyUserData, Lua, LuaSerdeExt, Table, Value};
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("decode", lua.create_function(decode)?)?;
    t.set("decode_string", lua.create_function(decode_string)?)?;
    t.set("encode", lua.create_function(encode)?)?;
    t.set("encode_pretty", lua.create_function(encode_pretty)?)?;
    t.set("encode_to", lua.create_function(encode_to)?)?;
    Ok(t)
}

#[instrument(name = "toml.decode", target = "luau::lib::toml", skip_all)]
fn decode(lua: &Lua, ud: AnyUserData) -> mlua::Result<(Value, Value)> {
    let rh = match ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "toml.decode", "expected a reader handle"),
    };
    let mut buf = Vec::new();
    // TODO: clean-code - 0.55 - COUPLING: reaches into ReaderHandle.0 internals instead of a method
    let result = std::io::Read::read_to_end(&mut **rh.0.lock(), &mut buf);
    drop(rh);
    if let Err(e) = result {
        return from_err(lua, "toml.decode", e);
    }
    let s = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(e) => return from_err(lua, "toml.decode", e),
    };
    parse_str(lua, s, "toml.decode")
}

#[instrument(name = "toml.decode_string", target = "luau::lib::toml", skip_all)]
fn decode_string(lua: &Lua, s: mlua::String) -> mlua::Result<(Value, Value)> {
    match std::str::from_utf8(&s.as_bytes()) {
        Ok(s) => parse_str(lua, s, "toml.decode_string"),
        Err(e) => from_err(lua, "toml.decode_string", e),
    }
}

#[instrument(name = "toml.encode", target = "luau::lib::toml", skip_all)]
fn encode(lua: &Lua, v: Value) -> mlua::Result<(Value, Value)> {
    to_string(lua, v, false)
}

#[instrument(name = "toml.encode_pretty", target = "luau::lib::toml", skip_all)]
fn encode_pretty(lua: &Lua, v: Value) -> mlua::Result<(Value, Value)> {
    to_string(lua, v, true)
}

#[instrument(name = "toml.encode_to", target = "luau::lib::toml", skip_all)]
fn encode_to(lua: &Lua, (v, ud): (Value, AnyUserData)) -> mlua::Result<(Value, Value)> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "toml.encode_to", "expected a writer handle"),
    };
    let s = match to_toml_string(&v, false) {
        Ok(s) => s,
        Err(e) => return from_err(lua, "toml.encode_to", e),
    };
    // TODO: clean-code - 0.55 - COUPLING: reaches into WriterHandle.0 internals instead of a method
    let mut locked = wh.0.lock();
    let result = std::io::Write::write_all(&mut **locked, s.as_bytes());
    drop(locked);
    drop(wh);
    match result {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "toml.encode_to", e),
    }
}

fn parse_str(lua: &Lua, s: &str, prefix: &str) -> mlua::Result<(Value, Value)> {
    match toml::from_str::<serde_json::Value>(s) {
        Ok(json) => match lua.to_value(&json) {
            Ok(v) => ok(lua, v),
            Err(e) => from_err(lua, prefix, e),
        },
        Err(e) => from_err(lua, prefix, e),
    }
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

// TODO: clean-code - 0.55 - KISS: bool flag drives both branching and prefix lookup — pass prefix explicitly
fn to_string(lua: &Lua, v: Value, pretty: bool) -> mlua::Result<(Value, Value)> {
    let prefix = if pretty {
        "toml.encode_pretty"
    } else {
        "toml.encode"
    };
    match to_toml_string(&v, pretty) {
        Ok(s) => ok(lua, s),
        Err(e) => from_err(lua, prefix, e),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks (three tiers per direction).
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

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
        .create_function(|_, _s: mlua::String| Ok(Value::Nil))
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
            let _: mlua::String = ident_encode.call(black_box(parsed_lua.clone())).unwrap();
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
