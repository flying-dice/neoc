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
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use mlua::{Function, Lua, LuaSerdeExt, MultiValue, Table, UserData, UserDataMethods, Value};
use parking_lot::RwLock;
use tracing::instrument;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    // `require("std:collections").Map.new()`
    let map_t = lua.create_table()?;
    map_t.set("new", lua.create_function(make_map)?)?;
    t.set("Map", map_t)?;

    // `require("std:collections").Counter.new(initial?)`
    let counter_t = lua.create_table()?;
    counter_t.set("new", lua.create_function(make_counter)?)?;
    t.set("Counter", counter_t)?;

    // `require("std:collections").Mutex.new()`
    let mutex_t = lua.create_table()?;
    mutex_t.set("new", lua.create_function(make_mutex)?)?;
    t.set("Mutex", mutex_t)?;

    Ok(t)
}

#[instrument(name = "Map.new", target = "luau::lib::collections", skip_all)]
fn make_map(_: &Lua, _: ()) -> mlua::Result<Map> {
    Ok(Map::from_arc(Arc::new(SharedMap::default())))
}

#[instrument(name = "Counter.new", target = "luau::lib::collections", skip_all)]
fn make_counter(_: &Lua, initial: Option<i64>) -> mlua::Result<Counter> {
    let c = Counter::from_arc(Arc::new(SharedCounter::default()));
    if let Some(v) = initial {
        c.0.value.store(v, Ordering::Relaxed);
    }
    Ok(c)
}

#[instrument(name = "Mutex.new", target = "luau::lib::collections", skip_all)]
fn make_mutex(_: &Lua, _: ()) -> mlua::Result<Mutex> {
    Ok(Mutex::from_arc(Arc::new(SharedMutex::default())))
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

impl Map {
    pub fn from_arc(arc: Arc<SharedMap>) -> Self {
        Self(arc)
    }
    pub fn arc(&self) -> Arc<SharedMap> {
        self.0.clone()
    }
}

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

        methods.add_method("delete", |_, this, k: String| {
            Ok(this.0.inner.write().remove(&k).is_some())
        });

        methods.add_method("has", |_, this, k: String| {
            Ok(this.0.inner.read().contains_key(&k))
        });

        methods.add_method(
            "size",
            |_, this, _: ()| Ok(this.0.inner.read().len() as i32),
        );

        methods.add_method("keys", |lua, this, _: ()| {
            let keys: Vec<String> = this.0.inner.read().keys().cloned().collect();
            let t = lua.create_table_with_capacity(keys.len(), 0)?;
            for (i, k) in keys.into_iter().enumerate() {
                t.set(i + 1, k)?;
            }
            Ok(t)
        });

        methods.add_method("clear", |_, this, _: ()| {
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

impl Counter {
    pub fn from_arc(arc: Arc<SharedCounter>) -> Self {
        Self(arc)
    }
    pub fn arc(&self) -> Arc<SharedCounter> {
        self.0.clone()
    }
}

fn i64_to_lua(v: i64) -> Value {
    // mlua::Integer is i32 under luau — same overflow handling as the DB modules.
    match i32::try_from(v) {
        Ok(i) => Value::Integer(i),
        Err(_) => Value::Number(v as f64),
    }
}

// `Counter` implements `Sendable` so it can be passed to spawned workers
// via `thread.spawn({ shared = { c = counter } })`.  The shared state is
// the inner `Arc<SharedCounter>`'s atomic — every worker that receives the
// counter sees increments from all others.
impl crate::lua::std::thread::Sendable for Counter {
    fn as_shared(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.0.clone()
    }
    fn from_shared(arc: &Arc<dyn std::any::Any + Send + Sync>) -> Option<Self> {
        arc.clone()
            .downcast::<SharedCounter>()
            .ok()
            .map(Counter::from_arc)
    }
}

impl UserData for Counter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, _: ()| {
            Ok(i64_to_lua(this.0.value.load(Ordering::Relaxed)))
        });

        methods.add_method("increment", |_, this, _: ()| {
            Ok(i64_to_lua(this.0.value.fetch_add(1, Ordering::Relaxed) + 1))
        });

        methods.add_method("decrement", |_, this, _: ()| {
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

impl Mutex {
    pub fn from_arc(arc: Arc<SharedMutex>) -> Self {
        Self(arc)
    }
    pub fn arc(&self) -> Arc<SharedMutex> {
        self.0.clone()
    }
}

impl UserData for Mutex {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `with_lock(fn)` — acquire the lock, call `fn` with no args, release
        // even if `fn` errors, and return whatever `fn` returned.
        methods.add_async_method("with_lock", |_, this, f: Function| {
            let mu = this.0.clone();
            async move {
                let _guard = mu.inner.lock().await;
                f.call_async::<MultiValue>(()).await
            }
        });
    }
}
