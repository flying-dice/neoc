//! JSON encode / decode — `lib.serde_json` (mirrors `serde_json`).
//!
//! | Lua function              | Rust equivalent                |
//! |---------------------------|--------------------------------|
//! | `from_str(s)`             | `serde_json::from_str`         |
//! | `from_reader(reader)`     | `serde_json::from_reader`      |
//! | `to_string(value)`        | `serde_json::to_string`        |
//! | `to_string_pretty(value)` | `serde_json::to_string_pretty` |
//! | `to_writer(value, writer)`| `serde_json::to_writer`        |

use std::collections::BTreeMap;

use mlua::{AnyUserData, Lua, Table, Value};
use tracing::instrument;

use crate::lua::lib::json::{JsonValue, lua_value_to_json_value, wrap_root};
use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::io::{ReaderHandle, WriterHandle};

// ---------------------------------------------------------------------------
// Bridge: serde_json::Value ↔ lib::json::JsonValue
//
// `lib:json` is intentionally serde_json-free — its in-memory shape is
// pure std (`String`, `Vec`, `BTreeMap`, primitives). The serde_json crate
// remains the only thing in this codebase that knows how to *parse* JSON
// bytes, so this module owns the conversion both ways.
// ---------------------------------------------------------------------------

fn jv_to_json_value(jv: serde_json::Value) -> JsonValue {
    match jv {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        // TODO: clean-code - 0.55 - KISS: Number conversion cascade is dense; extract jv_number_to_json with named branches
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Int(i)
            } else if let Some(u) = n.as_u64() {
                if u <= i64::MAX as u64 {
                    JsonValue::Int(u as i64)
                } else {
                    JsonValue::Float(u as f64)
                }
            } else if let Some(f) = n.as_f64() {
                JsonValue::Float(f)
            } else {
                JsonValue::Null
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s),
        serde_json::Value::Array(a) => {
            JsonValue::Array(a.into_iter().map(jv_to_json_value).collect())
        }
        serde_json::Value::Object(m) => {
            let mut out = BTreeMap::new();
            for (k, v) in m {
                out.insert(k, jv_to_json_value(v));
            }
            JsonValue::Object(out)
        }
    }
}

fn json_value_to_jv(j: &JsonValue) -> serde_json::Value {
    match j {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Int(i) => serde_json::Value::Number((*i).into()),
        JsonValue::Float(f) => match serde_json::Number::from_f64(*f) {
            Some(n) => serde_json::Value::Number(n),
            None => serde_json::Value::Null, // NaN/Inf — JSON has no syntax for these
        },
        JsonValue::String(s) => serde_json::Value::String(s.clone()),
        JsonValue::Array(a) => serde_json::Value::Array(a.iter().map(json_value_to_jv).collect()),
        JsonValue::Object(m) => {
            let mut map = serde_json::Map::with_capacity(m.len());
            for (k, v) in m {
                map.insert(k.clone(), json_value_to_jv(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

/// Public for `vnd:hyper`'s `req.json` getter — same parse pipeline,
/// reused so we don't duplicate the bridge.
pub(crate) fn parse_to_handle(lua: &Lua, bytes: &[u8]) -> mlua::Result<Value> {
    let jv: serde_json::Value = serde_json::from_slice(bytes).map_err(mlua::Error::external)?;
    wrap_root(lua, jv_to_json_value(jv))
}

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("from_str", lua.create_function(from_str)?)?;
    t.set("from_reader", lua.create_function(from_reader)?)?;
    t.set("to_string", lua.create_function(to_string)?)?;
    t.set("to_string_pretty", lua.create_function(to_string_pretty)?)?;
    t.set("to_writer", lua.create_function(to_writer)?)?;
    Ok(t)
}

#[instrument(
    name = "serde_json.from_str",
    target = "luau::lib::serde_json",
    skip_all
)]
fn from_str(lua: &Lua, s: mlua::String) -> mlua::Result<(Value, Value)> {
    match parse_to_handle(lua, &s.as_bytes()) {
        Ok(v) => ok(lua, v),
        Err(e) => from_err(lua, "serde_json.from_str", e),
    }
}

#[instrument(
    name = "serde_json.from_reader",
    target = "luau::lib::serde_json",
    skip_all
)]
// TODO: clean-code - 0.75 - KISS: nested match pyramid + manual drops; flatten with ? and a scoped block
fn from_reader(lua: &Lua, ud: AnyUserData) -> mlua::Result<(Value, Value)> {
    let rh = match ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => {
            return from_err(
                lua,
                "serde_json.from_reader",
                "expected a reader handle — accepted: Reader",
            );
        }
    };
    let mut locked = rh.inner.lock();
    let result = serde_json::from_reader::<_, serde_json::Value>(&mut **locked);
    drop(locked);
    drop(rh);
    match result {
        Ok(jv) => match wrap_root(lua, jv_to_json_value(jv)) {
            Ok(v) => ok(lua, v),
            Err(e) => from_err(lua, "serde_json.from_reader", e),
        },
        Err(e) => from_err(lua, "serde_json.from_reader", e),
    }
}

fn serialize_with(
    lua: &Lua,
    v: Value,
    op: &'static str,
    formatter: impl Fn(&serde_json::Value) -> serde_json::Result<String>,
) -> mlua::Result<(Value, Value)> {
    let j = match lua_value_to_json_value(&v) {
        Ok(j) => j,
        Err(e) => return from_err(lua, op, e),
    };
    match formatter(&json_value_to_jv(&j)) {
        Ok(s) => ok(lua, s),
        Err(e) => from_err(lua, op, e),
    }
}

#[instrument(
    name = "serde_json.to_string",
    target = "luau::lib::serde_json",
    skip_all
)]
fn to_string(lua: &Lua, v: Value) -> mlua::Result<(Value, Value)> {
    serialize_with(lua, v, "serde_json.to_string", serde_json::to_string)
}

#[instrument(
    name = "serde_json.to_string_pretty",
    target = "luau::lib::serde_json",
    skip_all
)]
fn to_string_pretty(lua: &Lua, v: Value) -> mlua::Result<(Value, Value)> {
    serialize_with(
        lua,
        v,
        "serde_json.to_string_pretty",
        serde_json::to_string_pretty,
    )
}

#[instrument(
    name = "serde_json.to_writer",
    target = "luau::lib::serde_json",
    skip_all
)]
fn to_writer(lua: &Lua, (v, ud): (Value, AnyUserData)) -> mlua::Result<(Value, Value)> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "serde_json.to_writer", "expected a writer handle"),
    };
    let j = match lua_value_to_json_value(&v) {
        Ok(j) => j,
        Err(e) => return from_err(lua, "serde_json.to_writer", e),
    };
    let jv = json_value_to_jv(&j);
    let mut locked = wh.0.lock();
    let result = serde_json::to_writer(&mut **locked, &jv);
    drop(locked);
    drop(wh);
    match result {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "serde_json.to_writer", e),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks (three tiers per direction).
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    const PAYLOAD: &str = r#"{"users":[{"id":1,"name":"Alice","email":"alice@example.com","active":true},{"id":2,"name":"Bob","email":"bob@example.com","active":false},{"id":3,"name":"Carol","email":"carol@example.com","active":true}],"total":3,"page":1}"#;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload_lua = lua.create_string(PAYLOAD).expect("payload");

    let mut g = c.benchmark_group("serde_json");
    g.throughput(Throughput::Bytes(PAYLOAD.len() as u64));

    // -- decode (string -> Value) -------------------------------------------
    g.bench_function("from_str/library", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(PAYLOAD)).unwrap();
        });
    });

    // Boundary: takes a Lua string, returns nil. Measures string-marshal-in
    // + Function call cost, no parsing.
    let ident_decode: mlua::Function = lua
        .create_function(|_, _s: mlua::String| Ok(Value::Nil))
        .expect("ident_decode");
    g.bench_function("from_str/into_out_lua", |b| {
        b.iter(|| {
            let _: Value = ident_decode.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let from_str_chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("vnd:serde_json")).from_str(p) end"#)
        .eval()
        .expect("from_str chunk");
    g.bench_function("from_str/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = from_str_chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    // -- encode (Value -> string) -------------------------------------------
    let parsed: serde_json::Value = serde_json::from_str(PAYLOAD).expect("seed");
    let parsed_lua: Value = {
        // Materialise the same shape into Lua tables once for the encode tier.
        let chunk: mlua::Function = lua
            .load(r#"return function(p) return (require("vnd:serde_json")).from_str(p) end"#)
            .eval()
            .expect("seed chunk");
        let (v, _): (Value, Value) = chunk.call(payload_lua.clone()).expect("seed eval");
        v
    };

    g.bench_function("to_string/library", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&parsed)).unwrap();
        });
    });

    // Boundary for encode: take a Value, return a fixed Lua string.
    let ident_encode: mlua::Function = lua
        .create_function(|lua, _v: Value| lua.create_string(""))
        .expect("ident_encode");
    g.bench_function("to_string/into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident_encode.call(black_box(parsed_lua.clone())).unwrap();
        });
    });

    let to_string_chunk: mlua::Function = lua
        .load(r#"return function(v) return (require("vnd:serde_json")).to_string(v) end"#)
        .eval()
        .expect("to_string chunk");
    g.bench_function("to_string/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = to_string_chunk.call(black_box(parsed_lua.clone())).unwrap();
        });
    });

    g.finish();

    // -----------------------------------------------------------------------
    // Large payload — 1000 user records, ~200 KiB, deeply enough nested that
    // the per-node `serde_json::Value` → `lua.to_value` walk dominates over
    // fixed costs. Same three tiers; the through_lua delta vs library here
    // is the one to watch when optimising the marshalling path.
    // -----------------------------------------------------------------------

    let large_payload = build_large_json_payload();
    let large_payload_lua = lua.create_string(&large_payload).expect("large payload");

    let mut g = c.benchmark_group("serde_json_large");
    g.throughput(Throughput::Bytes(large_payload.len() as u64));
    // Large payload benches need longer iter time per sample to stay above
    // the noise floor; small sample count keeps total runtime sane.
    g.sample_size(10);

    g.bench_function("from_str/library", |b| {
        b.iter(|| {
            let _: serde_json::Value =
                serde_json::from_str(black_box(large_payload.as_str())).unwrap();
        });
    });

    g.bench_function("from_str/into_out_lua", |b| {
        b.iter(|| {
            let _: Value = ident_decode
                .call(black_box(large_payload_lua.clone()))
                .unwrap();
        });
    });

    g.bench_function("from_str/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = from_str_chunk
                .call(black_box(large_payload_lua.clone()))
                .unwrap();
        });
    });

    let parsed_large: serde_json::Value = serde_json::from_str(&large_payload).expect("seed large");
    let parsed_large_lua: Value = {
        let (v, _): (Value, Value) = from_str_chunk
            .call(large_payload_lua.clone())
            .expect("seed large eval");
        v
    };

    g.bench_function("to_string/library", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&parsed_large)).unwrap();
        });
    });

    g.bench_function("to_string/into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident_encode
                .call(black_box(parsed_large_lua.clone()))
                .unwrap();
        });
    });

    g.bench_function("to_string/through_lua", |b| {
        b.iter(|| {
            let _: (Value, Value) = to_string_chunk
                .call(black_box(parsed_large_lua.clone()))
                .unwrap();
        });
    });

    g.finish();
}

#[cfg(feature = "bench")]
fn build_large_json_payload() -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(200_000);
    s.push_str(r#"{"total":1000,"page":1,"users":["#);
    for i in 0..1000u32 {
        if i > 0 {
            s.push(',');
        }
        // Deliberate mix of types per record to exercise every Visitor arm
        // in the from_str path: integer, string, boolean, nested object,
        // array of strings, float.
        let _ = write!(
            s,
            r#"{{"id":{i},"name":"User Name {i}","email":"user{i}@example.com","active":{active},"address":{{"street":"123 Main St #{i}","city":"Springfield","zip":"00000"}},"tags":["alpha","beta","gamma","delta"],"score":{score:.4}}}"#,
            active = if i % 2 == 0 { "true" } else { "false" },
            score = (i as f64) * 0.5,
        );
    }
    s.push_str("]}");
    s
}
