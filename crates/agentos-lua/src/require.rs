//! `require`: the two host modules, and nothing else.
//!
//! An app is built into one self-contained Lua 5.1 file (its own modules are bundled in at build time), so at
//! run time there is nothing to resolve in the package and no filesystem. `require("@agentos/app")` is the
//! capability table; `require("@agentos/<name>")` is a host module the host registered (the generated `ui`
//! constructors). Anything else is an error that names what is allowed. A host module runs once, in the
//! app's own environment (so it sees the same polyfills), and is cached; a cycle is an error.
//!
//! `require` is a plain (synchronous) function: a host module only builds tables, and a function the VM
//! cannot yield from is the one thing a metamethod or a C boundary on Lua 5.1 must be.

use crate::vm::Shared;
use agentos_luavm::{
    lua::sandbox::load_source,
    mlua::{self, Lua, Value as LuaValue},
};
use std::sync::Arc;

/// What a `require` may name, for the error message of one that names something else.
const ALLOWED: &str = "\"@agentos/app\" and \"@agentos/ui\"";

pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    let sh = sh.clone();
    let require = lua.create_function(move |lua, spec: String| load(lua, &sh, &spec))?;
    env.set("require", require)
}

fn load(lua: &Lua, sh: &Arc<Shared>, spec: &str) -> mlua::Result<LuaValue> {
    let Some(name) = spec.strip_prefix("@agentos/") else {
        return Err(mlua::Error::runtime(format!(
            "require(\"{spec}\"): only {ALLOWED} can be required at run time (an app's own modules are bundled \
             into its file when it is built)"
        )));
    };
    if name == "app" {
        return sh
            .app_table
            .lock()
            .clone()
            .map(LuaValue::Table)
            .ok_or_else(|| mlua::Error::runtime("the app table is gone (the app was unloaded)"));
    }
    let source = sh.host_modules.get(name).ok_or_else(|| {
        mlua::Error::runtime(format!(
            "unknown host module \"@agentos/{name}\": only {ALLOWED} can be required"
        ))
    })?;
    run_module(lua, sh, &format!("@agentos/{name}"), source)
}

fn run_module(lua: &Lua, sh: &Arc<Shared>, key: &str, source: &[u8]) -> mlua::Result<LuaValue> {
    if let Some(done) = sh.modules.lock().get(key) {
        return Ok(done.clone());
    }
    let env = sh
        .env
        .lock()
        .clone()
        .ok_or_else(|| mlua::Error::runtime("the app environment is gone (the app was unloaded)"))?;
    if !sh.loading.lock().insert(key.to_owned()) {
        let chain: Vec<String> = sh.loading.lock().iter().cloned().collect();
        return Err(mlua::Error::runtime(format!(
            "cyclic require of `{key}` (loading: {})",
            chain.join(", ")
        )));
    }
    let result =
        load_source(lua, source).set_name(format!("={key}")).set_environment(env).call::<LuaValue>(());
    sh.loading.lock().remove(key);
    let value = match result? {
        LuaValue::Nil => LuaValue::Boolean(true),
        v => v,
    };
    sh.modules.lock().insert(key.to_owned(), value.clone());
    Ok(value)
}
