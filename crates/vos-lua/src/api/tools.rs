//! `app.tools.register { ... }` (alias `app.tool { ... }`) and `ctx:fail(message)`.

use crate::{
    convert::{json_to_lua, lua_to_json},
    error::ToolFailure,
    vm::{Shared, ToolDecl},
};
use neoc_vm::lua::lifecycle::{async_function, Role};
use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_domain::{tool_schema, Actor, QualifiedToolName, ToolSpec};

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

/// `app.tools.all()`: every tool of every running app, as an agent is shown them.
fn all(lua: &Lua, sh: &Arc<Shared>) -> mlua::Result<Function> {
    let sh = sh.clone();
    lua.create_function(move |lua, _: mlua::Variadic<LuaValue>| {
        let list = lua.create_table()?;
        for (name, spec) in sh.host_tools.all() {
            let row = lua.create_table()?;
            row.set("name", name.to_string())?;
            row.set("app", name.app().to_string())?;
            if let Some(title) = spec.title {
                row.set("title", title)?;
            }
            row.set("description", spec.description)?;
            row.set("input_schema", json_to_lua(lua, &spec.input_schema)?)?;
            row.set("read_only", spec.read_only)?;
            list.raw_push(row)?;
        }
        Ok(list)
    })
}

/// The longest id or name of the agent a tool call is made for.
const MAX_CALLER: usize = 200;

/// `app.tools.call(name, args, agent)`: run a tool of a running app for an agent of this app's own. It waits
/// without the app's turn ([`Role::Wait`]): the tool runs in another app's VM, or in this one's as any other call.
fn call(lua: &Lua, sh: &Arc<Shared>) -> mlua::Result<Function> {
    let sh = sh.clone();
    async_function(lua, Role::Wait, move |lua, (name, args, agent): (String, LuaValue, Table)| {
        // read here, in the call itself: a table can run the app's code when it is read
        let prepared = (|| {
            sh.writable("app.tools.call")?;
            let name = QualifiedToolName::parse(&name).map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let who = |key: &str| -> mlua::Result<String> {
                match text(&agent, key)? {
                    Some(value) if !value.is_empty() && value.len() <= MAX_CALLER => Ok(value),
                    _ => Err(mlua::Error::runtime(format!(
                        "app.tools.call: the agent needs `{key}`, a string of 1 to {MAX_CALLER} bytes"
                    ))),
                }
            };
            // the id is the app's own for its agent: under the app's name, it is never the id of an agent the person
            // connected to this machine, nor of another app's
            let caller = Actor::agent(&format!("{}:{}", sh.app, who("id")?), &who("name")?);
            let args = match args {
                LuaValue::Nil => json!({}),
                // an empty table reads as `[]`: no arguments
                args => match lua_to_json(&args)? {
                    Value::Array(items) if items.is_empty() => json!({}),
                    args => args,
                },
            };
            Ok::<_, mlua::Error>((name, args, caller))
        })();
        let sh = sh.clone();
        async move {
            let (name, args, caller) = prepared?;
            match sh.host_tools.call(caller, &name, args).await {
                Ok(result) => Ok((json_to_lua(&lua, &result)?, LuaValue::Nil)),
                Err(e) => Ok((LuaValue::Nil, LuaValue::String(lua.create_string(e.to_string())?))),
            }
        }
    })
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
    tools.set("all", all(lua, sh)?)?;
    tools.set("call", call(lua, sh)?)?;
    app.set("tools", tools)?;
    app.set("tool", make(sh)?)?;

    // ctx:fail(message): the tool or handler reports a problem to its caller
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
