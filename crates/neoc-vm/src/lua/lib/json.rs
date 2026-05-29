//! JSON value handles — `lib:json`.
//!
//! Foundation that `vnd:serde_json::from_str` returns: a parsed JSON body
//! lives as one `JsonValue` tree in Rust memory (built out of `String`,
//! `Vec`, `BTreeMap`, `i64`, `f64`, `bool` — pure std), addressed from Lua
//! via `Object` / `Array` userdata that share an `Arc<Root>`. Reading
//! `req.body` and parsing 100 KiB of JSON allocates one Rust tree, not
//! ten thousand Lua table entries — that's the GC-pressure point.
//!
//! `lib:json` has no dependency on `serde_json` or any other vendored
//! crate; the `JsonValue` enum is the canonical in-memory shape. This
//! module is purely the data structure plus its Lua userdata view —
//! parsing and JSON-text encoding live in `vnd:serde_json` (which bridges
//! `serde_json::Value` ↔ `JsonValue` and returns these handles).
//!
//! ```lua
//! local json       = require("lib:json")
//! local serde_json = require("vnd:serde_json")
//!
//! local body = serde_json.from_str('{"name":"Alice","tags":["a","b"]}')
//! body:get("name")             -- "Alice"  (Lua string, materialised on access)
//! body:get("tags"):get(1)      -- "a"
//! body:get("tags"):len()       -- 2
//! body:keys()                  -- { "name", "tags" }
//! body:to_table()              -- materialise the whole tree as a Lua table
//! serde_json.to_string(body)   -- re-emit (encoding is serde_json's job)
//!
//! -- Build from scratch:
//! local o = json.object()
//! o:set("x", 1)
//! o:set("y", 2)
//! ```
//!
//! ## No `obj.x` sugar
//!
//! Object/Array are **userdata of dynamic JSON shape** — there is no
//! compile-time way for Luau strict mode to know what fields exist or
//! what types they are. Pretending otherwise via `__index` would just
//! mask type errors. Access is always through `:get(...)` / `:set(...)` /
//! `:len()` so the dynamic boundary is explicit at the call site.
//!
//! Scalar leaves (string / integer / number / boolean / null) come out as
//! Lua-native values on read. Objects and Arrays stay as userdata until
//! you ask for `:to_table()` or fall off into a leaf via field/index
//! access.
//!
//! ## Mutation
//!
//! `obj:set(k, v)` walks the path and mutates in place.
//! Two handles built from the same root observe each other's mutations.
//! A handle from `json.object()` / `json.array()` starts a fresh root.
//!
//! ## Threading
//!
//! `Root` wraps the value in `parking_lot::RwLock<JsonValue>` because
//! mlua's `UserData` trait requires `Send + Sync`. JSON handles are
//! single-engine — they are not Sendable and cannot be passed in
//! `thread.spawn({ shared = {...} })`. The `RwLock` satisfies the
//! type-level constraint while also enabling safe concurrent access
//! if cross-engine sharing is re-introduced later.
//! Read-side methods (`:get`, `:has`, `:keys`, `:len`,
//! `:to_table`) take the read lock; writers (`:set`, `:push`) take
//! the write lock.

use std::collections::BTreeMap;
use std::sync::Arc;

use mlua::{
    AnyUserData, IntoLua, Lua, MetaMethod, MultiValue, Table, UserData, UserDataMethods, Value,
};
use parking_lot::RwLock;

use crate::lua::facade::Module;

// ---------------------------------------------------------------------------
// JsonValue — pure-std representation
// ---------------------------------------------------------------------------

/// In-memory JSON value, expressed entirely in Rust standard library
/// types so this module has zero dependency on `serde_json` (or any
/// other vendored crate).
///
/// ### Why a custom enum instead of `serde_json::Value`?
///
/// `serde_json::Value` would have been a one-line solution, but it
/// would tie the `lib:json` storage shape to one specific vendored
/// crate. That's the wrong direction — `vnd:serde_json` is the JSON
/// *codec* that sits **on top** of `lib:json`, not the other way
/// around. Any future codec (msgpack, cbor, etc.) bridges into the
/// same `JsonValue` representation by walking it; nothing else in this
/// module reaches into a third-party type.
///
/// ### Variant choices
///
/// - `Null` / `Bool(bool)` — the trivial JSON scalars; tag-only or
///   single primitive, nothing to explain.
/// - `Int(i64)` / `Float(f64)` are **deliberately kept distinct** even
///   though serde_json collapses both into one `Number` type. The
///   reason: a u64-shaped ID (Twitter snowflakes, database row IDs,
///   etc.) survives a round-trip through `Int(i64)` losslessly, but
///   would lose precision past 2^53 if forced through `f64`. Splitting
///   the variants lets the parser preserve "this came in as an
///   integer literal, encode it back as an integer literal", which
///   matters for any JSON consumer that distinguishes `1` from `1.0`.
/// - `String(String)` — owned, heap-allocated. We own the data because
///   the `JsonValue` tree outlives the bytes the parser consumed (the
///   request body is freed; the Object userdata is still alive).
/// - `Array(Vec<JsonValue>)` — JSON arrays are dense, sequential, and
///   only ever indexed positionally; `Vec` is the obvious fit. Push /
///   index / iterate are all O(1) or O(n) as expected.
/// - `Object(BTreeMap<String, JsonValue>)` — see below.
///
/// ### Why `BTreeMap` and not `HashMap`?
///
/// Two reasons, both load-bearing:
///
/// 1. **Deterministic key order on encode.** `vnd:serde_json::to_string`
///    reads the keys in iteration order. `BTreeMap` iterates in sorted
///    order so re-serialising the same Object produces byte-identical
///    output every run, every machine. That's table-stakes for
///    snapshot tests, content-addressed caching, HTTP body hashes, and
///    any audit log that diffs JSON. `HashMap` would scramble the
///    order randomly per process (its iteration order is intentionally
///    non-deterministic for DoS resistance).
/// 2. **Predictable benchmarks and no DoS surface.** `HashMap`'s O(1)
///    average becomes O(n) on adversarial keys; `BTreeMap`'s O(log n)
///    is uniform. For typical JSON (5-50 keys per object), the
///    log-factor cost is dwarfed by the per-access overhead anyway,
///    and we keep the worst case bounded.
///
/// We pay for it: BTreeMap inserts are slightly slower than HashMap on
/// large maps (>100 keys), and lookups are O(log n) vs O(1). For JSON
/// workloads neither is a hot path because the per-key cost is tiny
/// compared to the lock acquire and the Lua-side dispatch.
#[derive(Clone, Debug)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub fn empty_object() -> Self {
        JsonValue::Object(BTreeMap::new())
    }
    pub fn empty_array() -> Self {
        JsonValue::Array(Vec::new())
    }

    fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(m) => Some(m),
            _ => None,
        }
    }
    fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(m) => Some(m),
            _ => None,
        }
    }
    fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// JsonNull — sentinel userdata for explicit JSON null
// ---------------------------------------------------------------------------

/// Userdata sentinel that represents JSON `null` distinctly from Lua
/// `nil`.
///
/// ### Why a sentinel rather than mapping to Lua nil?
///
/// Lua treats `nil` as both "absent value" and "no value here". JSON
/// distinguishes the two: `{"k": null}` and `{}` are different
/// documents. Without a sentinel, our `:get` API has to collapse both
/// into Lua nil and the script can't tell them apart — and round-
/// tripping a body through Lua silently turns explicit nulls into
/// missing keys. A sentinel preserves the distinction:
///
/// ```lua
/// local json = require("lib:json")
///
/// local v1 = body:get("explicit_null")  -- == json.null
/// local v2 = body:get("missing_key")    -- == nil
///
/// if v1 == json.null then ... end       -- key was present, value was null
/// if v2 == nil       then ... end       -- key was absent
/// ```
///
/// ### Why a unit-struct userdata?
///
/// Three other shapes were considered:
///
/// 1. **A reserved Lua table** (`json.null = {}`). Same identity
///    semantics if cached, but tables can be iterated, indexed,
///    mutated — surprising failure modes if someone does
///    `for k, v in pairs(json.null)`.
/// 2. **`mlua::LuaSerdeExt`'s `Value::NULL` lightuserdata.** mlua
///    provides a sentinel for serde-driven null encoding, but it's
///    keyed off mlua-internal identity and isn't easy to expose under
///    a stable user-facing name (`json.null`).
/// 3. **A `JsonValue::Null` newtype struct.** Same end result as the
///    unit struct, but without `__eq` semantics every fresh handle
///    would compare unequal — breaking the natural `v == json.null`
///    pattern.
///
/// A unit-struct userdata is the lightest of the three: zero bytes of
/// data, one mlua-registered metatable, and the `__eq` metamethod
/// gives every instance the same identity from Lua's perspective.
///
/// ### `__eq` semantics
///
/// Lua's `__eq` only fires when both operands have the *same metatable*
/// (each Rust UserData type gets its own metatable in mlua). So:
///
/// - `JsonNull == JsonNull` → fires `__eq` → returns `true`.
/// - `JsonNull == nil`      → different types, returns `false` without firing.
/// - `JsonNull == JsonObject`, `JsonNull == "null"`, etc. → different
///   metatables, returns `false` without firing.
///
/// This is the only behaviour that lets `v == json.null` work reliably
/// for any `v` returned by `:get`, regardless of which handle produced
/// it.
pub struct JsonNull;

impl UserData for JsonNull {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Belt-and-braces: even if someone bypasses `null_handle()` and
        // creates a fresh `JsonNull` userdata, this metamethod keeps
        // comparison meaningful from Lua's perspective.
        methods.add_meta_method(MetaMethod::Eq, |_, _, _: AnyUserData| Ok(true));
        // Convenience: `tostring(json.null)` and concatenation.
        methods.add_meta_method(MetaMethod::ToString, |_, _, _: ()| Ok("null".to_string()));
    }
}

/// Lua named-registry key under which the singleton `JsonNull` userdata
/// is cached per engine.
///
/// ### Why a singleton + cache?
///
/// mlua's `PartialEq` on `Value::UserData(_)` is **identity-based** —
/// it compares the underlying registry reference, not the value's
/// `__eq` metamethod. Two freshly-created `JsonNull` userdata are
/// always unequal under Rust-side `==`, even though they're the same
/// type. That breaks the user-visible expectation that
/// `body:get("k") == json.null` works.
///
/// Solution: every code path that returns a "this is JSON null"
/// resolves through `null_handle(lua)`, which lazily creates one
/// userdata on first call and caches it under this registry key. All
/// subsequent calls hand back the same registry handle. mlua's
/// identity-equality then sees them as equal because they *are*
/// literally the same value.
///
/// ### Why named-registry rather than a static?
///
/// The cached handle is a Lua-side reference, so it has to live in
/// the engine's registry to keep mlua's GC informed. A `static` cache
/// would either dangle (mlua GC frees the userdata) or leak the
/// engine's registry. The named-registry slot is the correct place
/// for "engine-scoped singleton Lua value".
const NULL_REGISTRY_KEY: &str = "__lib_json_null_singleton";

/// Returns the per-engine `JsonNull` singleton, creating + caching it
/// on first call.
fn null_handle(lua: &Lua) -> mlua::Result<Value> {
    // TODO: clean-code - 0.55 - KISS: registry miss returns Nil, not Err — fold the `Ok && !Nil` chain into a single match on the loaded Value.
    if let Ok(existing) = lua.named_registry_value::<Value>(NULL_REGISTRY_KEY)
        && !matches!(existing, Value::Nil)
    {
        return Ok(existing);
    }
    let ud = lua.create_userdata(JsonNull)?;
    let v = Value::UserData(ud);
    lua.set_named_registry_value(NULL_REGISTRY_KEY, v.clone())?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// Root + path
// ---------------------------------------------------------------------------

/// The shared, lock-protected owner of a `JsonValue` tree. Every
/// `Object` and `Array` userdata in this module holds an `Arc<Root>`
/// plus a path; "the document" is `Arc<Root>` and "your view of it" is
/// the path.
///
/// ### Why `RwLock<JsonValue>` and not `Mutex<JsonValue>` / `UnsafeCell`?
///
/// Two constraints push us here:
///
/// 1. **mlua `UserData` is `Send + Sync`.** Anything we hand to Lua as
///    a userdata must be safe to share across threads at the type
///    level — that's a hard requirement of the mlua crate's API, not a
///    soft preference. So whatever sits inside `Root` has to satisfy
///    `Send + Sync` without `unsafe` impls.
/// 2. **`RwLock` over `Mutex`** because read-side methods (`:get`,
///    `:has`, `:keys`, `:len`, `:to_table`) don't conflict with each
///    other. A `Mutex` would force every read to serialise; an
///    `RwLock` allows concurrent reads. This also leaves the door open
///    for re-introducing cross-engine sharing later without changing
///    the interior type.
///
/// `parking_lot::RwLock` (rather than `std::sync::RwLock`) for two
/// reasons: it's smaller (1 word vs 1+ words), and it doesn't poison
/// on panic, which is irrelevant for a pure data tree but still avoids
/// an extra failure mode.
///
/// ### Why `Arc<Root>` and not `Root` directly?
///
/// Every navigation hop produces a new userdata that points at the
/// same document. `Arc` is the only way to have N owners of one
/// shared value; `Box`/`Rc` would either prevent sharing or fail the
/// `Sync` requirement. The atomic ref-count cost is paid once per
/// userdata create/drop, which is negligible compared to the work
/// inside a method call.
pub struct Root(RwLock<JsonValue>);

impl Root {
    pub fn from_value(v: JsonValue) -> Arc<Self> {
        Arc::new(Self(RwLock::new(v)))
    }
}

/// One step in a path from a `Root` to a sub-value. An `Object` or
/// `Array` userdata stores `Vec<PathSeg>` — its position in the tree
/// — and resolves it lazily on each access.
///
/// ### Why a path-walk model rather than direct child references?
///
/// The alternative would be to have each userdata point at the
/// sub-`JsonValue` directly (e.g. `&JsonValue` or some kind of
/// pointer). Two reasons we don't:
///
/// 1. **Mutations through one handle must be visible through
///    another.** If you `obj:get("nested")` and stash the result in a
///    local, then later `obj:set("nested", new)`, the stashed handle
///    needs to *now* point at the new value. Direct pointers would be
///    dangling. Path-walk re-resolves every access, so mutations are
///    always observed.
/// 2. **Send+Sync without lifetime parameters.** A direct borrow would
///    need a lifetime tied to the `Root`'s lock guard, which can't
///    cross the userdata boundary into Lua. The path is plain owned
///    data (`Vec<PathSeg>`), trivially `Send + Sync`.
///
/// `String` (not `Arc<str>`) for keys because object keys are usually
/// short and rarely repeated across handles in a way that would
/// benefit from interning. `usize` for indices is the natural type
/// for `Vec` indexing.
#[derive(Clone, Debug)]
pub(crate) enum PathSeg {
    Key(String),
    Index(usize),
}

/// Acquires a shared (read) lock, walks the path, and calls `f` with the
/// resolved node. Multiple readers can be inside `with_resolved`
/// concurrently across workers because `RwLock::read` returns a shared
/// guard.
///
/// The closure pattern is mandatory here, not stylistic: the lock guard
/// is a stack-local `RwLockReadGuard<'_, T>` whose lifetime is tied to
/// this function's scope. We cannot return a `&JsonValue` borrowed from
/// the guard to a caller, because the guard would drop on return and
/// invalidate the borrow. Running `f` *inside* the function keeps the
/// borrow valid for the duration of its use.
fn with_resolved<T>(root: &Root, path: &[PathSeg], f: impl FnOnce(Option<&JsonValue>) -> T) -> T {
    let guard = root.0.read();
    f(walk(&guard, path))
}

/// Acquires the exclusive (write) lock for mutating ops (`:set`,
/// `:push`). Only one writer at a time, and writers block all readers
/// from the lock for the duration of the closure. Same closure rationale
/// as `with_resolved`.
fn with_resolved_mut<T>(
    root: &Root,
    path: &[PathSeg],
    f: impl FnOnce(Option<&mut JsonValue>) -> T,
) -> T {
    let mut guard = root.0.write();
    f(walk_mut(&mut guard, path))
}

// TODO: clean-code - 0.6 - DRY: walk and walk_mut differ only by &/&mut; either accept that as a macro or use a single generic over a Get/GetMut trait.
fn walk<'a>(root: &'a JsonValue, path: &[PathSeg]) -> Option<&'a JsonValue> {
    let mut cur = root;
    for seg in path {
        cur = match (cur, seg) {
            (JsonValue::Object(m), PathSeg::Key(k)) => m.get(k.as_str())?,
            (JsonValue::Array(a), PathSeg::Index(i)) => a.get(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn walk_mut<'a>(root: &'a mut JsonValue, path: &[PathSeg]) -> Option<&'a mut JsonValue> {
    let mut cur = root;
    for seg in path {
        cur = match (cur, seg) {
            (JsonValue::Object(m), PathSeg::Key(k)) => m.get_mut(k.as_str())?,
            (JsonValue::Array(a), PathSeg::Index(i)) => a.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

// ---------------------------------------------------------------------------
// Handle inner — shared between Object and Array
// ---------------------------------------------------------------------------

/// Variadic `:get` — accepts one or more path segments and walks the
/// whole chain under a *single* lock.
///
/// ```lua
/// body:get("name")              -- one segment, one lock
/// body:get("users", 1, "name")  -- three segments, still one lock
/// ```
///
/// Each segment is either a string (object key) or a positive integer
/// (1-based array index). The cost shape:
///   - 1 lock acquire (regardless of depth)
///   - N path-walk steps inside the lock
///   - 1 wrap of the leaf
fn walk_get(
    lua: &Lua,
    root: &Arc<Root>,
    base: &[PathSeg],
    args: MultiValue,
) -> mlua::Result<Value> {
    if args.is_empty() {
        return Err(mlua::Error::external(
            "json:get requires at least one path segment",
        ));
    }
    let mut full_path = base.to_vec();
    for arg in args {
        match arg {
            Value::String(s) => full_path.push(PathSeg::Key(s.to_str()?.to_owned())),
            Value::Integer(i) if i >= 1 => full_path.push(PathSeg::Index((i - 1) as usize)),
            Value::Integer(i) => {
                return Err(mlua::Error::external(format!(
                    "json:get: array index must be >= 1, got {i}"
                )));
            }
            other => {
                return Err(mlua::Error::external(format!(
                    "json:get: path segment must be string or integer, got Lua {}",
                    other.type_name()
                )));
            }
        }
    }
    with_resolved(root, &full_path, |r| match r {
        None => Ok(Value::Nil),
        Some(child) => wrap_value(lua, root.clone(), full_path.clone(), child),
    })
}

// ---------------------------------------------------------------------------
// Object
// ---------------------------------------------------------------------------

/// Lua-facing handle for a JSON object node.
///
/// Conceptually, an `Object` is "the document at this path, viewed as an
/// object". The struct itself is **only two fields** because the actual
/// data lives in `root` — the userdata is just a pointer + breadcrumb.
///
/// ### Field choices
///
/// - `root: Arc<Root>` — atomic ref-counted handle to the document.
///   Must be `Arc` (not `Box`/`Rc`) because mlua's `UserData` requires
///   `Send + Sync`, and any number of userdata within the same engine
///   may point at the same document. The ref count is bumped on `:get`
///   (which produces a child handle) and decremented when a userdata is
///   dropped by Lua's GC.
/// - `path: Vec<PathSeg>` — the address of *this* node within the
///   document, expressed as a sequence of object keys and array indices
///   walked from the root. Owned (not borrowed) because we can't tie a
///   userdata's lifetime to anything Rust-side; it lives as long as Lua
///   keeps a reference.
#[derive(Clone)]
pub struct Object {
    root: Arc<Root>,
    path: Vec<PathSeg>,
}

impl Object {
    // TODO: clean-code - 0.75 - DRY: set_inner/push_inner across Object+Array repeat the resolve+cast+error pattern; extract a `with_object_mut`/`with_array_mut` helper.
    fn set_inner(&self, key: String, v: JsonValue) -> mlua::Result<()> {
        with_resolved_mut(&self.root, &self.path, |resolved| {
            let cur = resolved
                .ok_or_else(|| mlua::Error::external("json.object: parent path no longer valid"))?;
            let m = cur
                .as_object_mut()
                .ok_or_else(|| mlua::Error::external("json.object: target is not an object"))?;
            m.insert(key, v);
            Ok(())
        })
    }
}

/// The element count of whichever container resolves at `path` — an `Object`
/// handle's path always lands on an object, an `Array`'s on an array, so one
/// helper serves both. `0` for a scalar or a stale path.
fn node_len(root: &Root, path: &[PathSeg]) -> i64 {
    with_resolved(root, path, |r| match r {
        Some(JsonValue::Object(m)) => m.len() as i64,
        Some(JsonValue::Array(a)) => a.len() as i64,
        _ => 0,
    })
}

/// The node at `path` as a deep Lua value, or `nil` for a stale path. Shared by
/// both handle types — the conversion is identical regardless of kind.
fn node_to_table(lua: &Lua, root: &Root, path: &[PathSeg]) -> mlua::Result<Value> {
    with_resolved(root, path, |r| match r {
        Some(v) => json_value_to_lua(lua, v),
        None => Ok(Value::Nil),
    })
}

impl UserData for Object {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `get`/`len`/`to_table` are shared with `Array` (both are root+path):
        // `get` delegates to `walk_get`, the other two to the `node_*` helpers.
        methods.add_method("get", |lua, this, args: MultiValue| {
            walk_get(lua, &this.root, &this.path, args)
        });
        methods.add_method("set", |_, this, (key, v): (String, Value)| {
            this.set_inner(key, lua_value_to_json_value(&v)?)
        });
        methods.add_method("has", |_, this, key: String| {
            Ok(with_resolved(&this.root, &this.path, |r| {
                r.and_then(JsonValue::as_object)
                    .map(|m| m.contains_key(&key))
                    .unwrap_or(false)
            }))
        });
        methods.add_method("keys", |lua, this, _: ()| {
            let t = lua.create_table()?;
            with_resolved(&this.root, &this.path, |r| -> mlua::Result<()> {
                if let Some(JsonValue::Object(m)) = r {
                    for (i, k) in m.keys().enumerate() {
                        t.set(i + 1, k.as_str())?;
                    }
                }
                Ok(())
            })?;
            Ok(t)
        });
        methods.add_method("len", |_, this, _: ()| Ok(node_len(&this.root, &this.path)));
        methods.add_method("to_table", |lua, this, _: ()| {
            node_to_table(lua, &this.root, &this.path)
        });
    }
}

// ---------------------------------------------------------------------------
// Array
// ---------------------------------------------------------------------------

/// Lua-facing handle for a JSON array node.
///
/// Same shape as `Object` — an `Arc<Root>` to the document plus a path
/// to this position within it. The two types are distinct (rather than
/// one `JsonHandle` with a runtime kind tag) so mlua's method dispatch
/// can route directly: `:push` on an Array userdata is found via the
/// Array vtable, and trying to call it on an Object errors at the
/// userdata-borrow boundary instead of getting a runtime "wrong-kind"
/// message we'd have to write ourselves.
#[derive(Clone)]
pub struct Array {
    root: Arc<Root>,
    path: Vec<PathSeg>,
}

impl Array {
    fn set_inner(&self, idx: usize, v: JsonValue) -> mlua::Result<()> {
        with_resolved_mut(&self.root, &self.path, |resolved| {
            let cur = resolved
                .ok_or_else(|| mlua::Error::external("json.array: parent path no longer valid"))?;
            let a = cur
                .as_array_mut()
                .ok_or_else(|| mlua::Error::external("json.array: target is not an array"))?;
            if idx >= a.len() {
                a.resize(idx + 1, JsonValue::Null);
            }
            a[idx] = v;
            Ok(())
        })
    }

    fn push_inner(&self, v: JsonValue) -> mlua::Result<()> {
        with_resolved_mut(&self.root, &self.path, |resolved| {
            let cur = resolved
                .ok_or_else(|| mlua::Error::external("json.array: parent path no longer valid"))?;
            let a = cur
                .as_array_mut()
                .ok_or_else(|| mlua::Error::external("json.array: target is not an array"))?;
            a.push(v);
            Ok(())
        })
    }
}

impl UserData for Array {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `get`/`len`/`to_table` are the same as `Object`'s — see those helpers.
        methods.add_method("get", |lua, this, args: MultiValue| {
            walk_get(lua, &this.root, &this.path, args)
        });
        methods.add_method("set", |_, this, (i, v): (i64, Value)| {
            if i < 1 {
                return Err(mlua::Error::external("json.array: index must be >= 1"));
            }
            this.set_inner((i - 1) as usize, lua_value_to_json_value(&v)?)
        });
        methods.add_method("push", |_, this, v: Value| {
            this.push_inner(lua_value_to_json_value(&v)?)
        });
        methods.add_method("len", |_, this, _: ()| Ok(node_len(&this.root, &this.path)));
        methods.add_method("to_table", |lua, this, _: ()| {
            node_to_table(lua, &this.root, &this.path)
        });
    }
}

// ---------------------------------------------------------------------------
// JsonValue ↔ Lua bridge
// ---------------------------------------------------------------------------

/// Lift a scalar `JsonValue` to a Lua value. Returns `Ok(None)` for
/// composite types (Object/Array), which callers must handle themselves
/// (wrap as userdata, or recurse into a Lua table).
fn json_scalar_to_lua(lua: &Lua, v: &JsonValue) -> mlua::Result<Option<Value>> {
    match v {
        JsonValue::Null => Ok(Some(null_handle(lua)?)),
        JsonValue::Bool(b) => Ok(Some(Value::Boolean(*b))),
        JsonValue::Int(i) => Ok(Some(match i32::try_from(*i) {
            Ok(i32v) => Value::Integer(i32v),
            Err(_) => Value::Number(*i as f64),
        })),
        JsonValue::Float(f) => Ok(Some(Value::Number(*f))),
        JsonValue::String(s) => Ok(Some(Value::String(lua.create_string(s)?))),
        JsonValue::Object(_) | JsonValue::Array(_) => Ok(None),
    }
}

/// Wrap one node of a `JsonValue` tree as a Lua value. Objects and Arrays
/// become userdata sharing the given root + path; scalars become Lua-native
/// values. Crate-pub so `vnd:serde_json::from_str` can call it.
pub(crate) fn wrap_value(
    lua: &Lua,
    root: Arc<Root>,
    path: Vec<PathSeg>,
    v: &JsonValue,
) -> mlua::Result<Value> {
    if let Some(scalar) = json_scalar_to_lua(lua, v)? {
        return Ok(scalar);
    }
    match v {
        JsonValue::Object(_) => Object { root, path }.into_lua(lua),
        JsonValue::Array(_) => Array { root, path }.into_lua(lua),
        _ => unreachable!("scalar arms handled above"),
    }
}

/// Wrap a parsed root value. Always builds a fresh `Root` cell.
pub fn wrap_root(lua: &Lua, v: JsonValue) -> mlua::Result<Value> {
    let root = Root::from_value(v.clone());
    wrap_value(lua, root, Vec::new(), &v)
}

/// Convert a Lua value to `JsonValue`, recognising our own `Object` /
/// `Array` handles so nested structures don't have to round-trip through
/// Lua tables.
pub fn lua_value_to_json_value(v: &Value) -> mlua::Result<JsonValue> {
    match v {
        Value::Nil => Ok(JsonValue::Null),
        Value::Boolean(b) => Ok(JsonValue::Bool(*b)),
        Value::Integer(i) => Ok(JsonValue::Int(*i as i64)),
        Value::Number(n) => {
            // Whole-number-fits-in-i64 → keep as Int so re-encode preserves
            // integer literal form. Otherwise Float.
            if n.is_finite() && n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                Ok(JsonValue::Int(*n as i64))
            } else {
                Ok(JsonValue::Float(*n))
            }
        }
        Value::String(s) => Ok(JsonValue::String(s.to_str()?.to_owned())),
        Value::Table(t) => table_to_json_value(t),
        Value::UserData(ud) => {
            if ud.borrow::<JsonNull>().is_ok() {
                return Ok(JsonValue::Null);
            }
            if let Ok(o) = ud.borrow::<Object>() {
                return Ok(with_resolved(&o.root, &o.path, |r| {
                    r.cloned().unwrap_or(JsonValue::Null)
                }));
            }
            if let Ok(a) = ud.borrow::<Array>() {
                return Ok(with_resolved(&a.root, &a.path, |r| {
                    r.cloned().unwrap_or(JsonValue::Null)
                }));
            }
            Err(mlua::Error::external(
                "cannot convert userdata to JSON (not an Object/Array/Null handle)",
            ))
        }
        other => Err(mlua::Error::external(format!(
            "cannot convert Lua {} to JSON",
            other.type_name()
        ))),
    }
}

// TODO: clean-code - 0.55 - KISS: array detection iterates pairs once then iterates again to build; collect once and decide, or rely on raw_len+all_int check without the double walk.
fn table_to_json_value(t: &Table) -> mlua::Result<JsonValue> {
    // Detect array form: keys are exactly 1..=N and there are N entries.
    // Anything else (string keys, gaps, 0 / negative indices) → object.
    let len = t.raw_len();
    let mut total: usize = 0;
    let mut all_int = true;
    let mut max_int: i64 = 0;
    for pair in t.clone().pairs::<Value, Value>() {
        let (k, _) = pair?;
        total += 1;
        match k {
            Value::Integer(i) if i >= 1 => {
                if i as i64 > max_int {
                    max_int = i as i64;
                }
            }
            _ => all_int = false,
        }
    }

    let is_array = total > 0 && all_int && total == len && total == max_int as usize;

    if is_array {
        let mut arr = Vec::with_capacity(len);
        for i in 1..=len {
            let v: Value = t.raw_get(i)?;
            arr.push(lua_value_to_json_value(&v)?);
        }
        Ok(JsonValue::Array(arr))
    } else {
        let mut map = BTreeMap::new();
        for pair in t.clone().pairs::<Value, Value>() {
            let (k, v) = pair?;
            let key = match k {
                Value::String(s) => s.to_str()?.to_owned(),
                Value::Integer(i) => i.to_string(),
                Value::Number(n) => n.to_string(),
                other => {
                    return Err(mlua::Error::external(format!(
                        "non-string key in JSON object: {}",
                        other.type_name()
                    )));
                }
            };
            map.insert(key, lua_value_to_json_value(&v)?);
        }
        Ok(JsonValue::Object(map))
    }
}

/// Materialise a `JsonValue` tree as a Lua-native value (recursive walk).
/// Used by `:to_table()` and any consumer that wants a plain Lua table
/// rather than the userdata view.
///
/// Scalars (including `Null`, which becomes the cached `JsonNull` singleton —
/// preserves the null/absent distinction on `:to_table()` round-trips) are
/// produced by `json_scalar_to_lua`; only the composite branches recurse
/// into Lua tables here.
pub fn json_value_to_lua(lua: &Lua, v: &JsonValue) -> mlua::Result<Value> {
    if let Some(scalar) = json_scalar_to_lua(lua, v)? {
        return Ok(scalar);
    }
    match v {
        JsonValue::Array(a) => {
            let t = lua.create_table_with_capacity(a.len(), 0)?;
            for (i, item) in a.iter().enumerate() {
                t.set(i + 1, json_value_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(t))
        }
        JsonValue::Object(m) => {
            let t = lua.create_table_with_capacity(0, m.len())?;
            for (k, v) in m.iter() {
                t.set(k.as_str(), json_value_to_lua(lua, v)?)?;
            }
            Ok(Value::Table(t))
        }
        _ => unreachable!("scalar arms handled above"),
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:json` surface through the facade.
///
/// `object` / `array` are registered with [`crate::lua::facade::Module::set`]
/// rather than `func` because the hand-written constructors carried **no**
/// `#[instrument]` span — routing them through `func` would add one and break
/// byte-identical tracing. The `Object` / `Array` userdata types keep their
/// own `impl UserData` (their methods are unchanged) and are recorded here for
/// the header via [`crate::lua::facade::Module::userdata`]; `null` is the
/// cached singleton sentinel, set verbatim and recorded as a `const`.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:json")?;

    m.set(
        "object",
        lua.create_function(|_, _: ()| {
            Ok(Object {
                root: Root::from_value(JsonValue::empty_object()),
                path: Vec::new(),
            })
        })?,
    )?
    .set(
        "array",
        lua.create_function(|_, _: ()| {
            Ok(Array {
                root: Root::from_value(JsonValue::empty_array()),
                path: Vec::new(),
            })
        })?,
    )?
    // The `JsonNull` sentinel — userdata, **not** a string or any other
    // primitive. Bound on the module table so the comparison is
    // `local json = require("lib:json"); v == json.null`.
    //
    // We pull the cached singleton via `null_handle` rather than
    // constructing a fresh `JsonNull` here, so this exact handle is
    // identity-equal to every value `:get` returns for a JSON null
    // (mlua's userdata equality is identity-based, not metamethod-
    // mediated).
    .set("null", null_handle(lua)?)?;

    m.record_fn(
        "object",
        &[],
        "Object",
        "Construct an empty mutable JSON object handle on a fresh root.",
    );
    m.record_fn(
        "array",
        &[],
        "Array",
        "Construct an empty mutable JSON array handle on a fresh root.",
    );

    m.userdata("Object", |o| {
        o.method(
            "get",
            &[("path", "LuaValue")],
            "LuaValue",
            "Walk a path of segments and return the leaf node.",
        )
        .method(
            "set",
            &[("key", "string"), ("value", "LuaValue")],
            "()",
            "Insert or replace `key` with `value`.",
        )
        .method(
            "has",
            &[("key", "string")],
            "bool",
            "Whether the object contains an entry for `key`.",
        )
        .method("keys", &[], "LuaValue", "The object's keys as a table.")
        .method("len", &[], "number", "Number of top-level keys.")
        .method(
            "to_table",
            &[],
            "LuaValue",
            "Materialise the subtree as a Lua table.",
        );
    });

    m.userdata("Array", |a| {
        a.method(
            "get",
            &[("path", "LuaValue")],
            "LuaValue",
            "Walk a path starting from a 1-based index.",
        )
        .method(
            "set",
            &[("index", "number"), ("value", "LuaValue")],
            "()",
            "Replace the element at a 1-based index.",
        )
        .method(
            "push",
            &[("value", "LuaValue")],
            "()",
            "Append `value` to the end of the array.",
        )
        .method("len", &[], "number", "Current length of the array.")
        .method(
            "to_table",
            &[],
            "LuaValue",
            "Materialise the array as a 1-indexed Lua table.",
        );
    });

    m.const_(
        "null",
        "LuaValue",
        "Sentinel userdata for explicit JSON `null`, distinct from Lua `nil`.",
    );

    Ok(m)
}

// ---------------------------------------------------------------------------
// Benchmarks — measure the userdata path itself (mutation/access), not
// parsing. The parse benches live in `vnd:serde_json` since parsing is
// `vnd:serde_json`'s job.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("json");

    // Object construction + fill via the public Lua surface — what scripts
    // actually do when they build a response payload.
    let build_chunk: mlua::Function = lua
        .load(
            r#"
            local json = require("lib:json")
            return function()
                local o = json.object()
                o:set("id", 1)
                o:set("name", "Alice")
                o:set("email", "alice@example.com")
                o:set("active", true)
                return o
            end
            "#,
        )
        .eval()
        .expect("build chunk");
    g.bench_function("object_build_4_fields", |b| {
        b.iter(|| {
            let _: Value = build_chunk.call(black_box(())).unwrap();
        });
    });

    // Field read on a pre-built handle — measures the get path through
    // the resolve walk + JV → Lua wrap.
    let get_setup: mlua::Function = lua
        .load(
            r#"
            local json = require("lib:json")
            local o = json.object()
            o:set("x", 42)
            return function() return o:get("x") end
            "#,
        )
        .eval()
        .expect("get setup");
    g.bench_function("object_field_read", |b| {
        b.iter(|| {
            let _: Value = get_setup.call(black_box(())).unwrap();
        });
    });

    // Deep navigation across a wide array — verifies the navigation hops
    // stay constant-time regardless of array width. If someone re-
    // introduces a `.cloned()` in the get path, this bench balloons
    // proportional to the 10k array length.
    //
    // Two variants of the same access pattern:
    //   - chained:    body:get("users"):get(1):get("name")  → 3 locks
    //   - single get: body:get("users", 1, "name")          → 1 lock
    // The variadic `:get` is the trick for cutting per-hop lock cost.
    let deep_setup = r#"
            local serde_json = require("vnd:serde_json")
            local items = { '{"users":[' }
            for i = 1, 10000 do
                if i > 1 then items[#items+1] = "," end
                items[#items+1] = '{"name":"user' .. i .. '","tags":["a","b","c"]}'
            end
            items[#items+1] = ']}'
            local payload = table.concat(items)
            local body = serde_json.from_str(payload)
        "#;

    let deep_nav_chained: mlua::Function = lua
        .load(&format!(
            "{deep_setup}\n return function() return body:get(\"users\"):get(1):get(\"name\") end"
        ))
        .eval()
        .expect("deep nav chained");
    g.bench_function("deep_nav/chained_3_locks", |b| {
        b.iter(|| {
            let _: Value = deep_nav_chained.call(black_box(())).unwrap();
        });
    });

    let deep_nav_single: mlua::Function = lua
        .load(&format!(
            "{deep_setup}\n return function() return body:get(\"users\", 1, \"name\") end"
        ))
        .eval()
        .expect("deep nav single");
    g.bench_function("deep_nav/single_1_lock", |b| {
        b.iter(|| {
            let _: Value = deep_nav_single.call(black_box(())).unwrap();
        });
    });

    g.finish();
}
