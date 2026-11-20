//! `app.tools.register { ... }` (alias `app.tool { ... }`) and `ctx:fail(message)`.

use crate::{
    convert::lua_to_json,
    error::ToolFailure,
    vm::{Shared, ToolDecl},
};
use agentos_domain::{tool_schema, QualifiedToolName, ToolSpec};
use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::{json, Value};
use std::sync::Arc;

fn text(t: &Table, key: &str) -> mlua::Result<Option<String>> {
    match t.get::<LuaValue>(key)? {
        LuaValue::Nil => Ok(None),
        LuaValue::String(s) => Ok(Some(s.to_string_lossy())),
        other => {
            Err(mlua::Error::runtime(format!("tool `{key}` must be a string, got {}", other.type_name())))
        }
    }
}

fn register(sh: &Shared, def: &Table) -> mlua::Result<()> {
    sh.writable("app.tools.register")?;
    let name = text(def, "name")?.ok_or_else(|| mlua::Error::runtime("a tool needs a `name`"))?;
    QualifiedToolName::new(sh.app.clone(), &name).map_err(|e| mlua::Error::runtime(e.to_string()))?;
    let run: Function = def.get("run").map_err(|_| {
        mlua::Error::runtime(format!("tool `{name}` needs a `run = function(input, ctx) ... end`"))
    })?;

    let input_schema = match (def.get::<LuaValue>("input")?, def.get::<LuaValue>("input_schema")?) {
        (LuaValue::Nil, LuaValue::Nil) => {
            json!({"type": "object", "properties": {}, "additionalProperties": false})
        }
        (LuaValue::Table(fields), LuaValue::Nil) => {
            let Value::Object(fields) = lua_to_json(&LuaValue::Table(fields))? else {
                // an empty table reads as `[]`: no input fields
                return finish(
                    sh,
                    &name,
                    def,
                    run,
                    json!({"type": "object", "properties": {}, "additionalProperties": false}),
                );
            };
            tool_schema::compile_fields(&fields)
                .map_err(|e| mlua::Error::runtime(format!("tool `{name}`: {e}")))?
        }
        (LuaValue::Nil, LuaValue::Table(schema)) => lua_to_json(&LuaValue::Table(schema))?,
        _ => {
            return Err(mlua::Error::runtime(format!(
                "tool `{name}`: give `input` (app.t fields) or `input_schema`, not both"
            )))
        }
    };
    finish(sh, &name, def, run, input_schema)
}

fn finish(sh: &Shared, name: &str, def: &Table, run: Function, input_schema: Value) -> mlua::Result<()> {
    let read_only = match def.get::<LuaValue>("readOnly")? {
        LuaValue::Nil => false,
        LuaValue::Boolean(b) => b,
        _ => return Err(mlua::Error::runtime(format!("tool `{name}`: readOnly must be true or false"))),
    };
    let spec = ToolSpec {
        title: text(def, "title")?,
        description: text(def, "description")?.unwrap_or_default(),
        input_schema,
        read_only,
        name: name.to_owned(),
    };
    let mut tools = sh.tools.lock();
    if tools.iter().any(|t| t.spec.name == name) {
        return Err(mlua::Error::runtime(format!("tool `{name}` is already registered")));
    }
    tools.push(ToolDecl { spec, run });
    Ok(())
}

pub(super) fn install(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    let make = |sh: &Arc<Shared>| {
        let sh = sh.clone();
        lua.create_function(move |_, args: mlua::Variadic<Table>| {
            // `register { ... }` and `register({ ... })` pass the definition; skip a leading `tools`/`app`
            let def =
                args.into_iter().last().ok_or_else(|| mlua::Error::runtime("register needs a table"))?;
            register(&sh, &def)
        })
    };
    let tools = lua.create_table()?;
    tools.set("register", make(sh)?)?;
    app.set("tools", tools)?;
    app.set("tool", make(sh)?)?;

    // ctx:fail(message): the tool reports a problem to its caller
    let fail = lua.create_function(|_, args: mlua::Variadic<LuaValue>| -> mlua::Result<()> {
        let message = args
            .into_iter()
            .find_map(|v| match v {
                LuaValue::String(s) => Some(s.to_string_lossy()),
                _ => None,
            })
            .unwrap_or_else(|| "the tool failed".to_owned());
        Err(mlua::Error::external(ToolFailure(message)))
    })?;
    *sh.fail.lock() = Some(fail);
    Ok(())
}
