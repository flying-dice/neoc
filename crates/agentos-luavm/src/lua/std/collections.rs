//! Concurrent data structures — `std:collections`.
//!
//! Constructors return userdata wrapping `Send + Sync` Arc-shared state.
//! Anything constructed here is worker-local until explicitly shared via
//! `thread.spawn({ shared = {…} })`.
//!
//! ```lua
//! local Map     = require("std:collections").Map
//! local Counter = require("std:collections").Counter
//!
//! -- worker-local
//! local cache = Map.new()
//! cache:set("k", "v")
//!
//! -- shared via spawn-time handoff
//! local hits = Counter.new(0)
//! thread.spawn({ file = "worker.lua", shared = { hits = hits } })
//! ```
//!
//! Map values round-trip via `serde_json::Value` so they're decoupled from
//! any single VM's `mlua::Value` (which is bound to that VM's state).

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use mlua::{Function, Lua, LuaSerdeExt, MultiValue, Table, UserData, UserDataMethods, Value};
use parking_lot::RwLock;

use crate::lua::facade::Module;
use crate::lua::sandbox::i64_to_lua;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:collections` surface through the facade. Each type is a
/// constructor sub-table (`Map.new()`, `Counter.new(initial?)`, `Mutex.new()`)
/// whose `new` span (`"<Type>.new"` at `lua::lib::collections`) and registered
/// Lua surface are byte-identical to the hand-written registration this
/// replaces; the instance methods stay in each type's `impl UserData` and are
/// recorded here for the header only.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:collections")?;

    // `require("std:collections").Map.new()`
    m.type_("Map", |t| {
        t.constructor("new", &[], "Construct an empty concurrent map.", make_map)?
            .method("get", &[("key", "string")], "LuaValue", "Read a key.")
            .method("set", &[("key", "string"), ("value", "LuaValue")], "()", "Insert or replace a key.")
            .method("delete", &[("key", "string")], "bool", "Remove a key, returning whether it was present.")
            .method("has", &[("key", "string")], "bool", "Test for a key.")
            .method("size", &[], "number", "Number of entries.")
            .method("keys", &[], "LuaValue", "All keys as a 1-indexed table.")
            .method("clear", &[], "()", "Remove all entries.");
        Ok(())
    })?
    // `require("std:collections").Counter.new(initial?)`
    .type_("Counter", |t| {
        t.constructor(
            "new",
            &[("initial", "number?")],
            "Construct a counter, optionally seeded with an initial value.",
            make_counter,
        )?
        .method("get", &[], "number", "Current value.")
        .method("increment", &[], "number", "Add one, returning the new value.")
        .method("decrement", &[], "number", "Subtract one, returning the new value.")
        .method("add", &[("n", "number")], "number", "Add `n`, returning the new value.")
        .method("set", &[("n", "number")], "()", "Set the value to `n`.");
        Ok(())
    })?
    // `require("std:collections").Mutex.new()`
    .type_("Mutex", |t| {
        t.constructor("new", &[], "Construct a coordination mutex.", make_mutex)?.method(
            "with_lock",
            &[("f", "LuaValue")],
            "LuaValue",
            "Acquire the lock, call `f`, release, and return `f`'s result.",
        );
        Ok(())
    })?;

    Ok(m)
}

fn make_map(_: &Lua, _: ()) -> mlua::Result<Map> {
    Ok(Map::from_arc(Arc::new(SharedMap::default())))
}

fn make_counter(_: &Lua, initial: Option<i64>) -> mlua::Result<Counter> {
    let c = Counter::from_arc(Arc::new(SharedCounter::default()));
    if let Some(v) = initial {
        c.0.value.store(v, Ordering::Relaxed);
    }
    Ok(c)
}

fn make_mutex(_: &Lua, _: ()) -> mlua::Result<Mutex> {
    Ok(Mutex::from_arc(Arc::new(SharedMutex::default())))
}

/// A shared-state newtype is a thin `Arc<Shared*>` wrapper, rebuilt from a raw
/// `Arc` when it crosses a worker boundary. Generates the one accessor every
/// such type needs — `from_arc` — shared by `Map`, `Counter`, and `Mutex`.
macro_rules! arc_newtype {
    ($ty:ident, $shared:ty) => {
        impl $ty {
            pub fn from_arc(arc: Arc<$shared>) -> Self {
                Self(arc)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// SharedMap
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SharedMap {
    inner: RwLock<HashMap<String, serde_json::Value>>,
}

#[derive(Clone)]
pub struct Map(Arc<SharedMap>);

arc_newtype!(Map, SharedMap);

impl crate::lua::std::thread::Sendable for Map {
    fn as_shared(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.0.clone()
    }
    fn from_shared(arc: &Arc<dyn std::any::Any + Send + Sync>) -> Option<Self> {
        arc.clone().downcast::<SharedMap>().ok().map(Map::from_arc)
    }
}

impl UserData for Map {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("set", |_, this, (k, v): (String, Value)| {
            let json = serde_json::to_value(&v).map_err(mlua::Error::external)?;
            this.0.inner.write().insert(k, json);
            Ok(())
        });

        methods.add_method("get", |lua, this, k: String| {
            let v = this.0.inner.read().get(&k).cloned();
            match v {
                Some(json) => lua.to_value(&json),
                None => Ok(Value::Nil),
            }
        });

        methods.add_method("delete", |_, this, k: String| Ok(this.0.inner.write().remove(&k).is_some()));

        methods.add_method("has", |_, this, k: String| Ok(this.0.inner.read().contains_key(&k)));

        methods.add_method("size", |_, this, (): ()| Ok(this.0.inner.read().len() as i32));

        methods.add_method("keys", |lua, this, (): ()| {
            let keys: Vec<String> = this.0.inner.read().keys().cloned().collect();
            let t = lua.create_table_with_capacity(keys.len(), 0)?;
            for (i, k) in keys.into_iter().enumerate() {
                t.set(i + 1, k)?;
            }
            Ok(t)
        });

        methods.add_method("clear", |_, this, (): ()| {
            this.0.inner.write().clear();
            Ok(())
        });
    }
}

// ---------------------------------------------------------------------------
// SharedCounter
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SharedCounter {
    value: AtomicI64,
}

#[derive(Clone)]
pub struct Counter(Arc<SharedCounter>);

arc_newtype!(Counter, SharedCounter);

// `Counter` implements `Sendable` so it can be passed to spawned workers
// via `thread.spawn({ shared = { c = counter } })`.  The shared state is
// the inner `Arc<SharedCounter>`'s atomic — every worker that receives the
// counter sees increments from all others.
impl crate::lua::std::thread::Sendable for Counter {
    fn as_shared(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.0.clone()
    }
    fn from_shared(arc: &Arc<dyn std::any::Any + Send + Sync>) -> Option<Self> {
        arc.clone().downcast::<SharedCounter>().ok().map(Counter::from_arc)
    }
}

impl UserData for Counter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, (): ()| Ok(i64_to_lua(this.0.value.load(Ordering::Relaxed))));

        methods.add_method("increment", |_, this, (): ()| {
            Ok(i64_to_lua(this.0.value.fetch_add(1, Ordering::Relaxed) + 1))
        });

        methods.add_method("decrement", |_, this, (): ()| {
            Ok(i64_to_lua(this.0.value.fetch_sub(1, Ordering::Relaxed) - 1))
        });

        methods.add_method("add", |_, this, n: i64| {
            Ok(i64_to_lua(this.0.value.fetch_add(n, Ordering::Relaxed) + n))
        });

        methods.add_method("set", |_, this, n: i64| {
            this.0.value.store(n, Ordering::Relaxed);
            Ok(())
        });
    }
}

// ---------------------------------------------------------------------------
// SharedMutex (coordination gate — no protected data)
// ---------------------------------------------------------------------------

/// Tokio mutex so awaiting it yields the calling Lua coroutine instead of
/// blocking the worker thread — other coroutines (and other VMs sharing the
/// runtime) keep making progress while one VM holds the lock.
#[derive(Default)]
pub struct SharedMutex {
    inner: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct Mutex(Arc<SharedMutex>);

arc_newtype!(Mutex, SharedMutex);

impl UserData for Mutex {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `with_lock(fn)` — acquire the lock, call `fn` with no args, release
        // even if `fn` errors, and return whatever `fn` returned.
        methods.add_async_method("with_lock", |_, this, f: Function| {
            let mu = this.0.clone();
            async move {
                // waiting for the lock does not use the VM (and the holder may need it to finish)
                let _guard = crate::lua::lifecycle::idle(mu.inner.lock()).await?;
                f.call_async::<MultiValue>(()).await
            }
        });
    }
}
