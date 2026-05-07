//! XML encode / decode — `lib.quick_xml` (backed by `quick-xml`).
//!
//! | Function                   | Backed by                          |
//! |----------------------------|------------------------------------|
//! | `decode(reader)`           | `quick_xml::de::from_str`*         |
//! | `decode_string(s)`         | `quick_xml::de::from_str`          |
//! | `encode(value)`            | `quick_xml::se::to_string`         |
//! | `encode_to(value, writer)` | `quick_xml::se::to_string` + write |
//!
//! *`decode` reads the handle to completion first, then parses. Decode goes
//! through `serde_json::Value` then `lua.to_value`. quick-xml represents text
//! content as `$text` and attributes prefixed with `@`.

use mlua::{AnyUserData, Lua, LuaSerdeExt, Table, Value};
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

#[instrument(name = "quick_xml.decode", target = "luau::lib::quick_xml", skip_all)]
fn decode(lua: &Lua, ud: AnyUserData) -> mlua::Result<(Value, Value)> {
    let rh = match ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "quick_xml.decode", "expected a reader handle"),
    };
    let mut buf = Vec::new();
    let result = std::io::Read::read_to_end(&mut **rh.0.lock(), &mut buf);
    drop(rh);
    if let Err(e) = result {
        return from_err(lua, "quick_xml.decode", e);
    }
    let s = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(e) => return from_err(lua, "quick_xml.decode", e),
    };
    parse_str(lua, s, "quick_xml.decode")
}

#[instrument(
    name = "quick_xml.decode_string",
    target = "luau::lib::quick_xml",
    skip_all
)]
fn decode_string(lua: &Lua, s: mlua::String) -> mlua::Result<(Value, Value)> {
    match std::str::from_utf8(&s.as_bytes()) {
        Ok(s) => parse_str(lua, s, "quick_xml.decode_string"),
        Err(e) => from_err(lua, "quick_xml.decode_string", e),
    }
}

#[instrument(name = "quick_xml.encode", target = "luau::lib::quick_xml", skip_all)]
fn encode(lua: &Lua, v: Value) -> mlua::Result<(Value, Value)> {
    match to_xml_string(&v) {
        Ok(s) => ok(lua, s),
        Err(e) => from_err(lua, "quick_xml.encode", e),
    }
}

#[instrument(
    name = "quick_xml.encode_to",
    target = "luau::lib::quick_xml",
    skip_all
)]
fn encode_to(lua: &Lua, (v, ud): (Value, AnyUserData)) -> mlua::Result<(Value, Value)> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "quick_xml.encode_to", "expected a writer handle"),
    };
    let s = match to_xml_string(&v) {
        Ok(s) => s,
        Err(e) => return from_err(lua, "quick_xml.encode_to", e),
    };
    let mut locked = wh.0.lock();
    let result = std::io::Write::write_all(&mut **locked, s.as_bytes());
    drop(locked);
    drop(wh);
    match result {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "quick_xml.encode_to", e),
    }
}

fn parse_str(lua: &Lua, s: &str, prefix: &str) -> mlua::Result<(Value, Value)> {
    match quick_xml::de::from_str::<serde_json::Value>(s) {
        Ok(json) => {
            let flat = flatten_text(json);
            match lua.to_value(&flat) {
                Ok(v) => ok(lua, v),
                Err(e) => from_err(lua, prefix, e),
            }
        }
        Err(e) => from_err(lua, prefix, e),
    }
}

fn to_xml_string(v: &Value) -> Result<String, String> {
    let json = serde_json::to_value(v).map_err(|e| e.to_string())?;
    quick_xml::se::to_string_with_root("root", &json).map_err(|e| e.to_string())
}

/// quick-xml represents element text as `{"$text": "..."}`. When that's the
/// only field, replace the object with the bare text so `<name>x</name>`
/// decodes to `{ name = "x" }` instead of `{ name = { ["$text"] = "x" } }`.
fn flatten_text(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(mut map) => {
            if map.len() == 1
                && let Some(text) = map.remove("$text")
            {
                return text;
            }
            let entries: Vec<_> = map.into_iter().map(|(k, v)| (k, flatten_text(v))).collect();
            serde_json::Value::Object(entries.into_iter().collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(flatten_text).collect())
        }
        other => other,
    }
}
