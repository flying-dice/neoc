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

use crate::lua::facade::Module;
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:quick_xml` surface through the facade. Every function is
/// fallible; the `(value, error)` shape, the `quick_xml.<fn>` error prefixes,
/// the exact error suffixes (handle, UTF-8, parse, bridge, serialize, and I/O
/// failures), and the `lua::lib::quick_xml` spans are byte-identical to the
/// hand-written registration this replaces.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:quick_xml")?;
    m.try_func(
        "decode",
        &[("reader", "LuaValue")],
        "Read a reader handle to completion and parse it as XML.",
        decode,
    )?
    .try_func(
        "decode_string",
        &[("s", "string")],
        "Parse an XML string into a Lua value.",
        decode_string,
    )?
    .try_func(
        "encode",
        &[("value", "LuaValue")],
        "Serialise a Lua value to an XML string rooted at `<root>`.",
        encode,
    )?
    .try_func(
        "encode_to",
        &[("value", "LuaValue"), ("writer", "LuaValue")],
        "Serialise a Lua value to XML and write it to a writer handle.",
        encode_to,
    )?;
    Ok(m)
}

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

fn decode_string(lua: &Lua, s: mlua::LuaString) -> Result<Value, String> {
    let s = std::str::from_utf8(&s.as_bytes())
        .map(str::to_owned)
        .map_err(|e| e.to_string())?;
    parse_str(lua, &s)
}

fn encode(_: &Lua, v: Value) -> Result<String, String> {
    to_xml_string(&v)
}

fn encode_to(_: &Lua, (v, ud): (Value, AnyUserData)) -> Result<bool, String> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return Err("expected a writer handle".to_string()),
    };
    let s = to_xml_string(&v)?;
    let mut locked = wh.0.lock();
    let result = std::io::Write::write_all(&mut **locked, s.as_bytes());
    drop(locked);
    drop(wh);
    match result {
        Ok(()) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

fn parse_str(lua: &Lua, s: &str) -> Result<Value, String> {
    let json = quick_xml::de::from_str::<serde_json::Value>(s).map_err(|e| e.to_string())?;
    let flat = flatten_text(json);
    lua.to_value(&flat).map_err(|e| e.to_string())
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
