//! `require`: the host's modules, and nothing from the package.
//!
//! An app is built into one self-contained Lua 5.1 file (its own modules are bundled in at build time), so at
//! run time there is nothing to resolve in the package and no filesystem. Three kinds of name resolve:
//!
//! - `require("@agentos/app")` is the `app` table, and `require("@agentos/<name>")` a host module the host
//!   registered (the generated `ui` constructors). A host module runs once, in the app's own environment (so
//!   it sees the same polyfills), and is cached; a cycle is an error.
//! - `require("std:<name>")`, `require("lib:<name>")` and `require("vnd:<name>")` are the library modules of
//!   `neoc-vm` (neoc's own spelling: `std:task`, `lib:json`, `vnd:hyper`, ...). Each is built for the app
//!   on first use in the app's VM and cached, so every `require` of one module in an app sees one table and
//!   two apps never share one.
//!
//! Anything else is an error that names what is allowed.
//!
//! `require` is a plain (synchronous) function: a module only builds tables, so it never has to suspend.

use crate::vm::Shared;
use neoc_vm::{
    lua::{modules, sandbox::load_source},
    mlua::{self, Lua, Value as LuaValue},
};
use std::sync::Arc;

/// The modules of the VM library that an app is not offered: `lib:test` (a test harness that prints to stdout),
/// `lib:compile` (builds standalone executables) and `std:workers` (a process-wide shutdown signal). Which
/// modules a script may have is the embedder's policy; the library offers all of them (`modules::names`).
pub const NOT_FOR_APPS: &[&str] = &["lib:test", "lib:compile", "std:workers"];

/// The library modules an app (and a `std:thread` worker it starts) is offered: every one the library has,
/// less [`NOT_FOR_APPS`]. The one place that rule is written.
pub(crate) fn offered_modules() -> Vec<&'static str> {
    modules::names().filter(|name| !NOT_FOR_APPS.contains(name)).collect()
}

/// What a `require` may name, for the error message of one that names something else.
const ALLOWED: &str = "\"@agentos/app\", \"@agentos/ui\" and the std:, lib: and vnd: modules";

pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    let sh = sh.clone();
    let require = lua.create_function(move |lua, spec: String| load(lua, &sh, &spec))?;
    env.set("require", require)
}

fn load(lua: &Lua, sh: &Arc<Shared>, spec: &str) -> mlua::Result<LuaValue> {
    if spec.starts_with("std:") || spec.starts_with("lib:") || spec.starts_with("vnd:") {
        return library(lua, sh, spec);
    }
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

/// A library module (`std:task`, `lib:json`, ...), built in this VM the first time it is required.
fn library(lua: &Lua, sh: &Arc<Shared>, spec: &str) -> mlua::Result<LuaValue> {
    if let Some(done) = sh.modules.lock().get(spec) {
        return Ok(done.clone());
    }
    let offered = offered_modules();
    if !offered.contains(&spec) {
        return Err(modules::unknown_module(spec, &offered));
    }
    let built = modules::build(lua, spec).ok_or_else(|| modules::unknown_module(spec, &offered))??;
    let value = LuaValue::Table(built);
    sh.modules.lock().insert(spec.to_owned(), value.clone());
    Ok(value)
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
