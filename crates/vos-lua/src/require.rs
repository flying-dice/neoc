//! `require`: the `app` table and the library's modules, and nothing from the package.
//!
//! An app is built into one self-contained Luau file (its own modules are bundled in at build time), so at
//! run time there is nothing to resolve in the package and no filesystem. Two kinds of name resolve:
//!
//! - `require("vos:app")` is the `app` table, the only `vos:` name there is.
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
    mlua::{self, Lua, Value as LuaValue},
    modules,
};
use std::sync::Arc;

/// The modules of the VM library that an app is not offered (the list is `vos_domain::NOT_FOR_APPS`, which the Luau
/// declarations are generated from as well). Which modules a script may have is the embedder's policy; the library
/// offers all of them (`modules::names`).
pub use vos_domain::NOT_FOR_APPS;

/// Is `name` a module of the VM library (`std:`, `lib:`, `vnd:`)? The registry also lists the modules this crate
/// declares itself (`vos:app`, which is the app's own table and never a worker's).
pub(crate) fn is_library(name: &str) -> bool {
    name.starts_with("std:") || name.starts_with("lib:") || name.starts_with("vnd:")
}

/// The library modules an app (and a `std:thread` worker it starts) is offered: every one the library has,
/// less [`NOT_FOR_APPS`] and less `refused` (`VmConfig::refused_modules`, empty in the host). The one place that
/// rule is written.
pub(crate) fn offered_modules(refused: &[String]) -> Vec<&'static str> {
    modules::names()
        .filter(|name| is_library(name) && !NOT_FOR_APPS.contains(name) && !refused.iter().any(|r| r == name))
        .collect()
}

/// What a `require` may name, for the error message of one that names something else.
const ALLOWED: &str = "\"vos:app\" and the std:, lib: and vnd: modules";

pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>, env: &mlua::Table) -> mlua::Result<()> {
    let sh = sh.clone();
    let require = lua.create_function(move |lua, spec: String| load(lua, &sh, &spec))?;
    env.set("require", require)
}

fn load(lua: &Lua, sh: &Arc<Shared>, spec: &str) -> mlua::Result<LuaValue> {
    if is_library(spec) {
        return library(lua, sh, spec);
    }
    if spec == crate::api::MODULE {
        return sh
            .app_table
            .lock()
            .clone()
            .map(LuaValue::Table)
            .ok_or_else(|| mlua::Error::runtime("the app table is gone (the app was unloaded)"));
    }
    if let Some(name) = spec.strip_prefix("vos:") {
        return Err(mlua::Error::runtime(format!(
            "unknown host module \"vos:{name}\": only {ALLOWED} can be required"
        )));
    }
    Err(mlua::Error::runtime(format!(
        "require(\"{spec}\"): only {ALLOWED} can be required at run time (an app's own modules are bundled \
         into its file when it is built)"
    )))
}

/// A library module (`std:task`, `lib:json`, ...), built in this VM the first time it is required.
fn library(lua: &Lua, sh: &Arc<Shared>, spec: &str) -> mlua::Result<LuaValue> {
    if let Some(done) = sh.modules.lock().get(spec) {
        return Ok(done.clone());
    }
    let offered = &sh.offered;
    if !offered.contains(&spec) {
        return Err(modules::unknown_module(spec, offered));
    }
    let built = modules::build(lua, spec).ok_or_else(|| modules::unknown_module(spec, offered))??;
    let value = LuaValue::Table(built);
    sh.modules.lock().insert(spec.to_owned(), value.clone());
    Ok(value)
}
