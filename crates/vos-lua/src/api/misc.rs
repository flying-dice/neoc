//! `app:openWindow`, `app.log`, `app.notify`, `app.emit`, `app.agents`.

use crate::{
    convert::lua_to_json,
    vm::{Effect, Shared},
};
use neoc_vm::mlua;
use neoc_vm::mlua::{Lua, Table, Value as LuaValue, Variadic};
use serde_json::Value;
use std::sync::Arc;
use vos_domain::Notification;

/// A value's type as Luau's `typeof` names it (mlua tells an integer from a number; a script cannot).
fn luau_type(value: &LuaValue) -> &'static str {
    match value {
        LuaValue::Integer(_) => "number",
        other => other.type_name(),
    }
}

fn text_field(t: &Table, key: &str, what: &str, required: bool, max: usize) -> mlua::Result<Option<String>> {
    match t.get::<LuaValue>(key)? {
        LuaValue::Nil if required => Err(mlua::Error::runtime(format!("{what} needs `{key}`"))),
        LuaValue::Nil => Ok(None),
        LuaValue::String(s) => {
            let s = s.to_string_lossy();
            if required && (s.chars().count() > max || s.trim().is_empty()) {
                return Err(mlua::Error::runtime(format!(
                    "{what}: `{key}` must be 1 to {max} characters, not blank"
                )));
            }
            if s.chars().count() > max {
                return Err(mlua::Error::runtime(format!(
                    "{what}: `{key}` must be at most {max} characters"
                )));
            }
            Ok(Some(s))
        }
        other => {
            Err(mlua::Error::runtime(format!("{what}: `{key}` must be a string, got {}", luau_type(&other))))
        }
    }
}

/// The table of `app.notify { ... }` or `app:notify { ... }`: the last table given that is not `app` itself, so a
/// colon call with no table says it takes one instead of reading `app`'s own keys as a notification's.
fn notification_table(sh: &Shared, args: Variadic<LuaValue>) -> mlua::Result<Table> {
    let app = sh.app_table.lock().as_ref().map(Table::to_pointer);
    args.into_iter()
        .rev()
        .find_map(|v| match v {
            LuaValue::Table(t) if Some(t.to_pointer()) != app => Some(t),
            _ => None,
        })
        .ok_or_else(|| mlua::Error::runtime("app.notify takes a table"))
}

/// Refuse a key of `t` that is not in `allowed`, so a misspelt or unsupported field raises instead of being
/// dropped. The keys are read raw (no `__pairs`, no `__index`): what the table holds, not what it pretends to.
fn only_keys(t: &Table, what: &str, allowed: &[&str]) -> mlua::Result<()> {
    for pair in t.pairs::<LuaValue, LuaValue>() {
        let (key, _) = pair?;
        let known = match &key {
            LuaValue::String(s) => s.to_str().is_ok_and(|s| allowed.contains(&&*s)),
            _ => false,
        };
        if !known {
            let expected = allowed.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
            let message = match &key {
                LuaValue::String(s) => format!("unknown key `{}` (it takes {expected})", s.to_string_lossy()),
                // `f { "text" }`: a value written without a name lands at index 1
                LuaValue::Integer(i) => format!("unknown positional entry {i} (it takes {expected} by name)"),
                other => format!("unknown key of type {} (it takes {expected})", luau_type(other)),
            };
            return Err(mlua::Error::runtime(format!("{what}: {message}")));
        }
    }
    Ok(())
}

pub(super) fn install(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    // app:openWindow(name?, params?): an app has one window, the frame of its page, so the name may be left out
    let s = sh.clone();
    app.set(
        "openWindow",
        lua.create_function(
            move |_, (_this, name, params): (LuaValue, Option<String>, Option<LuaValue>)| {
                s.writable("app:openWindow")?;
                if let Some(name) = name.filter(|name| name != Shared::WINDOW) {
                    return Err(mlua::Error::runtime(format!(
                        "openWindow: an app has one window, `{}`; leave the name out (got `{name}`)",
                        Shared::WINDOW
                    )));
                }
                let params = match params {
                    Some(p) => lua_to_json(&p)?,
                    None => Value::Null,
                };
                s.push_effect(Effect::OpenWindow { name: Shared::WINDOW.to_owned(), params })
            },
        )?,
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

    // app.notify { title, body? } and app:notify { ... }: delivered now, to the clients connected now
    let s = sh.clone();
    app.set(
        "notify",
        lua.create_function(move |_, args: Variadic<LuaValue>| {
            s.writable("app.notify")?;
            let t = notification_table(&s, args)?;
            only_keys(&t, "app.notify", &["title", "body"])?;
            let title = text_field(&t, "title", "app.notify", true, 200)?.unwrap_or_default();
            let body = text_field(&t, "body", "app.notify", false, 2000)?;
            s.push_notification(Notification { app: s.app.clone(), title, body })
        })?,
    )?;

    install_emit(lua, sh, app)?;
    install_agents(lua, sh, app)
}

/// The longest event name `app.emit` takes.
const MAX_EVENT_NAME: usize = 64;
/// The most JSON bytes of data one event carries: it travels to every open client of the app.
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
/// The most events that may wait to be sent: a loop that emits faster than the host sends gets an error, not an
/// unbounded queue.
const MAX_QUEUED_EVENTS: usize = 256;

/// `app.emit(name, data?)`: tell the app's open pages something changed outside them (`host.on(name, fn)` in the
/// page). It is a change to the world like `app.notify`, so a `readOnly` tool may not.
fn install_emit(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    let s = sh.clone();
    app.set(
        "emit",
        lua.create_function(move |_, (name, data): (String, Option<LuaValue>)| {
            s.writable("app.emit")?;
            let valid = !name.is_empty()
                && name.len() <= MAX_EVENT_NAME
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
            if !valid {
                return Err(mlua::Error::runtime(format!(
                    "app.emit: an event name is 1 to {MAX_EVENT_NAME} letters, digits, . _ : or -"
                )));
            }
            if name.starts_with("host:") {
                return Err(mlua::Error::runtime("app.emit: names starting with `host:` are the host's own"));
            }
            let data = match data {
                Some(d) => lua_to_json(&d)?,
                None => Value::Null,
            };
            if data.to_string().len() > MAX_EVENT_BYTES {
                return Err(mlua::Error::runtime(format!(
                    "app.emit: the data of an event is at most {MAX_EVENT_BYTES} bytes of JSON"
                )));
            }
            let mut effects = s.effects.lock();
            let waiting = effects.iter().filter(|e| matches!(e, Effect::Emit { .. })).count();
            if waiting >= MAX_QUEUED_EVENTS {
                return Err(mlua::Error::runtime(format!(
                    "app.emit: {MAX_QUEUED_EVENTS} events are already waiting to be sent"
                )));
            }
            effects.push(Effect::Emit { name, data });
            Ok(())
        })?,
    )
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
