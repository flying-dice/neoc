//! `app.state(initial)`: shared in-memory server state with **tracked reads**.
//!
//! Reading a field while a tracking scope is open (a render) records the dependency; assigning a field
//! marks it dirty. The view layer asks which fields a render read and which fields changed since, and
//! re-renders the instances that read a dirty one.
//!
//! Values are held as Lua values (functions and tables are fine), but only assignment to a field is
//! tracked: change a nested table by assigning the field again.

use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua, Table, Value as LuaValue};
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
    /// Which scope is open (`0`: none), so that closing a scope that was dropped while its call waited cannot
    /// end the scope of the call that took its place.
    open_id: u64,
    scopes_opened: u64,
}

impl Tracker {
    pub fn new(read_only: Arc<AtomicBool>) -> Self {
        Self {
            read_only,
            next_state: 0,
            active: None,
            dirty: BTreeSet::new(),
            pure: false,
            open_id: 0,
            scopes_opened: 0,
        }
    }
    /// Open a scope; returns its id (for [`Tracker::end_scope`]).
    pub fn begin(&mut self, pure: bool) -> u64 {
        self.active = Some(BTreeSet::new());
        self.pure = pure;
        self.scopes_opened += 1;
        self.open_id = self.scopes_opened;
        self.open_id
    }
    pub fn end(&mut self) -> BTreeSet<DepKey> {
        self.pure = false;
        self.open_id = 0;
        self.active.take().unwrap_or_default()
    }
    /// Close scope `id` if it is the open one (nothing otherwise: it was set aside, or already closed).
    pub fn end_scope(&mut self, id: u64) -> BTreeSet<DepKey> {
        if self.open_id == id {
            self.end()
        } else {
            BTreeSet::new()
        }
    }
    #[cfg(test)]
    pub fn is_tracking(&self) -> bool {
        self.active.is_some()
    }
    /// Has any field been assigned since the last `take_dirty`?
    pub fn has_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }
    pub fn take_dirty(&mut self) -> BTreeSet<DepKey> {
        std::mem::take(&mut self.dirty)
    }
    pub fn is_pure(&self) -> bool {
        self.pure
    }
    /// Set the open scope aside (the call waits and another runs): see [`TrackerScope`].
    pub fn suspend(&mut self) -> TrackerScope {
        TrackerScope {
            active: self.active.take(),
            pure: std::mem::take(&mut self.pure),
            id: std::mem::take(&mut self.open_id),
        }
    }
    pub fn resume(&mut self, scope: TrackerScope) {
        self.active = scope.active;
        self.pure = scope.pure;
        self.open_id = scope.id;
    }
}

/// A tracking scope set aside while its call waits.
pub(crate) struct TrackerScope {
    active: Option<BTreeSet<DepKey>>,
    pure: bool,
    id: u64,
}

/// An open tracking scope of a call. However the call ends (finished, failed, or its future dropped at a host
/// call because the caller gave up), dropping the guard closes the scope, so a cancelled call cannot leave the
/// tracker recording reads for the calls that follow.
pub(crate) struct TrackGuard {
    tracker: SharedTracker,
    id: u64,
}

impl TrackGuard {
    pub fn begin(tracker: &SharedTracker) -> Self {
        let id = tracker.lock().begin(false);
        Self { tracker: tracker.clone(), id }
    }

    /// Close the scope: what the call read.
    pub fn finish(self) -> BTreeSet<DepKey> {
        self.tracker.lock().end_scope(self.id)
    }
}

impl Drop for TrackGuard {
    fn drop(&mut self) {
        self.tracker.lock().end_scope(self.id);
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
                {
                    let t = write_tracker.lock();
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
                }
                // `equals` can run an app's `__eq`, which may read `app.state` and so lock the tracker again:
                // compare with the lock released
                let old = write_data.raw_get::<LuaValue>(k.clone())?;
                if !old.equals(&v)? {
                    write_tracker.lock().dirty.insert(DepKey { state: id, key });
                }
                write_data.raw_set(k, v)
            })?,
        )?;
        meta.set("__metatable", "app.state")?;
        let proxy = lua.create_table()?;
        proxy.set_metatable(Some(meta))?;
        Ok(proxy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> SharedTracker {
        Arc::new(Mutex::new(Tracker::new(Arc::new(AtomicBool::new(false)))))
    }

    #[test]
    fn a_dropped_guard_closes_its_scope() {
        let tracker = tracker();
        let guard = TrackGuard::begin(&tracker);
        assert!(tracker.lock().is_tracking());
        drop(guard); // the call was cancelled
        assert!(!tracker.lock().is_tracking());
    }

    #[test]
    fn a_finished_guard_returns_what_was_read_and_closes_once() {
        let tracker = tracker();
        let guard = TrackGuard::begin(&tracker);
        tracker.lock().active.as_mut().unwrap().insert(DepKey { state: 1, key: "v".into() });
        let deps = guard.finish();
        assert_eq!(deps.len(), 1);
        assert!(!tracker.lock().is_tracking());
    }

    /// A call dropped while it waits (its scope was set aside) must not end the scope of the call that took its
    /// place.
    #[test]
    fn a_guard_dropped_while_its_call_waits_leaves_the_next_calls_scope_alone() {
        let tracker = tracker();
        let first = TrackGuard::begin(&tracker);
        let set_aside = tracker.lock().suspend(); // the first call waits
        let second = TrackGuard::begin(&tracker);
        drop(first); // cancelled while waiting
        assert!(tracker.lock().is_tracking(), "the second call's scope is still open");
        let deps = second.finish();
        assert!(deps.is_empty());
        assert!(!tracker.lock().is_tracking());
        drop(set_aside);
    }
}
