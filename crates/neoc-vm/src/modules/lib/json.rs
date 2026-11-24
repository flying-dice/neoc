//! JSON documents as live handles: `lib:json`.
//!
//! A document is one `serde_json::Value` tree in Rust memory; a script holds `Object` and `Array` handles into it
//! (the shared root and a path). Reading a 100 KiB body allocates one Rust tree, not ten thousand Lua tables, and a
//! handle re-resolves its path on every access, so two handles of one document see each other's changes. Scalars
//! come out as Lua values; a container comes out as a handle until `to_table` is asked for.
//!
//! Limits: a document nests at most [`MAX_DEPTH`] levels (what `serde_json` reads back), and `Array:set` pads at
//! most [`MAX_ARRAY_GAP`] `null`s past the end.
//!
//! The API's rules:
//! - There is no `json.null` sentinel. JSON `null` reads as `nil` (like a missing key); `kind(key)` tells them apart
//!   (`"null"` or `nil`), and `set(key, nil)` writes a `null`. `to_table` leaves nulls out.
//! - `get` takes one key or index; a longer path is `at({ "users", 1, "name" })`.
//! - Indices are whole numbers from 1: a negative or fractional one is refused as an argument.
//! - A failure is a typed `JsonError`, raised (`zero_index`, `stale`, `too_far`, `too_deep`); `json.error_of` reads it
//!   back from what `pcall` caught.
//! - The nesting limit counts the handle's own depth too, so a document cannot be grown past it piece by piece.
//! - A handle nested inside a plain table is refused (pass the handle itself as the value).

use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;

/// One step from a document's root to a node; indices count from 0 here.
#[derive(Debug, Clone)]
enum Step {
    Key(String),
    Index(usize),
}

/// A place in a shared document. `Object` and `Array` are this, each with its own methods.
#[derive(Debug, Clone)]
struct Place {
    root: Arc<RwLock<Value>>,
    path: Vec<Step>,
}

fn step<'a>(node: &'a Value, step: &Step) -> Option<&'a Value> {
    match step {
        Step::Key(key) => node.as_object()?.get(key),
        Step::Index(index) => node.as_array()?.get(*index),
    }
}

fn step_mut<'a>(node: &'a mut Value, step: &Step) -> Option<&'a mut Value> {
    match step {
        Step::Key(key) => node.as_object_mut()?.get_mut(key),
        Step::Index(index) => node.as_array_mut()?.get_mut(*index),
    }
}

impl Place {
    fn new(value: Value) -> Self {
        Self { root: Arc::new(RwLock::new(value)), path: Vec::new() }
    }

    fn child(&self, next: Step) -> Self {
        let mut path = self.path.clone();
        path.push(next);
        Self { root: Arc::clone(&self.root), path }
    }

    /// Runs `f` on the node this place points at now, or on `None` when the path no longer leads anywhere.
    fn read<T>(&self, f: impl FnOnce(Option<&Value>) -> T) -> T {
        let root = self.root.read();
        f(self.path.iter().try_fold(&*root, step))
    }

    fn write<T>(&self, f: impl FnOnce(Option<&mut Value>) -> T) -> T {
        let mut root = self.root.write();
        f(self.path.iter().try_fold(&mut *root, step_mut))
    }

    /// A copy of the node, `null` when the path no longer leads anywhere.
    fn snapshot(&self) -> Value {
        self.read(|node| node.cloned().unwrap_or(Value::Null))
    }
}

/// `true` when `value` nests more than `limit` containers deep. It stops at the limit, so its own recursion is
/// bounded whatever it is given.
pub(crate) fn deeper_than(value: &Value, limit: usize) -> bool {
    let Some(rest) = limit.checked_sub(1) else {
        return matches!(value, Value::Array(_) | Value::Object(_));
    };
    match value {
        Value::Array(items) => items.iter().any(|item| deeper_than(item, rest)),
        Value::Object(entries) => entries.values().any(|item| deeper_than(item, rest)),
        _ => false,
    }
}

fn wide(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[mlua_ir::module("lib:json")]
pub mod json {
    use mlua_ir::{Caught, LuaError, Object as LuaObject, OneOf, Raise, Raised, Shape, Unknown};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use super::{Place, Step, deeper_than, wide};

    /// How many levels a document may nest: one under `serde_json`'s parse limit, so what is built here can be read
    /// back. A table that contains itself (or just nests this far) is refused.
    pub const MAX_DEPTH: u32 = 127;

    /// How many `null`s `Array:set` pads when the index is past the end.
    pub const MAX_ARRAY_GAP: u32 = 1 << 20;

    mlua_ir::unknown_reason!(
        /// Why a value written into a document has no declared shape.
        PlainData = "any plain data a script built (tables, strings, numbers, booleans, nil): a JSON document holds whatever it is given"
    );
    mlua_ir::unknown_reason!(
        /// Why `to_table`'s result has no declared shape.
        Document =
            "a JSON document as plain tables: its shape is the document's, decided by whoever wrote it"
    );

    /// What a JSON value is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Kind {
        /// JSON `null`.
        Null,
        /// `true` or `false`.
        Bool,
        /// A number.
        Number,
        /// A string.
        String,
        /// An array.
        Array,
        /// An object.
        Object,
    }

    fn kind_of(value: &Value) -> Kind {
        match value {
            Value::Null => Kind::Null,
            Value::Bool(_) => Kind::Bool,
            Value::Number(_) => Kind::Number,
            Value::String(_) => Kind::String,
            Value::Array(_) => Kind::Array,
            Value::Object(_) => Kind::Object,
        }
    }

    /// Which container a handle stands for.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Container {
        /// An `Object` handle.
        Object,
        /// An `Array` handle.
        Array,
    }

    /// Why a document refused a read or a change. These are raised; `error_of` reads one back.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum JsonError {
        /// An array index of 0: indices count from 1.
        #[error("array indices start at 1")]
        ZeroIndex,
        /// The handle's place in the document is gone, or now holds something else (its parent was replaced).
        #[error("this handle's place in the document no longer holds an {expected:?}")]
        Stale {
            /// What the handle stands for.
            expected: Container,
        },
        /// `Array:set` past the end by more than `MAX_ARRAY_GAP`.
        #[error(
            "index {index} is too far past the end (length {length}, at most {max_gap} may be padded with null)"
        )]
        TooFar {
            /// The index as given.
            index: u32,
            /// The array's length.
            length: u32,
            /// `MAX_ARRAY_GAP`.
            max_gap: u32,
        },
        /// The document would nest deeper than `MAX_DEPTH`.
        #[error("the document would nest more than {max_depth} levels deep")]
        TooDeep {
            /// `MAX_DEPTH`.
            max_depth: u32,
        },
    }

    /// A JSON object in a document: a live handle. Keys are read in sorted order.
    #[derive(Debug, Clone, LuaObject)]
    pub struct Object {
        place: Place,
    }

    /// A JSON array in a document: a live handle. Indices count from 1.
    #[derive(Debug, Clone, LuaObject)]
    pub struct Array {
        place: Place,
    }

    /// What a read gives: a scalar as a Lua value, a container as a handle. JSON `null` reads as `nil`.
    #[derive(Debug, Clone, OneOf)]
    pub enum Node {
        /// A JSON object.
        Object(Object),
        /// A JSON array.
        Array(Array),
        /// A JSON string.
        Text(String),
        /// A JSON number.
        Number(f64),
        /// `true` or `false`.
        Bool(bool),
    }

    /// A value to write: a handle (its document is copied in), or plain data.
    #[derive(OneOf)]
    pub enum Input {
        /// An `Object` handle; what it holds now is copied.
        Object(Object),
        /// An `Array` handle; what it holds now is copied.
        Array(Array),
        /// Plain data: a table without holes is an array, any other table an object, `nil` is `null`.
        Data(Unknown<PlainData>),
    }

    impl Input {
        /// The JSON value this is.
        #[must_use]
        pub fn into_value(self) -> Value {
            match self {
                Input::Object(object) => object.place.snapshot(),
                Input::Array(array) => array.place.snapshot(),
                Input::Data(data) => data.value,
            }
        }
    }

    /// One step of a path: an object key, or an array index from 1.
    #[derive(Debug, Clone, OneOf)]
    pub enum Segment {
        /// An object key.
        Key(String),
        /// An array index, from 1.
        Index(u32),
    }

    fn index_step(index: u32) -> Result<Step, JsonError> {
        match index.checked_sub(1) {
            Some(zero_based) => Ok(Step::Index(usize::try_from(zero_based).unwrap_or(usize::MAX))),
            None => Err(JsonError::ZeroIndex),
        }
    }

    fn node_at(place: Place) -> Option<Node> {
        let scalar = place.read(|node| match node? {
            Value::Null => None,
            Value::Bool(flag) => Some(Ok(Node::Bool(*flag))),
            Value::Number(number) => Some(Ok(Node::Number(number.as_f64().unwrap_or(f64::NAN)))),
            Value::String(text) => Some(Ok(Node::Text(text.clone()))),
            Value::Array(_) => Some(Err(Container::Array)),
            Value::Object(_) => Some(Err(Container::Object)),
        })?;
        Some(match scalar {
            Ok(node) => node,
            Err(Container::Array) => Node::Array(Array { place }),
            Err(Container::Object) => Node::Object(Object { place }),
        })
    }

    fn follow(place: &Place, path: Vec<Segment>) -> Result<Option<Node>, JsonError> {
        let mut target = place.clone();
        for segment in path {
            target.path.push(match segment {
                Segment::Key(key) => Step::Key(key),
                Segment::Index(index) => index_step(index)?,
            });
        }
        Ok(node_at(target))
    }

    /// The value to store at `place`'s child, refused when the document would nest too deep with it.
    fn admitted(place: &Place, value: Input) -> Result<Value, JsonError> {
        let value = value.into_value();
        let limit = (MAX_DEPTH as usize).saturating_sub(place.path.len());
        if deeper_than(&value, limit) {
            return Err(JsonError::TooDeep { max_depth: MAX_DEPTH });
        }
        Ok(value)
    }

    impl Object {
        /// A handle on a new document holding `entries`.
        #[must_use]
        pub fn from_map(entries: serde_json::Map<String, Value>) -> Self {
            Self { place: Place::new(Value::Object(entries)) }
        }

        /// A copy of what the handle holds now; `null` when its place is gone.
        #[must_use]
        pub fn value(&self) -> Value {
            self.place.snapshot()
        }

        fn entries<T>(&self, f: impl FnOnce(&serde_json::Map<String, Value>) -> T) -> Option<T> {
            self.place.read(|node| node.and_then(Value::as_object).map(f))
        }
    }

    #[mlua_ir::object]
    impl Object {
        /// The value under `key`: a scalar, or a handle for a container. `nil` when there is none, and for a JSON
        /// `null` (`kind` tells the two apart).
        ///
        /// ```luau
        /// local o = json.object()
        /// o:set("name", "ada")
        /// assert(o:get("name") == "ada" and o:get("nope") == nil)
        /// ```
        pub fn get(&self, key: String) -> Option<Node> {
            node_at(self.place.child(Step::Key(key)))
        }

        /// The value at the end of `path` (keys and indices from 1), read in one go. `nil` when the path leads
        /// nowhere.
        ///
        /// ```luau
        /// local o = json.object()
        /// o:set("users", { { name = "ada" } })
        /// assert(o:at({ "users", 1, "name" }) == "ada")
        /// ```
        pub fn at(&self, path: Vec<Segment>) -> Result<Option<Node>, Raise<JsonError>> {
            Ok(follow(&self.place, path)?)
        }

        /// What the value under `key` is; `nil` when there is no such key.
        ///
        /// ```luau
        /// local o = json.object()
        /// o:set("gone", nil)
        /// assert(o:kind("gone") == "null" and o:kind("never") == nil)
        /// ```
        pub fn kind(&self, key: String) -> Option<Kind> {
            self.entries(|entries| entries.get(&key).map(kind_of)).flatten()
        }

        /// Insert or replace `key`. `nil` writes a JSON `null`.
        pub fn set(&self, key: String, value: Input) -> Result<(), Raise<JsonError>> {
            let value = admitted(&self.place, value)?;
            self.place.write(|node| match node.and_then(Value::as_object_mut) {
                Some(entries) => {
                    entries.insert(key, value);
                    Ok(())
                }
                None => Err(JsonError::Stale { expected: Container::Object }.into()),
            })
        }

        /// `true` when the object has `key` (a `null` under it counts).
        pub fn has(&self, key: String) -> bool {
            self.entries(|entries| entries.contains_key(&key)).unwrap_or(false)
        }

        /// The keys, sorted.
        pub fn keys(&self) -> Vec<String> {
            let mut keys =
                self.entries(|entries| entries.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
            keys.sort_unstable();
            keys
        }

        /// How many keys the object has; 0 when the handle's place no longer holds an object.
        pub fn len(&self) -> u32 {
            self.entries(|entries| wide(entries.len())).unwrap_or(0)
        }

        /// `true` when the object has no keys.
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        /// The object as plain tables, nested containers included. A `null` is left out.
        pub fn to_table(&self) -> Unknown<Document> {
            Unknown::new(self.place.snapshot())
        }
    }

    impl Array {
        /// A handle on a new document holding `items`.
        #[must_use]
        pub fn from_items(items: Vec<Value>) -> Self {
            Self { place: Place::new(Value::Array(items)) }
        }

        /// A copy of what the handle holds now; `null` when its place is gone.
        #[must_use]
        pub fn value(&self) -> Value {
            self.place.snapshot()
        }

        fn items<T>(&self, f: impl FnOnce(&Vec<Value>) -> T) -> Option<T> {
            self.place.read(|node| node.and_then(Value::as_array).map(f))
        }

        fn items_mut<T>(
            &self,
            f: impl FnOnce(&mut Vec<Value>) -> Result<T, JsonError>,
        ) -> Result<T, JsonError> {
            self.place.write(|node| match node.and_then(Value::as_array_mut) {
                Some(items) => f(items),
                None => Err(JsonError::Stale { expected: Container::Array }),
            })
        }
    }

    #[mlua_ir::object]
    impl Array {
        /// The item at `index` (from 1): a scalar, or a handle for a container. `nil` past the end, and for a JSON
        /// `null` (`kind` tells the two apart).
        ///
        /// ```luau
        /// local a = json.array()
        /// a:push("x")
        /// assert(a:get(1) == "x" and a:get(2) == nil)
        /// ```
        pub fn get(&self, index: u32) -> Result<Option<Node>, Raise<JsonError>> {
            Ok(node_at(self.place.child(index_step(index)?)))
        }

        /// The value at the end of `path` (indices from 1 and keys), read in one go. `nil` when the path leads
        /// nowhere.
        pub fn at(&self, path: Vec<Segment>) -> Result<Option<Node>, Raise<JsonError>> {
            Ok(follow(&self.place, path)?)
        }

        /// What the item at `index` is; `nil` past the end.
        pub fn kind(&self, index: u32) -> Result<Option<Kind>, Raise<JsonError>> {
            let Step::Index(at) = index_step(index)? else { return Ok(None) };
            Ok(self.items(|items| items.get(at).map(kind_of)).flatten())
        }

        /// Replace the item at `index` (from 1). Past the end, the gap is filled with `null`s (at most
        /// `MAX_ARRAY_GAP`). `nil` writes a JSON `null`.
        pub fn set(&self, index: u32, value: Input) -> Result<(), Raise<JsonError>> {
            let Step::Index(at) = index_step(index)? else { return Ok(()) };
            let value = admitted(&self.place, value)?;
            Ok(self.items_mut(|items| {
                // The padding is bounded: an index of 2^32 must not become a resize of that many nulls.
                if at >= items.len().saturating_add(MAX_ARRAY_GAP as usize) {
                    return Err(JsonError::TooFar {
                        index,
                        length: wide(items.len()),
                        max_gap: MAX_ARRAY_GAP,
                    });
                }
                if at >= items.len() {
                    items.resize(at + 1, Value::Null);
                }
                items[at] = value;
                Ok(())
            })?)
        }

        /// Append `value`. `nil` appends a JSON `null`.
        pub fn push(&self, value: Input) -> Result<(), Raise<JsonError>> {
            let value = admitted(&self.place, value)?;
            Ok(self.items_mut(|items| {
                items.push(value);
                Ok(())
            })?)
        }

        /// How many items the array has; 0 when the handle's place no longer holds an array.
        pub fn len(&self) -> u32 {
            self.items(|items| wide(items.len())).unwrap_or(0)
        }

        /// `true` when the array has no items.
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        /// The array as plain tables, nested containers included. A `null` item is `nil` (a hole).
        pub fn to_table(&self) -> Unknown<Document> {
            Unknown::new(self.place.snapshot())
        }
    }

    impl Node {
        /// `value` as the root of a new document: what a parser hands a script. `None` for a JSON `null`.
        #[must_use]
        pub fn root(value: Value) -> Option<Node> {
            node_at(Place::new(value))
        }
    }

    /// An empty object, on a new document.
    ///
    /// ```luau
    /// local o = json.object()
    /// o:set("tags", { "a", "b" })
    /// local tags = o:get("tags") :: lib_json_Array
    /// tags:push("c")
    /// assert(tags:len() == 3 and o:at({ "tags", 3 }) == "c")
    /// ```
    pub fn object() -> Object {
        Object::from_map(serde_json::Map::new())
    }

    /// An empty array, on a new document.
    pub fn array() -> Array {
        Array::from_items(Vec::new())
    }

    /// The `JsonError` a call of this module raised, read back from what `pcall` caught; `nil` for anything else.
    ///
    /// ```luau
    /// local ok, caught = pcall(function() json.array():set(0, 1) end)
    /// local err = json.error_of(caught)
    /// assert(not ok and err ~= nil and err.kind == "zero_index")
    /// ```
    pub fn error_of(caught: Caught) -> Option<Raised<JsonError>> {
        caught.of()
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use serde_json::json;

    use super::json::{Array, Node, Object};

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:json")"#).eval().unwrap();
        lua.globals().set("json", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    /// `Array:set(2^31, v)` must not `resize` to that many elements; a small gap still pads with null.
    #[test]
    fn array_set_far_past_the_end_is_an_error_not_an_abort() {
        let (small, far_kind, far_msg, len): (bool, String, String, i64) = eval(
            r#"
            local a = json.array()
            a:push(1)
            a:set(5, "x")
            local small = a:len() == 5 and a:get(3) == nil and a:kind(3) == "null"
            local ok1, far = pcall(a.set, a, 2^31, 1)
            local ok2 = pcall(a.set, a, 2^40, 1)
            local ok3 = pcall(a.set, a, 2^63, 1)
            assert(not ok1 and not ok2 and not ok3)
            local err = json.error_of(far)
            assert(err.index == 2^31 and err.length == 5 and err.max_gap == json.MAX_ARRAY_GAP)
            return small, err.kind, tostring(far), a:len()
            "#,
        );
        assert!(small, "a small gap still pads with null");
        assert_eq!(far_kind, "too_far");
        assert!(far_msg.contains("too far past the end"), "{far_msg}");
        assert_eq!(len, 5, "a refused set leaves the array alone");
    }

    #[test]
    fn the_largest_allowed_gap_works_and_one_past_it_is_refused() {
        let (at, over): (bool, bool) = eval(
            r#"
            local a = json.array()
            a:push(1)
            local ok_at = pcall(a.set, a, 1 + 2 ^ 20, "x")
            local b = json.array()
            b:push(1)
            local ok_over = pcall(b.set, b, 2 + 2 ^ 20, "x")
            return ok_at and a:len() == 1 + 2 ^ 20, ok_over
            "#,
        );
        assert!(at);
        assert!(!over);
    }

    /// A table that contains itself, or one nested a hundred thousand deep, is refused: no stack overflow.
    #[test]
    fn cyclic_and_very_deep_tables_are_an_error_not_a_stack_overflow() {
        let refused: Vec<String> = eval(
            r#"
            local out = {}
            local cyc = {}
            cyc[1] = cyc
            local obj = {}
            obj.self = obj
            local deep = {}
            for _ = 1, 100000 do deep = { deep } end
            local a = json.array()
            for _, v in ipairs({ cyc, obj, deep }) do
                local ok, e = pcall(a.push, a, v)
                assert(not ok)
                out[#out + 1] = tostring(e)
            end
            assert(a:len() == 0)
            return out
            "#,
        );
        assert_eq!(refused.len(), 3);
    }

    #[test]
    fn the_nesting_limit_is_127_levels_under_a_root() {
        let (ok127, kind128): (bool, String) = eval(
            r#"
            local function nested(depth)
                local v = {}
                for _ = 2, depth do v = { v } end
                return v
            end
            local a = json.array()
            local ok127 = pcall(a.push, a, nested(127))
            local ok128, caught = pcall(a.push, a, nested(128))
            assert(not ok128)
            local err = json.error_of(caught)
            assert(err.max_depth == json.MAX_DEPTH and err.op == "lib:json.Array:push")
            return ok127, err.kind
            "#,
        );
        assert!(ok127);
        assert_eq!(kind128, "too_deep");
    }

    /// Handles nested inside handles, or values written through a deep child handle, cannot pass the limit either.
    #[test]
    fn a_document_cannot_be_grown_past_the_limit_piece_by_piece() {
        let (wrapped, through_child): (i64, i64) = eval(
            r#"
            local inner = json.array()
            local wrapped = 0
            for i = 1, 300 do
                local outer = json.array()
                if not pcall(outer.push, outer, inner) then break end
                inner = outer
                wrapped = i
            end
            local root = json.array()
            local cursor = root
            local through_child = 0
            for i = 1, 300 do
                if not pcall(cursor.push, cursor, json.array()) then break end
                cursor = cursor:get(1)
                through_child = i
            end
            return wrapped, through_child
            "#,
        );
        assert_eq!(wrapped, 127, "128 arrays in all");
        assert_eq!(through_child, 127, "128 arrays in all");
    }

    // A handle re-resolves its path on every access, so retyping a child under a live handle leaves that handle
    // pointing at the other kind: it reads as empty, and refuses a write.
    #[test]
    fn a_handle_whose_place_was_retyped_reads_as_empty_and_refuses_writes() {
        let (object_len, keys, has, array_len, object_err, array_err): (i64, i64, bool, i64, String, String) =
            eval(
                r#"
            local root = json.object()
            root:set("a", json.object())
            root:set("b", json.array())
            local a, b = root:get("a"), root:get("b")
            root:set("a", { 10, 20, 30 })
            root:set("b", { x = 1, y = 2, z = 3 })
            local _, e1 = pcall(a.set, a, "k", 1)
            local _, e2 = pcall(b.push, b, 1)
            local stale = json.error_of(e2)
            assert(stale.kind == "stale" and stale.expected == "array")
            return a:len(), #a:keys(), a:has("anything"), b:len(), json.error_of(e1).expected, tostring(e2)
            "#,
            );
        assert_eq!((object_len, keys, has, array_len), (0, 0, false, 0));
        assert_eq!(object_err, "object");
        assert!(array_err.starts_with("lib:json.Array:push: stale:"), "{array_err}");
    }

    #[test]
    fn len_reports_the_live_count_for_a_matching_kind() {
        let (object_len, array_len): (i64, i64) = eval(
            r#"
            local root = json.object()
            root:set("o", { x = 1, y = 2 })
            root:set("a", { 10, 20, 30, 40 })
            assert(not root:is_empty() and json.object():is_empty() and json.array():is_empty())
            return root:get("o"):len(), root:get("a"):len()
            "#,
        );
        assert_eq!((object_len, array_len), (2, 4));
    }

    #[test]
    fn every_kind_of_value_round_trips_and_is_named() {
        let seen: Vec<String> = eval(
            r#"
            local o = json.object()
            o:set("null", nil)
            o:set("bool", false)
            o:set("int", -42)
            o:set("float", 3.25)
            o:set("string", "")
            o:set("array", { 1, "two", true })
            o:set("object", { nested = { deep = "yes" } })
            assert(o:get("null") == nil and o:has("null") and not o:has("missing"))
            assert(o:get("bool") == false and o:get("int") == -42 and o:get("float") == 3.25 and o:get("string") == "")
            assert(o:get("array"):get(2) == "two" and o:at({ "object", "nested", "deep" }) == "yes")
            local out = {}
            for _, key in ipairs(o:keys()) do out[#out + 1] = key .. "=" .. o:kind(key) end
            assert(o:kind("missing") == nil)
            local a = o:get("array")
            a:push(nil)
            assert(a:len() == 4 and a:kind(4) == "null" and a:kind(5) == nil and a:kind(1) == "number")
            return out
            "#,
        );
        assert_eq!(
            seen,
            [
                "array=array",
                "bool=bool",
                "float=number",
                "int=number",
                "null=null",
                "object=object",
                "string=string"
            ]
        );
    }

    #[test]
    fn handles_of_one_document_see_each_other() {
        let ok: bool = eval(
            r#"
            local root = json.object()
            root:set("user", { name = "ada", tags = { "a" } })
            local user, same = root:get("user"), root:get("user")
            local tags = user:get("tags")
            local name = user:get("name")
            user:set("name", "grace")
            same:get("tags"):push("b")
            root:at({ "user", "tags" }):set(1, "z")
            -- a scalar was read by value; handles follow the document
            return name == "ada" and same:get("name") == "grace" and tags:len() == 2 and tags:get(1) == "z"
                and root:at({ "user", "tags", 2 }) == "b"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_handle_given_as_a_value_is_copied() {
        let ok: bool = eval(
            r#"
            local inner = json.object()
            inner:set("n", 1)
            local list = json.array()
            list:push(10)
            local root = json.object()
            root:set("inner", inner)
            root:set("list", list)
            inner:set("n", 2)
            list:push(20)
            return root:at({ "inner", "n" }) == 1 and root:get("list"):len() == 1
            "#,
        );
        assert!(ok, "the document holds what the handle held when it was written");
    }

    #[test]
    fn to_table_gives_plain_tables_without_nulls() {
        let ok: bool = eval(
            r#"
            local o = json.object()
            o:set("a", { 1, 2, { deep = true } })
            o:set("gone", nil)
            o:set("s", "x")
            local t = o:to_table()
            assert(type(t) == "table" and t.gone == nil and t.s == "x")
            assert(#t.a == 3 and t.a[3].deep == true)
            local list = o:get("a"):to_table()
            return type(list) == "table" and list[1] == 1 and list[2] == 2
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_path_takes_keys_and_indices_and_leads_nowhere_quietly() {
        let (zero, bad): (String, String) = eval(
            r#"
            local o = json.object()
            o:set("users", { { name = "ada" }, { name = "grace" } })
            assert(o:at({ "users", 2, "name" }) == "grace")
            assert(o:at({ "users", 9, "name" }) == nil and o:at({ "nope", "name" }) == nil)
            assert(o:at({ "users", "name" }) == nil, "a key into an array leads nowhere")
            assert(o:at({}):len() == 1, "the empty path is the handle itself")
            local users = o:get("users")
            assert(users:at({ 1, "name" }) == "ada" and users:get(3) == nil)
            local _, zero = pcall(users.get, users, 0)
            assert(json.error_of(zero).kind == "zero_index")
            local _, also = pcall(o.at, o, { "users", 0 })
            assert(json.error_of(also).kind == "zero_index")
            assert(not pcall(users.set, users, 0, "x") and not pcall(users.kind, users, 0))
            local _, bad = pcall(users.get, users, -1)
            assert(not pcall(users.get, users, 1.5) and not pcall(o.at, o, { true }))
            return tostring(zero), tostring(bad)
            "#,
        );
        assert!(zero.starts_with("lib:json.Array:get: zero_index: array indices start at 1"), "{zero}");
        assert!(bad.contains("lib:json.Array:get: argument #1 'index'"), "{bad}");
    }

    #[test]
    fn values_that_are_not_plain_data_are_refused() {
        let refused: String = eval(
            r#"
            local o = json.object()
            assert(not pcall(o.set, o, "f", function() end))
            assert(not pcall(o.set, o, "nested", { json.array() }), "a handle inside a table")
            assert(not pcall(o.set, o, 1, "x"), "a key is a string")
            local _, e = pcall(o.set, o, "f", print)
            return tostring(e)
            "#,
        );
        assert!(refused.contains("lib:json.Object:set: argument #2 'value'"), "{refused}");
    }

    #[test]
    fn rust_builds_a_document_and_reads_a_handle_back() {
        let lua = engine();
        let Some(Node::Object(body)) = Node::root(json!({ "name": "ada", "tags": ["a", "b"], "none": null }))
        else {
            panic!("an object");
        };
        lua.globals().set("body", body.clone()).unwrap();
        let ok: bool = lua
            .load(
                r#"
                body:get("tags"):push("c")
                return body:get("name") == "ada" and body:kind("none") == "null" and body:len() == 3
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
        assert_eq!(body.value(), json!({ "name": "ada", "tags": ["a", "b", "c"], "none": null }));
        assert!(Node::root(json!(null)).is_none());
        assert!(matches!(Node::root(json!(7)), Some(Node::Number(n)) if (n - 7.0).abs() < f64::EPSILON));
        assert_eq!(Array::from_items(vec![json!(1)]).value(), json!([1]));
        assert_eq!(Object::from_map(serde_json::Map::new()).value(), json!({}));
    }
}
