//! `app.t`: input schema builders for tools.
//!
//! ```lua
//! local t = app.t
//! input = {
//!   id    = t.integer { description = "Note id", min = 1 },
//!   text  = t.string {},
//!   limit = t.integer { default = 10, min = 1, max = 50 },
//!   tag   = t.string { optional = true },
//!   mode  = t.enum({ "a", "b" }),
//!   tags  = t.array(t.string {}),
//! }
//! ```
//! Each builder returns a [`Schema`]: a JSON Schema fragment the script holds but does not read.
//! `vos_domain::tool_schema` compiles the fields into the tool's schema and validates every call against it.

use super::app::t;
use mlua_ir::{Object, Options, Registry, Shape};
use neoc_vm2::mlua;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use vos_domain::tool_schema::{compile_fields, OPTIONAL_KEY};

/// The default of a field: any JSON value (it is checked against the field's type when a call leaves it out).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DefaultValue(Value);

impl Shape for DefaultValue {
    fn shape(_: &mut Registry) -> mlua_ir::ir::Ty {
        mlua_ir::ir::Ty::Unknown { reason: "the default of a field: its type is the field's own".to_owned() }
    }
}

/// What a builder takes. A field is required unless it is `optional` or has a `default`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vos:app")]
pub struct SchemaOptions {
    /// What the field is for: an agent reads it.
    pub description: Option<String>,
    /// The value the field takes when a call leaves it out.
    pub default: Option<DefaultValue>,
    /// The least a number may be.
    pub min: Option<f64>,
    /// The most a number may be.
    pub max: Option<f64>,
    /// The fewest characters a string may have.
    #[serde(rename = "minLength")]
    pub min_length: Option<u32>,
    /// The most characters a string may have.
    #[serde(rename = "maxLength")]
    pub max_length: Option<u32>,
    /// A call may leave the field out.
    pub optional: bool,
}

/// The type of one input field of a tool, made by a builder of `app.t`.
#[derive(Clone, Debug, PartialEq, Object)]
#[shape(module = "vos:app")]
pub struct Schema {
    fragment: Map<String, Value>,
}

impl Schema {
    /// The JSON Schema fragment (with the `x-optional` marker `compile_fields` reads).
    pub(crate) fn into_fragment(self) -> Value {
        Value::Object(self.fragment)
    }
}

#[mlua_ir::object(module = "vos:app")]
impl Schema {
    /// The JSON Schema fragment as text.
    #[to_string]
    pub fn display(&self) -> String {
        Value::Object(self.fragment.clone()).to_string()
    }
}

/// A whole number as a JSON integer: a script's `1` is `1`, not `1.0`, in the schema an agent reads.
fn number(n: f64) -> Value {
    // 2^53: beyond it a double no longer represents every integer
    const LIMIT: f64 = 9_007_199_254_740_992.0;
    if n.fract() == 0.0 && n.abs() < LIMIT {
        #[allow(clippy::cast_possible_truncation)]
        return Value::from(n as i64);
    }
    Number::from_f64(n).map_or(Value::Null, Value::Number)
}

/// Every whole number inside `value` as a JSON integer (a default is read through serde, which keeps Luau's
/// doubles as they are).
fn whole_numbers(value: Value) -> Value {
    match value {
        Value::Number(n) => n.as_f64().map_or(Value::Number(n), number),
        Value::Array(items) => Value::Array(items.into_iter().map(whole_numbers).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, whole_numbers(v))).collect()),
        other => other,
    }
}

fn fragment(kind: &str, options: SchemaOptions) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("type".into(), Value::String(kind.into()));
    let SchemaOptions { description, default, min, max, min_length, max_length, optional } = options;
    let mut set = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            out.insert(key.to_owned(), value);
        }
    };
    set("description", description.map(Value::String));
    set("default", default.map(|d| whole_numbers(d.0)));
    set("minimum", min.map(number));
    set("maximum", max.map(number));
    set("minLength", min_length.map(Value::from));
    set("maxLength", max_length.map(Value::from));
    set(OPTIONAL_KEY, optional.then_some(Value::Bool(true)));
    out
}

fn of(kind: &str, options: SchemaOptions) -> Schema {
    Schema { fragment: fragment(kind, options) }
}

#[mlua_ir::object(module = "vos:app")]
impl t {
    /// Text.
    pub fn string(options: SchemaOptions) -> Schema {
        of("string", options)
    }

    /// A whole number.
    pub fn integer(options: SchemaOptions) -> Schema {
        of("integer", options)
    }

    /// A number.
    pub fn number(options: SchemaOptions) -> Schema {
        of("number", options)
    }

    /// `true` or `false`.
    pub fn boolean(options: SchemaOptions) -> Schema {
        of("boolean", options)
    }

    /// A list whose items are `items`.
    pub fn array(items: Schema, options: SchemaOptions) -> Schema {
        let mut out = fragment("array", options);
        out.insert("items".into(), items.into_fragment());
        Schema { fragment: out }
    }

    /// A table with these fields: they are required or optional as a tool's `input` fields are.
    pub fn object(fields: BTreeMap<String, Schema>, options: SchemaOptions) -> mlua::Result<Schema> {
        let mut out = fragment("object", options);
        let fields: Map<String, Value> =
            fields.into_iter().map(|(name, field)| (name, field.into_fragment())).collect();
        if let Value::Object(compiled) = compile_fields(&fields).map_err(mlua::Error::runtime)? {
            out.extend(compiled);
        }
        Ok(Schema { fragment: out })
    }

    /// One of `values`.
    pub fn r#enum(values: Vec<String>, options: SchemaOptions) -> mlua::Result<Schema> {
        if values.is_empty() {
            return Err(mlua::Error::runtime("t.enum needs at least one value: t.enum({ \"a\", \"b\" })"));
        }
        let mut out = fragment("enum", options);
        out.remove("type");
        out.insert("enum".into(), Value::Array(values.into_iter().map(Value::String).collect()));
        Ok(Schema { fragment: out })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_ir::{FromLuaArg as _, ObjectDef as _};
    use neoc_vm2::mlua::{Lua, Value as LuaValue};
    use serde_json::json;

    fn engine() -> Lua {
        let lua = Lua::new();
        lua.globals().set("t", t::statics(&lua).unwrap()).unwrap();
        lua
    }

    fn eval(src: &str) -> Value {
        let lua = engine();
        let built: LuaValue = lua.load(src).eval().unwrap();
        Schema::from_lua_arg(built, &lua).unwrap().into_fragment()
    }

    #[test]
    fn builders_make_json_schema_fragments() {
        assert_eq!(
            eval("return t.integer { description = 'Note id', min = 1, max = 9, default = 3 }"),
            json!({"type": "integer", "description": "Note id", "minimum": 1, "maximum": 9, "default": 3})
        );
        assert!(eval("return t.integer { min = 1, default = 3 }")["default"].is_i64(), "3, not 3.0");
        assert_eq!(eval("return t.number { max = 0.5 }"), json!({"type": "number", "maximum": 0.5}));
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
        let lua = engine();
        let e = lua.load("return t.string { descripton = 'x' }").eval::<LuaValue>().unwrap_err().to_string();
        assert!(e.contains("descripton"), "{e}");
        assert!(lua.load("return t.enum({})").eval::<LuaValue>().is_err());
        // a field is a `Schema`, not a table that looks like one
        let e = lua.load("return t.array({ type = 'string' })").eval::<LuaValue>().unwrap_err().to_string();
        assert!(e.contains("expected a Schema"), "{e}");
    }

    #[test]
    fn a_schema_prints_as_its_json() {
        let lua = engine();
        let text: String = lua.load("return tostring(t.string { optional = true })").eval().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            json!({"type": "string", "x-optional": true})
        );
    }
}
