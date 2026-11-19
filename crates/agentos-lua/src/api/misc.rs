//! `app.manifest`, `app.window`, `app:openWindow`, `app.on`, `app.log`, `app.notify`, `app.shell`.

use super::{check_effects_allowed, push_shell};
use crate::{
    convert::lua_to_json,
    vm::{Effect, Shared, ShellEffect},
};
use agentos_application::ports::MenuCommand;
use agentos_domain::{AppDecl, Notification};
use agentos_luavm::mlua;
use agentos_luavm::mlua::{Function, Lua, Table, Value as LuaValue, Variadic};
use serde_json::Value;
use std::sync::Arc;

/// The table argument of `f { ... }` / `f({ ... })` / `app:f { ... }`: the last table given.
fn last_table(args: Variadic<LuaValue>, what: &str) -> mlua::Result<Table> {
    args.into_iter()
        .rev()
        .find_map(|v| match v {
            LuaValue::Table(t) => Some(t),
            _ => None,
        })
        .ok_or_else(|| mlua::Error::runtime(format!("{what} takes a table")))
}

fn text_field(t: &Table, key: &str, what: &str, required: bool, max: usize) -> mlua::Result<Option<String>> {
    match t.get::<LuaValue>(key)? {
        LuaValue::Nil if required => Err(mlua::Error::runtime(format!("{what} needs `{key}`"))),
        LuaValue::Nil => Ok(None),
        LuaValue::String(s) => {
            let s = s.to_string_lossy();
            if s.chars().count() > max || (required && s.trim().is_empty()) {
                return Err(mlua::Error::runtime(format!("{what}: `{key}` must be 1 to {max} characters")));
            }
            Ok(Some(s))
        }
        other => {
            Err(mlua::Error::runtime(format!("{what}: `{key}` must be a string, got {}", other.type_name())))
        }
    }
}

pub(super) fn install(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    // app.manifest { id, name, icon, tint, window, kit }
    let s = sh.clone();
    app.set(
        "manifest",
        lua.create_function(move |_, args: Variadic<LuaValue>| {
            s.writable("app.manifest")?;
            let table = last_table(args, "app.manifest")?;
            let mut json = lua_to_json(&LuaValue::Table(table))?;
            // TODO: clean-code - 0.4 - BOUNDARY: the adapter edits the declaration before the domain parses it; tolerate the legacy `permissions` key in `AppDecl` (and drop the shim when no installed app carries it).
            // `permissions` was a key of the manifest before apps lost their permissions: still accepted (the
            // apps and tools written for it keep loading), ignored
            if let serde_json::Value::Object(map) = &mut json {
                map.remove("permissions");
            }
            let decl: AppDecl = serde_json::from_value(json)
                .map_err(|e| mlua::Error::runtime(format!("app.manifest: {e}")))?;
            decl.check_against(&s.app).map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let mut slot = s.decl.lock();
            if slot.is_some() {
                return Err(mlua::Error::runtime("app.manifest was already called"));
            }
            *slot = Some(decl);
            Ok(())
        })?,
    )?;

    // app.window(name, spec): a window kind; the view layer renders it
    let s = sh.clone();
    app.set(
        "window",
        lua.create_function(move |_, (a, b, c): (LuaValue, LuaValue, Option<Table>)| {
            s.writable("app.window")?;
            // `app.window(name, spec)` or `app:window(name, spec)`
            let (name, spec) = match (a, b, c) {
                (LuaValue::String(n), LuaValue::Table(t), None)
                | (LuaValue::Table(_), LuaValue::String(n), Some(t)) => (n.to_string_lossy(), t),
                _ => return Err(mlua::Error::runtime("app.window(name, { render = function(w) ... end })")),
            };
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                return Err(mlua::Error::runtime(format!(
                    "window name `{name}` must be letters, digits, - or _"
                )));
            }
            if !matches!(spec.get::<LuaValue>("render")?, LuaValue::Function(_)) {
                return Err(mlua::Error::runtime(format!(
                    "window `{name}` needs `render = function(w) ... end`"
                )));
            }
            if s.windows.lock().insert(name.clone(), spec).is_some() {
                return Err(mlua::Error::runtime(format!("window `{name}` is already declared")));
            }
            Ok(())
        })?,
    )?;

    // TODO: clean-code - 0.45 - DRY: same body as `w:openWindow` in view.rs (writable, declared_window, params to JSON, OpenWindow effect), and the `check_effects_allowed` + `writable` pair here repeats data.rs's `require_write`; one `Shared` method for each.
    // app:openWindow(name, params?)
    let s = sh.clone();
    app.set(
        "openWindow",
        lua.create_function(move |_, (_this, name, params): (LuaValue, String, Option<LuaValue>)| {
            check_effects_allowed(&s, "app:openWindow")?;
            s.writable("app:openWindow")?;
            s.declared_window(&name)?;
            let params = match params {
                Some(p) => lua_to_json(&p)?,
                None => Value::Null,
            };
            s.push_effect(Effect::OpenWindow { name, params })
        })?,
    )?;

    // app.on(event, fn): a named hook the host can call
    let s = sh.clone();
    app.set(
        "on",
        lua.create_function(move |_, (name, f): (String, Function)| {
            s.writable("app.on")?;
            s.handlers.lock().insert(name, f);
            Ok(())
        })?,
    )?;

    // app.log(message)
    let id = sh.app.to_string();
    app.set(
        "log",
        lua.create_function(move |_, msg: Variadic<String>| {
            tracing::info!(app = %id, "{}", msg.join(" "));
            Ok(())
        })?,
    )?;

    // app.notify { title, body?, at?, actions? } and app:notify { ... }
    let s = sh.clone();
    app.set(
        "notify",
        lua.create_function(move |_, args: Variadic<LuaValue>| {
            check_effects_allowed(&s, "app.notify")?;
            s.writable("app.notify")?;
            let t = last_table(args, "app.notify")?;
            let title = text_field(&t, "title", "app.notify", true, 200)?.unwrap_or_default();
            let body = text_field(&t, "body", "app.notify", false, 2000)?;
            text_field(&t, "at", "app.notify", false, 64)?;
            s.push_effect(Effect::Notify(Notification { app: s.app.clone(), title, body }))
        })?,
    )?;

    install_agents(lua, sh, app)?;
    install_shell(lua, sh, app)
}

/// `app.agents:list()`: the agent contacts as `{ id, name, avatar?, role }`. Never grants, tokens or launch
/// commands.
fn install_agents(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    let agents = lua.create_table()?;
    let s = sh.clone();
    agents.set(
        "list",
        lua.create_function(move |lua, _: Variadic<LuaValue>| {
            let list = lua.create_table()?;
            for (i, card) in s.roster.cards().into_iter().enumerate() {
                let row = lua.create_table()?;
                row.set("id", card.id.0)?;
                row.set("name", card.name)?;
                if let Some(avatar) = card.avatar {
                    row.set("avatar", avatar)?;
                }
                row.set("role", card.role)?;
                list.raw_set(i + 1, row)?;
            }
            Ok(list)
        })?,
    )?;
    app.set("agents", agents)
}

/// `app.shell`: Dock badge, menu commands, deep links. Declarations only: the shell implements them later.
fn install_shell(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    let shell = lua.create_table()?;

    // app.shell.badge(value): a string, a number, or nil to clear
    let s = sh.clone();
    shell.set(
        "badge",
        lua.create_function(move |_, value: Option<LuaValue>| {
            s.writable("app.shell.badge")?;
            let badge = match value {
                None | Some(LuaValue::Nil) => None,
                Some(LuaValue::String(t)) => Some(t.to_string_lossy()),
                Some(LuaValue::Integer(n)) => Some(n.to_string()),
                Some(LuaValue::Number(n)) => Some(n.to_string()),
                Some(other) => {
                    return Err(mlua::Error::runtime(format!(
                        "badge must be a string, a number or nil, got {}",
                        other.type_name()
                    )))
                }
            };
            push_shell(&s, ShellEffect::Badge(badge.filter(|b| !b.is_empty() && b != "0")));
            Ok(())
        })?,
    )?;

    // app.shell.command { id, title, shortcut?, run = function() end }
    let s = sh.clone();
    shell.set(
        "command",
        lua.create_function(move |_, def: Table| {
            s.writable("app.shell.command")?;
            let id = text_field(&def, "id", "app.shell.command", true, 64)?.unwrap_or_default();
            let title = text_field(&def, "title", "app.shell.command", true, 80)?.unwrap_or_default();
            let shortcut = text_field(&def, "shortcut", "app.shell.command", false, 32)?;
            let run: Function = def
                .get("run")
                .map_err(|_| mlua::Error::runtime("app.shell.command needs `run = function() ... end`"))?;
            let mut commands = s.commands.lock();
            if commands.iter().any(|(c, _)| c.id == id) {
                return Err(mlua::Error::runtime(format!("menu command `{id}` is already declared")));
            }
            commands.push((MenuCommand { id, title, shortcut }, run));
            let all = commands.iter().map(|(c, _)| c.clone()).collect();
            drop(commands);
            push_shell(&s, ShellEffect::Commands(all));
            Ok(())
        })?,
    )?;

    // app.shell.deepLink(pattern, handler): e.g. "note/:id"
    let s = sh.clone();
    shell.set(
        "deepLink",
        lua.create_function(move |_, (pattern, handler): (String, Function)| {
            s.writable("app.shell.deepLink")?;
            if pattern.is_empty() || pattern.len() > 128 {
                return Err(mlua::Error::runtime(
                    "a deep link pattern is 1 to 128 characters, like \"note/:id\"",
                ));
            }
            let mut links = s.deep_links.lock();
            links.retain(|(p, _)| *p != pattern);
            links.push((pattern, handler));
            let all = links.iter().map(|(p, _)| p.clone()).collect();
            drop(links);
            push_shell(&s, ShellEffect::DeepLinks(all));
            Ok(())
        })?,
    )?;
    app.set("shell", shell)
}
