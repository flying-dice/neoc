//! `app.id`, `app.openWindow`, `app.notify`, `app.emit`, `app.agents`.

use super::{json::Json, shared};
use crate::vm::{Effect, Shared};
use mlua_ir::{IntoLuaRet, Object, Registry, Shape, Unknown};
use neoc_vm::mlua::{self, IntoLua as _, Lua, Value as LuaValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// `app.id`: the id of the app whose VM the module is built in. A module's constant is a value of the Rust
/// program; this one is read from the VM when the module's table is made, and declared a string.
#[derive(Clone, Copy, Debug)]
pub struct ThisApp;

impl Shape for ThisApp {
    fn shape(reg: &mut Registry) -> mlua_ir::ir::Ty {
        String::shape(reg)
    }
}

impl IntoLuaRet for ThisApp {
    fn into_lua_ret(self, lua: &Lua, _: &'static str) -> mlua::Result<LuaValue> {
        shared(lua)?.app.as_str().into_lua(lua)
    }
}

mlua_ir::unknown_reason!(
    /// Why the data of a window or an event has no declared shape.
    PageData = "JSON the app hands its own page: its shape is the app's"
);

/// What `app.openWindow` hands the page: named JSON values.
pub type WindowParams = Json<BTreeMap<String, Unknown<PageData>>>;

/// The data of an event: any JSON value.
pub type EventData = Json<Unknown<PageData>>;

pub(super) fn open_window(sh: &Shared, params: Option<WindowParams>) -> mlua::Result<()> {
    sh.writable("app.openWindow")?;
    let params = match params.map(|params| params.value) {
        None => Value::Null,
        Some(named @ Value::Object(_)) => named,
        // an empty table reads as `[]`: no parameters
        Some(Value::Array(none)) if none.is_empty() => Value::Null,
        // an app has one window, so there is no name to give: `app.openWindow("settings")` is a mistake
        Some(_) => return Err(mlua::Error::runtime(
            "app.openWindow: `params` is a table of named values (an app has one window: it takes no name)",
        )),
    };
    sh.push_effect(Effect::OpenWindow { name: Shared::WINDOW.to_owned(), params })
}

/// A notification: a title and an optional body, and no other key. The host adds which app sent it.
#[derive(Clone, Debug, Serialize, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(module = "vos:app")]
pub struct Notification {
    /// 1 to 200 characters, not blank.
    pub title: String,
    /// Up to 2000 characters.
    pub body: Option<String>,
}

/// The longest title a notification takes, in characters.
const MAX_TITLE: usize = 200;
/// The longest body a notification takes, in characters.
const MAX_BODY: usize = 2000;

pub(super) fn notify(sh: &Shared, notification: Notification) -> mlua::Result<()> {
    sh.writable("app.notify")?;
    let Notification { title, body } = notification;
    if title.chars().count() > MAX_TITLE || title.trim().is_empty() {
        return Err(mlua::Error::runtime(format!(
            "app.notify: `title` must be 1 to {MAX_TITLE} characters, not blank"
        )));
    }
    if body.as_ref().is_some_and(|body| body.chars().count() > MAX_BODY) {
        return Err(mlua::Error::runtime(format!(
            "app.notify: `body` must be at most {MAX_BODY} characters"
        )));
    }
    sh.push_notification(vos_domain::Notification { app: sh.app.clone(), title, body })
}

/// The longest event name `app.emit` takes.
const MAX_EVENT_NAME: usize = 64;
/// The most JSON bytes of data one event carries: it travels to every open client of the app.
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
/// The most events that may wait to be sent: a loop that emits faster than the host sends gets an error, not an
/// unbounded queue.
const MAX_QUEUED_EVENTS: usize = 256;

/// `app.emit(name, data?)`: it is a change to the world like `app.notify`, so a `readOnly` tool may not.
pub(super) fn emit(sh: &Shared, name: String, data: Option<EventData>) -> mlua::Result<()> {
    sh.writable("app.emit")?;
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
    let data = data.map_or(Value::Null, |data| data.value);
    if data.to_string().len() > MAX_EVENT_BYTES {
        return Err(mlua::Error::runtime(format!(
            "app.emit: the data of an event is at most {MAX_EVENT_BYTES} bytes of JSON"
        )));
    }
    let mut effects = sh.effects.lock();
    let waiting = effects.iter().filter(|e| matches!(e, Effect::Emit { .. })).count();
    if waiting >= MAX_QUEUED_EVENTS {
        return Err(mlua::Error::runtime(format!(
            "app.emit: {MAX_QUEUED_EVENTS} events are already waiting to be sent"
        )));
    }
    effects.push(Effect::Emit { name, data });
    Ok(())
}

/// What an app may know of an agent contact: never grants, tokens or launch commands.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct AgentCard {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    pub role: String,
}

/// The agent contacts of the host.
#[derive(Clone, Copy, Debug, Object)]
#[shape(module = "vos:app")]
pub struct Agents;

#[mlua_ir::object(module = "vos:app")]
impl Agents {
    /// Every agent contact.
    pub fn list(&self, lua: &Lua) -> mlua::Result<Vec<AgentCard>> {
        let cards = shared(lua)?.roster.cards();
        Ok(cards
            .into_iter()
            .map(|card| AgentCard { id: card.id.0, name: card.name, avatar: card.avatar, role: card.role })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_ir::FromLuaArg as _;

    /// `app.id` is declared a string, not a type of its own.
    #[test]
    fn the_id_is_declared_a_string() {
        assert_eq!(ThisApp::shape(&mut Registry::new()), mlua_ir::ir::Ty::String);
        // and what a notification refuses is refused by its shape: a key it does not have
        let lua = Lua::new();
        let table: LuaValue = lua.load("return { title = 'x', at = 'later' }").eval().unwrap();
        let refused = Notification::from_lua_arg(table, &lua).unwrap_err();
        assert!(refused.contains("at"), "{refused}");
    }
}
