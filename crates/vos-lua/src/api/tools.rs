//! `app.tool { ... }`: an agent tool, and the types of an app's entry ([`Handlers`]).

use super::{context::Context, json::Json, schema::Schema};
use crate::vm::{Shared, ToolDecl};
use mlua_ir::ir::{Field, Ty, TypeBody};
use mlua_ir::{expected, Callback, Definition, FromLuaArg, Registry, Shape, Unknown};
use neoc_vm2::mlua;
use neoc_vm2::mlua::{Lua, Table, Value as LuaValue};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use vos_domain::{tool_schema, QualifiedToolName, ToolSpec};

mlua_ir::unknown_reason!(
    /// Why a tool's input has no declared shape here.
    ToolInput =
        "the tool's input, already checked against the tool's own `input` fields: its shape is that tool's"
);
mlua_ir::unknown_reason!(
    /// Why a tool's result has no declared shape.
    ToolOutput = "whatever JSON the tool answers the agent with"
);
mlua_ir::unknown_reason!(
    /// Why a handler's arguments have no declared shape here.
    RpcArgs =
        "the arguments the app's own page sent: their shape is the app's (its `rpc` definition types them)"
);
mlua_ir::unknown_reason!(
    /// Why a handler's result has no declared shape here.
    RpcResult = "whatever JSON the handler answers its page with: its shape is the app's"
);

/// A tool's body: `run(input, ctx)`.
pub type Run = Callback<fn(input: Json<Unknown<ToolInput>>, ctx: Context) -> Json<Unknown<ToolOutput>>>;

/// A handler of the app's backend: `handler(ctx, args)`.
pub type Handler = Callback<fn(ctx: Context, args: Json<Unknown<RpcArgs>>) -> Json<Unknown<RpcResult>>>;

/// What an app's entry returns: the handlers its page reaches with `rpc(name, args)`, as `handler(ctx, args)`. An
/// entry that returns anything else (nothing included) does not load; an app with nothing to call returns `{}`.
pub struct Handlers;

impl Shape for Handlers {
    fn shape(reg: &mut Registry) -> Ty {
        reg.named("Handlers", Some(super::MODULE), std::any::type_name::<Self>(), |reg| {
            let mut handler = <Handler as Shape>::shape(reg);
            mlua_ir::name_params(&mut handler, &["ctx", "args"]);
            Definition {
                doc: Some(
                    "What an app's entry returns: the handlers its page reaches with `rpc(name, args)`, as\n\
                     `handler(ctx, args)`. An entry that returns anything else (nothing included) does not load; an\n\
                     app with nothing to call returns `{}`."
                        .to_owned(),
                ),
                body: TypeBody::Alias { ty: Ty::Map { key: Box::new(Ty::String), value: Box::new(handler) } },
            }
        })
    }
}

/// The definition of a tool: what `app.tool` takes.
///
/// A record with a function in it, which `#[derive(Shape)]` (plain data, through serde) cannot be: its shape and
/// its reading are written here, each field from its Rust type.
pub struct Tool {
    name: String,
    title: Option<String>,
    description: Option<String>,
    input: Option<BTreeMap<String, Schema>>,
    read_only: Option<bool>,
    run: Run,
}

/// One field of [`Tool`]: its key, its doc, and (through the two functions below) its Rust type.
struct Key {
    name: &'static str,
    doc: &'static str,
}

const NAME: Key =
    Key { name: "name", doc: "A local identifier: `[a-z][a-z0-9_]*` (agents see it as `<appId>__<name>`)." };
const TITLE: Key = Key { name: "title", doc: "The name a person is shown." };
const DESCRIPTION: Key =
    Key { name: "description", doc: "What the tool does: the agent decides from it when to call the tool." };
const INPUT: Key = Key {
    name: "input",
    doc: "The fields of the tool's input, each built with `app.t`. A call is checked against them before `run`.",
};
const READ_ONLY: Key = Key {
    name: "readOnly",
    doc: "Enforced: the tool runs with app writes blocked (db writes, `app.kv` set/delete, `db:migrate`,\n\
          `app.files` writes raise) and an agent may run it without a permission prompt.",
};
const RUN: Key =
    Key { name: "run", doc: "The tool: `run(input, ctx)`. What it returns (JSON) is the answer." };

impl Key {
    /// The field in the declared record.
    fn field<T: Shape>(&self, reg: &mut Registry) -> Field {
        let mut ty = T::shape(reg);
        mlua_ir::name_params(&mut ty, &["input", "ctx"]);
        Field {
            name: self.name.to_owned(),
            doc: Some(self.doc.to_owned()),
            ty,
            default: None,
            readonly: false,
        }
    }

    /// The field's value in the table a script gave.
    fn read<T: FromLuaArg>(&self, table: &Table, lua: &Lua) -> Result<T, String> {
        let value: LuaValue = table.raw_get(self.name).map_err(|e| e.to_string())?;
        T::from_lua_arg(value, lua).map_err(|reason| format!("{}: {reason}", self.name))
    }
}

impl Shape for Tool {
    fn shape(reg: &mut Registry) -> Ty {
        reg.named("Tool", Some(super::MODULE), std::any::type_name::<Self>(), |reg| Definition {
            doc: Some("A tool an agent can call.".to_owned()),
            body: TypeBody::Record {
                fields: vec![
                    NAME.field::<String>(reg),
                    TITLE.field::<Option<String>>(reg),
                    DESCRIPTION.field::<Option<String>>(reg),
                    INPUT.field::<Option<BTreeMap<String, Schema>>>(reg),
                    READ_ONLY.field::<Option<bool>>(reg),
                    RUN.field::<Run>(reg),
                ],
            },
        })
    }
}

impl FromLuaArg for Tool {
    fn from_lua_arg(value: LuaValue, lua: &Lua) -> Result<Self, String> {
        let LuaValue::Table(table) = value else {
            return Err(expected("a tool definition (a table)", &value));
        };
        let keys = [NAME.name, TITLE.name, DESCRIPTION.name, INPUT.name, READ_ONLY.name, RUN.name];
        for pair in table.pairs::<LuaValue, LuaValue>() {
            let (key, _) = pair.map_err(|e| e.to_string())?;
            let known = matches!(&key, LuaValue::String(s) if s.to_str().is_ok_and(|s| keys.contains(&&*s)));
            if !known {
                let shown = match &key {
                    LuaValue::String(s) => format!("`{}`", s.to_string_lossy()),
                    other => format!("of type {}", other.type_name()),
                };
                return Err(format!("unknown key {shown} (a tool has {})", keys.join(", ")));
            }
        }
        Ok(Tool {
            name: NAME.read(&table, lua)?,
            title: TITLE.read(&table, lua)?,
            description: DESCRIPTION.read(&table, lua)?,
            input: INPUT
                .read(&table, lua)
                .map_err(|reason| format!("{reason}: build each field with app.t (t.string {{}} ...)"))?,
            read_only: READ_ONLY.read(&table, lua)?,
            run: RUN.read(&table, lua)?,
        })
    }
}

/// `app.tool(def)`.
pub(super) fn register(sh: &Shared, def: Tool) -> mlua::Result<()> {
    sh.writable("app.tool")?;
    let Tool { name, title, description, input, read_only, run } = def;
    QualifiedToolName::new(sh.app.clone(), &name).map_err(|e| mlua::Error::runtime(e.to_string()))?;
    let input_schema = match input {
        None => json!({"type": "object", "properties": {}, "additionalProperties": false}),
        Some(fields) => {
            let fields: Map<String, Value> =
                fields.into_iter().map(|(field, schema)| (field, schema.into_fragment())).collect();
            tool_schema::compile_fields(&fields)
                .map_err(|e| mlua::Error::runtime(format!("tool `{name}`: {e}")))?
        }
    };
    let spec = ToolSpec {
        title,
        description: description.unwrap_or_default(),
        input_schema,
        read_only: read_only.unwrap_or(false),
        name: name.clone(),
    };
    let mut tools = sh.tools.lock();
    if tools.iter().any(|t| t.spec.name == name) {
        return Err(mlua::Error::runtime(format!("tool `{name}` is already registered")));
    }
    tools.push(ToolDecl { spec, run: run.into_function() });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(src: &str) -> Result<Tool, String> {
        let lua = Lua::new();
        let value: LuaValue = lua.load(src).eval().unwrap();
        Tool::from_lua_arg(value, &lua)
    }

    #[test]
    fn a_definition_is_read_field_by_field_and_a_wrong_one_says_which() {
        let tool = read("return { name = 'count', readOnly = true, run = function() end }").unwrap();
        assert_eq!((tool.name.as_str(), tool.read_only, tool.title), ("count", Some(true), None));
        let refused = |src: &str| read(src).err().unwrap();
        assert!(refused("return { run = function() end }").starts_with("name: expected a string"));
        assert!(refused("return { name = 'x' }").starts_with("run: expected a function"));
        assert!(refused("return { name = 'x', run = function() end, readOnly = 1 }").starts_with("readOnly:"));
        assert!(refused("return { name = 'x', run = function() end, input = { a = 5 } }").contains("app.t"));
        assert!(refused("return { name = 'x', run = function() end, reads = true }")
            .contains("unknown key `reads`"));
        assert!(refused("return 'x'").contains("a tool definition"));
    }

    /// The declared record has exactly the keys the reader takes.
    #[test]
    fn the_declared_record_has_the_fields_that_are_read() {
        let mut reg = Registry::new();
        let ty = Tool::shape(&mut reg);
        let types = reg.into_types();
        let def = types.iter().find(|t| Some(t.name.as_str()) == ty.name()).unwrap();
        let TypeBody::Record { fields } = &def.body else { panic!("{def:?}") };
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["name", "title", "description", "input", "readOnly", "run"]);
        let Ty::Function { params, .. } = &fields[5].ty else { panic!("{:?}", fields[5]) };
        assert_eq!(params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["input", "ctx"]);
    }
}
