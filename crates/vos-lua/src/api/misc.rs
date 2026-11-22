//! `app.manifest`, `app.window`, `app:openWindow`, `app:windows`, `app.log`, `app.notify`, `app.shell`.

use super::{check_effects_allowed, push_shell};
use crate::{
    convert::lua_to_json,
    vm::{Effect, Shared, ShellEffect},
};
use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua, Table, Value as LuaValue, Variadic};
use serde_json::Value;
use std::sync::Arc;
use vos_domain::{AppDecl, Notification};

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

/// The order windows were opened in: the application layer numbers a view `v<n>` as it opens it, so the number is
/// the order (`v2` before `v10`, which comparing the ids as text would get wrong). An id that is not `v<n>` sorts
/// after the numbered ones, by its text.
fn open_order(view: &str) -> (u64, &str) {
    (view.strip_prefix('v').and_then(|n| n.parse().ok()).unwrap_or(u64::MAX), view)
}

/// `app:windows(name)`: the open instances of a declared window, in the order they were opened, each the `w` its
/// render got (an app reaches the windows it has open: a tool empties their announcers, say). Listing is a read: a
/// `readOnly` tool may, and what it does to a `w` is that `w`'s own call, refused the same way.
fn install_windows(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    let s = sh.clone();
    app.set(
        "windows",
        lua.create_function(move |lua, (_this, name): (LuaValue, String)| {
            s.declared_window("app:windows", &name)?;
            let mut open: Vec<(String, Table)> = s
                .views
                .lock()
                .iter()
                .filter(|(_, instance)| instance.window == name)
                .map(|(view, instance)| (view.clone(), instance.w.clone()))
                .collect();
            open.sort_by(|a, b| open_order(&a.0).cmp(&open_order(&b.0)));
            lua.create_sequence_from(open.into_iter().map(|(_, w)| w))
        })?,
    )
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
            s.declared_window("openWindow", &name)?;
            let params = match params {
                Some(p) => lua_to_json(&p)?,
                None => Value::Null,
            };
            s.push_effect(Effect::OpenWindow { name, params })
        })?,
    )?;

    install_windows(lua, sh, app)?;

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
            check_effects_allowed(&s, "app.notify")?;
            s.writable("app.notify")?;
            let t = notification_table(&s, args)?;
            only_keys(&t, "app.notify", &["title", "body"])?;
            let title = text_field(&t, "title", "app.notify", true, 200)?.unwrap_or_default();
            let body = text_field(&t, "body", "app.notify", false, 2000)?;
            s.push_notification(Notification { app: s.app.clone(), title, body })
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

/// `app.shell`: deep links, accepted and dropped until the shell shows them.
fn install_shell(lua: &Lua, sh: &Arc<Shared>, app: &Table) -> mlua::Result<()> {
    let shell = lua.create_table()?;

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

#[cfg(test)]
mod tests {
    use super::open_order;

    /// Views sort by their number, so `v2` comes before `v10`; an id that is not `v<n>` sorts after them, by
    /// its text.
    #[test]
    fn numbered_views_sort_by_number_and_other_ids_come_last_by_text() {
        let mut ids = ["v10", "zeta", "v2", "alpha", "v1", "v", "v1x", "v-3"];
        ids.sort_by(|a, b| open_order(a).cmp(&open_order(b)));
        assert_eq!(ids, ["v1", "v2", "v10", "alpha", "v", "v-3", "v1x", "zeta"]);
    }
}
