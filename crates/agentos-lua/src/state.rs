//! `app.state(initial)`: shared in-memory server state with **tracked reads**.
//!
//! Reading a field while a tracking scope is open (a render) records the dependency; assigning a field
//! marks it dirty. The view layer asks which fields a render read and which fields changed since, and
//! re-renders the instances that read a dirty one.
//!
//! Values are held as Lua values (functions and tables are fine), but only assignment to a field is
//! tracked: change a nested table by assigning the field again.

use agentos_luavm::mlua;
use agentos_luavm::mlua::{Function, Lua, Table, Value as LuaValue};
use parking_lot::Mutex;
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// One tracked field: which `app.state` table, which key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DepKey {
    pub state: u32,
    pub key: String,
}

pub(crate) struct Tracker {
    /// The running call is a `readOnly` tool (the VM's flag): assigning a state field would write behind
    /// its guarantee.
    read_only: Arc<AtomicBool>,
    next_state: u32,
    /// Reads recorded since `begin`, while a scope is open.
    active: Option<BTreeSet<DepKey>>,
    /// Fields assigned since the last `take_dirty`.
    dirty: BTreeSet<DepKey>,
    /// The open scope is a render: it must not change anything.
    pure: bool,
}

impl Tracker {
    pub fn new(read_only: Arc<AtomicBool>) -> Self {
        Self { read_only, next_state: 0, active: None, dirty: BTreeSet::new(), pure: false }
    }
    pub fn begin(&mut self, pure: bool) {
        self.active = Some(BTreeSet::new());
        self.pure = pure;
    }
    pub fn end(&mut self) -> BTreeSet<DepKey> {
        self.pure = false;
        self.active.take().unwrap_or_default()
    }
    pub fn take_dirty(&mut self) -> BTreeSet<DepKey> {
        std::mem::take(&mut self.dirty)
    }
    pub fn is_pure(&self) -> bool {
        self.pure
    }
}

pub(crate) type SharedTracker = Arc<Mutex<Tracker>>;

fn key_of(k: &LuaValue) -> mlua::Result<String> {
    match k {
        LuaValue::String(s) => Ok(s.to_string_lossy()),
        LuaValue::Integer(i) => Ok(i.to_string()),
        LuaValue::Number(n) => Ok(n.to_string()),
        other => Err(mlua::Error::runtime(format!("app.state keys are strings, got {}", other.type_name()))),
    }
}

/// `app.state { ... }` -> a proxy table over the initial values.
pub(crate) fn state_constructor(lua: &Lua, tracker: SharedTracker) -> mlua::Result<Function> {
    lua.create_function(move |lua, (first, second): (LuaValue, Option<Table>)| {
        // `app.state { ... }` passes the table first; `app:state { ... }` would pass `app` before it
        let initial = match (first, second) {
            (_, Some(t)) | (LuaValue::Table(t), None) => t,
            _ => lua.create_table()?,
        };
        make_state(lua, &tracker, &initial)
    })
}

/// A tracked state proxy over a copy of `initial` (`app.state`, and each window's `w.state`).
pub(crate) fn make_state(lua: &Lua, tracker: &SharedTracker, initial: &Table) -> mlua::Result<Table> {
    let tracker = tracker.clone();
    let id = {
        let mut t = tracker.lock();
        t.next_state += 1;
        t.next_state
    };
    {
        let data = lua.create_table()?;
        for pair in initial.pairs::<LuaValue, LuaValue>() {
            let (k, v) = pair?;
            data.raw_set(k, v)?;
        }

        let meta = lua.create_table()?;
        let (read_data, read_tracker) = (data.clone(), tracker.clone());
        meta.set(
            "__index",
            lua.create_function(move |_, (_proxy, k): (LuaValue, LuaValue)| {
                let key = key_of(&k)?;
                if let Some(active) = read_tracker.lock().active.as_mut() {
                    active.insert(DepKey { state: id, key });
                }
                read_data.raw_get::<LuaValue>(k)
            })?,
        )?;
        let (write_data, write_tracker) = (data, tracker.clone());
        meta.set(
            "__newindex",
            lua.create_function(move |_, (_proxy, k, v): (LuaValue, LuaValue, LuaValue)| {
                let key = key_of(&k)?;
                let mut t = write_tracker.lock();
                if t.pure {
                    return Err(mlua::Error::runtime(
                        "app.state cannot be changed while rendering: render must be pure",
                    ));
                }
                if t.read_only.load(Ordering::SeqCst) {
                    return Err(mlua::Error::runtime(
                        "app.state cannot be changed in a tool declared readOnly: a readOnly tool cannot write",
                    ));
                }
                let old = write_data.raw_get::<LuaValue>(k.clone())?;
                if !old.equals(&v)? {
                    t.dirty.insert(DepKey { state: id, key });
                }
                drop(t);
                write_data.raw_set(k, v)
            })?,
        )?;
        meta.set("__metatable", "app.state")?;
        let proxy = lua.create_table()?;
        proxy.set_metatable(Some(meta))?;
        Ok(proxy)
    }
}
