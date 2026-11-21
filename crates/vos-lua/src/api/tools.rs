//! `app.tools.register { ... }` (alias `app.tool { ... }`) and `ctx:fail(message)`.

use crate::{
    convert::lua_to_json,
    error::{RpcFailure, ToolFailure},
    vm::{Shared, ToolDecl},
};
use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_domain::{tool_schema, QualifiedToolName, ToolSpec};

fn text(t: &Table, key: &str) -> mlua::Result<Option<String>> {
    match t.get::<LuaValue>(key)? {
        LuaValue::Nil => Ok(None),
        LuaValue::String(s) => Ok(Some(s.to_string_lossy())),
        other => {
            Err(mlua::Error::runtime(format!("tool `{key}` must be a string, got {}", other.type_name())))
        }
    }
}

/// A whole number that fits an `i32`.
fn whole(n: f64) -> Option<i32> {
    #[allow(clippy::cast_possible_truncation)] // a whole number inside the i32 range, checked here
    (n.fract() == 0.0 && n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX)).then_some(n as i32)
}

fn code_error() -> mlua::Error {
    mlua::Error::runtime("ctx:fail: `code` must be a whole number (a JSON-RPC error code)")
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

    // ctx:fail(message, { code, data }?): the tool or handler reports a problem to its caller; with a `code` it is a
    // JSON-RPC error of that code
    let fail = lua.create_function(|_, args: mlua::Variadic<LuaValue>| -> mlua::Result<()> {
        let mut message = None;
        let mut options = None;
        for value in args {
            match value {
                LuaValue::String(s) if message.is_none() => message = Some(s.to_string_lossy()),
                LuaValue::Table(t) if options.is_none() => options = Some(t),
                _ => {}
            }
        }
        let message = message.unwrap_or_else(|| "the tool failed".to_owned());
        let Some(options) = options else { return Err(mlua::Error::external(ToolFailure(message))) };
        let code: i32 = match options.get::<LuaValue>("code")? {
            LuaValue::Nil => return Err(mlua::Error::external(ToolFailure(message))),
            LuaValue::Integer(i) => i32::try_from(i).map_err(|_| code_error())?,
            LuaValue::Number(n) => whole(n).ok_or_else(code_error)?,
            _ => return Err(code_error()),
        };
        let data = match options.get::<LuaValue>("data")? {
            LuaValue::Nil => None,
            other => Some(lua_to_json(&other)?),
        };
        Err(mlua::Error::external(RpcFailure { code, message, data }))
    })?;
    *sh.fail.lock() = Some(fail);
    Ok(())
}
