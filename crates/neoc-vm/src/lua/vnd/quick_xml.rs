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
use crate::lua::serde_guard::refuse_buffer;
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:quick_xml` surface through the facade. Every function is
/// fallible (and the encoders raise for a buffer, see `serde_guard`); the `(value, error)` shape, the `quick_xml.<fn>` error prefixes,
/// the exact error suffixes (handle, UTF-8, parse, bridge, serialize, and I/O
/// failures), and the `lua::lib::quick_xml` spans are byte-identical to the
/// hand-written registration this replaces.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:quick_xml")?;
    #[lua]
    m.try_func("decode", decode)?;
    #[lua]
    m.try_func("decode_string", decode_string)?;
    #[lua]
    m.try_func_raising("encode", encode)?;
    #[lua]
    m.try_func_raising("encode_to", encode_to)?;
    Ok(m)
}

/// Read a reader handle to completion and parse it as XML.
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

/// Parse an XML string into a Lua value.
#[neoc_macros::lua_fn]
fn decode_string(lua: &Lua, s: mlua::LuaString) -> Result<Value, String> {
    let s = std::str::from_utf8(&s.as_bytes())
        .map(str::to_owned)
        .map_err(|e| e.to_string())?;
    parse_str(lua, &s)
}

/// Serialise a Lua value to an XML string rooted at `<root>`.
#[neoc_macros::lua_fn(names(v = "value"))]
fn encode(_: &Lua, v: Value) -> mlua::Result<Result<String, String>> {
    refuse_buffer(&v, "quick_xml.encode")?;
    Ok(to_xml_string(&v))
}

/// Serialise a Lua value to XML and write it to a writer handle.
#[neoc_macros::lua_fn(names(v = "value", ud = "writer"), types(ud = "std_io.WriterHandle"))]
fn encode_to(_: &Lua, (v, ud): (Value, AnyUserData)) -> mlua::Result<Result<bool, String>> {
    refuse_buffer(&v, "quick_xml.encode_to")?;
    Ok(write_xml(&v, &ud))
}

fn write_xml(v: &Value, ud: &AnyUserData) -> Result<bool, String> {
    let wh = match ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return Err("expected a writer handle".to_string()),
    };
    let s = to_xml_string(v)?;
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
