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
use mlua::{Lua, Table, Value as LuaValue};
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

fn fragment(kind: &str, opts: Option<&Table>) -> mlua::Result<Value> {
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
                None if key == "items" && kind == "array" => {
                    out.insert(key, value);
                }
                None if key == "fields" && kind == "object" => {
                    let fields = value.as_object().cloned().unwrap_or_default();
                    let compiled = compile_fields(&fields).map_err(mlua::Error::runtime)?;
                    if let Value::Object(c) = compiled {
                        out.extend(c);
                    }
                }
                None => return Err(mlua::Error::runtime(format!("t.{kind}: unknown option `{key}`"))),
            }
        }
    }
    if kind == "object" && !out.contains_key("properties") {
        out.insert("properties".into(), Value::Object(Map::new()));
    }
    Ok(Value::Object(out))
}

pub(super) fn builders(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for kind in ["string", "integer", "number", "boolean", "array", "object"] {
        t.set(
            kind,
            lua.create_function(move |lua, opts: Option<Table>| {
                let value = fragment(kind, opts.as_ref())?;
                json_to_lua(lua, &value)
            })?,
        )?;
    }
    // t.enum { "a", "b", description = "..." }: the positional entries are the allowed values
    t.set(
        "enum",
        lua.create_function(|lua, opts: Table| {
            let mut out = Map::new();
            let mut options = Vec::new();
            for v in opts.sequence_values::<LuaValue>() {
                options.push(lua_to_json(&v?)?);
            }
            if options.is_empty() {
                return Err(mlua::Error::runtime("t.enum needs at least one value: t.enum { \"a\", \"b\" }"));
            }
            out.insert("enum".into(), Value::Array(options));
            for (key, to) in RENAMES {
                if let Ok(v) = opts.get::<LuaValue>(*key) {
                    if !v.is_nil() {
                        out.insert((*to).to_owned(), lua_to_json(&v)?);
                    }
                }
            }
            json_to_lua(lua, &Value::Object(out))
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
            eval("return t.array { items = t.number {} }"),
            json!({"type": "array", "items": {"type": "number"}})
        );
        assert_eq!(
            eval("return t.enum { 'a', 'b', description = 'mode' }"),
            json!({"enum": ["a", "b"], "description": "mode"})
        );
    }

    #[test]
    fn nested_objects_compile_their_fields() {
        let v =
            eval("return t.object { fields = { id = t.integer {}, note = t.string { optional = true } } }");
        assert_eq!(v["required"], json!(["id"]));
        assert_eq!(v["properties"]["note"], json!({"type": "string"}));
    }

    #[test]
    fn typos_are_errors_not_silently_ignored() {
        let lua = Lua::new();
        lua.globals().set("t", builders(&lua).unwrap()).unwrap();
        let e = lua.load("return t.string { descripton = 'x' }").eval::<LuaValue>().unwrap_err().to_string();
        assert!(e.contains("unknown option `descripton`"), "{e}");
        assert!(lua.load("return t.enum {}").eval::<LuaValue>().is_err());
    }
}
