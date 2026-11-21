//! `app.t`: input schema builders for tools.
//!
//! ```lua
//! local t = app.t
//! input = {
//!   id    = t.integer { description = "Note id", min = 1 },
//!   text  = t.string {},
//!   limit = t.integer { default = 10, min = 1, max = 50 },
//!   tag   = t.string { optional = true },
//!   mode  = t.enum { "a", "b" },
//!   tags  = t.array { items = t.string {} },
//! }
//! ```
//! Each builder returns a plain table: a JSON Schema fragment. `agentos_domain::tool_schema` compiles the
//! fields into the tool's schema and validates every call against it.

use crate::convert::{json_to_lua, lua_to_json};
use agentos_domain::tool_schema::{compile_fields, OPTIONAL_KEY};
use neoc_vm::mlua;
use neoc_vm::mlua::{Lua, Table, Value as LuaValue};
use serde_json::{Map, Value};

/// Options a builder understands, renamed to their JSON Schema keys.
const RENAMES: &[(&str, &str)] = &[
    ("description", "description"),
    ("default", "default"),
    ("min", "minimum"),
    ("max", "maximum"),
    ("minLength", "minLength"),
    ("maxLength", "maxLength"),
    ("optional", OPTIONAL_KEY),
];

fn fragment(kind: &str, opts: Option<&Table>) -> mlua::Result<Map<String, Value>> {
    let mut out = Map::new();
    out.insert("type".into(), Value::String(kind.into()));
    if let Some(opts) = opts {
        let given = match lua_to_json(&LuaValue::Table(opts.clone()))? {
            Value::Object(m) => m,
            Value::Array(a) if a.is_empty() => Map::new(),
            _ => {
                return Err(mlua::Error::runtime(format!(
                    "t.{kind}: options must be a table of named options"
                )))
            }
        };
        for (key, value) in given {
            match RENAMES.iter().find(|(from, _)| *from == key) {
                Some((_, to)) => {
                    out.insert((*to).to_owned(), value);
                }
                None => return Err(mlua::Error::runtime(format!("t.{kind}: unknown option `{key}`"))),
            }
        }
    }
    Ok(out)
}

fn as_schema(m: Map<String, Value>) -> Value {
    Value::Object(m)
}

pub(super) fn builders(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for kind in ["string", "integer", "number", "boolean"] {
        t.set(
            kind,
            lua.create_function(move |lua, opts: Option<Table>| {
                json_to_lua(lua, &as_schema(fragment(kind, opts.as_ref())?))
            })?,
        )?;
    }
    // t.array(items, options?)
    t.set(
        "array",
        lua.create_function(|lua, (items, opts): (Table, Option<Table>)| {
            let mut out = fragment("array", opts.as_ref())?;
            out.insert("items".into(), lua_to_json(&LuaValue::Table(items))?);
            json_to_lua(lua, &as_schema(out))
        })?,
    )?;
    // t.object(fields, options?): the fields compile like a tool's `input`
    t.set(
        "object",
        lua.create_function(|lua, (fields, opts): (Table, Option<Table>)| {
            let mut out = fragment("object", opts.as_ref())?;
            let fields = match lua_to_json(&LuaValue::Table(fields))? {
                Value::Object(m) => m,
                _ => Map::new(),
            };
            if let Value::Object(compiled) = compile_fields(&fields).map_err(mlua::Error::runtime)? {
                out.extend(compiled);
            }
            json_to_lua(lua, &as_schema(out))
        })?,
    )?;
    // t.enum({ "a", "b" }, options?)
    t.set(
        "enum",
        lua.create_function(|lua, (values, opts): (Table, Option<Table>)| {
            let mut options = Vec::new();
            for v in values.sequence_values::<LuaValue>() {
                options.push(lua_to_json(&v?)?);
            }
            if options.is_empty() {
                return Err(mlua::Error::runtime(
                    "t.enum needs at least one value: t.enum({ \"a\", \"b\" })",
                ));
            }
            let mut out = fragment("enum", opts.as_ref())?;
            out.remove("type");
            out.insert("enum".into(), Value::Array(options));
            json_to_lua(lua, &as_schema(out))
        })?,
    )?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eval(src: &str) -> Value {
        let lua = Lua::new();
        lua.globals().set("t", builders(&lua).unwrap()).unwrap();
        lua_to_json(&lua.load(src).eval::<LuaValue>().unwrap()).unwrap()
    }

    #[test]
    fn builders_make_json_schema_fragments() {
        assert_eq!(
            eval("return t.integer { description = 'Note id', min = 1, max = 9, default = 3 }"),
            json!({"type": "integer", "description": "Note id", "minimum": 1, "maximum": 9, "default": 3})
        );
        assert_eq!(eval("return t.string()"), json!({"type": "string"}));
        assert_eq!(
            eval("return t.string { optional = true }"),
            json!({"type": "string", "x-optional": true})
        );
        assert_eq!(
            eval("return t.array(t.number {})"),
            json!({"type": "array", "items": {"type": "number"}})
        );
        assert_eq!(
            eval("return t.enum({ 'a', 'b' }, { description = 'mode' })"),
            json!({"enum": ["a", "b"], "description": "mode"})
        );
    }

    #[test]
    fn nested_objects_compile_their_fields() {
        let v = eval("return t.object({ id = t.integer {}, note = t.string { optional = true } })");
        assert_eq!(v["required"], json!(["id"]));
        assert_eq!(v["properties"]["note"], json!({"type": "string"}));
    }

    #[test]
    fn typos_are_errors_not_silently_ignored() {
        let lua = Lua::new();
        lua.globals().set("t", builders(&lua).unwrap()).unwrap();
        let e = lua.load("return t.string { descripton = 'x' }").eval::<LuaValue>().unwrap_err().to_string();
        assert!(e.contains("unknown option `descripton`"), "{e}");
        assert!(lua.load("return t.enum({})").eval::<LuaValue>().is_err());
    }
}
