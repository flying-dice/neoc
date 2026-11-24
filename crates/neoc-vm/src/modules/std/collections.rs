//! Concurrent data structures: `std:collections`.
//!
//! `Map` and `Counter` are handles on `Send + Sync` state behind an `Arc`: a copy of the handle (in another
//! coroutine, or in a `std:thread` worker it was shared with) sees the same entries and the same count.
//!
//! ```lua
//! local collections = require("std:collections")
//! local cache = collections.Map.new()
//! cache:set("k", "v")
//! local hits = collections.Counter.new(0)
//! hits:increment()
//! ```
//!
//! A map's values are plain data (held as `serde_json::Value`), so they belong to no single VM.
//!
//! The API's rules:
//! - `Map:set(key, nil)` removes the key, so `has` and `get` agree; `Map:keys()` is sorted.
//! - A value that is not plain data (a buffer, a function) is refused as an argument of `Map:set`, with its place.
//! - `Counter` takes whole numbers only: `Counter.new(1.5)` is refused.

#[mlua_ir::module("std:collections")]
pub mod collections {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use mlua_ir::{Object, Unknown};
    use parking_lot::RwLock;

    mlua_ir::unknown_reason!(
        /// Why a map's value has no declared shape.
        Stored = "whatever plain data a script stored under this key: the map holds any value it is given"
    );

    /// A map from string keys to plain data, shared by every copy of the handle.
    #[derive(Clone, Default, Object)]
    pub struct Map {
        entries: Arc<RwLock<BTreeMap<String, serde_json::Value>>>,
    }

    #[mlua_ir::object]
    impl Map {
        /// An empty map.
        ///
        /// ```luau
        /// local map = collections.Map.new()
        /// map:set("user", { name = "alice" })
        /// local user = map:get("user") :: { name: string } -- this script's own knowledge of what it stored
        /// assert(user.name == "alice" and map:size() == 1)
        /// ```
        pub fn new() -> Map {
            Map::default()
        }

        /// The value under `key`, or `nil` when there is none. It is a copy: changing it does not change the map.
        pub fn get(&self, key: String) -> Option<Unknown<Stored>> {
            self.entries.read().get(&key).cloned().map(Unknown::new)
        }

        /// Put `value` (plain data: tables, strings, numbers, booleans) under `key`, replacing what was there. A
        /// `nil` value removes the key.
        pub fn set(&self, key: String, value: Unknown<Stored>) {
            let mut entries = self.entries.write();
            if value.value.is_null() {
                entries.remove(&key);
            } else {
                entries.insert(key, value.value);
            }
        }

        /// Remove `key`; whether it was there.
        pub fn delete(&self, key: String) -> bool {
            self.entries.write().remove(&key).is_some()
        }

        /// Whether there is a value under `key`.
        pub fn has(&self, key: String) -> bool {
            self.entries.read().contains_key(&key)
        }

        /// How many entries the map holds.
        pub fn size(&self) -> usize {
            self.entries.read().len()
        }

        /// Every key, sorted.
        pub fn keys(&self) -> Vec<String> {
            self.entries.read().keys().cloned().collect()
        }

        /// Remove every entry.
        pub fn clear(&self) {
            self.entries.write().clear();
        }
    }

    /// A whole number every copy of the handle counts on. Arithmetic wraps at the edges of 64 bits; a script reads
    /// it as a Lua number, which is exact up to 2^53.
    #[derive(Clone, Default, Object)]
    pub struct Counter {
        value: Arc<AtomicI64>,
    }

    #[mlua_ir::object]
    impl Counter {
        /// A counter at `initial`, or at zero.
        ///
        /// ```luau
        /// local hits = collections.Counter.new()
        /// assert(hits:increment() == 1 and hits:add(4) == 5 and hits:get() == 5)
        /// ```
        pub fn new(initial: Option<i64>) -> Counter {
            Counter { value: Arc::new(AtomicI64::new(initial.unwrap_or(0))) }
        }

        /// The current value.
        pub fn get(&self) -> i64 {
            self.value.load(Ordering::Relaxed)
        }

        /// Add one; the new value.
        pub fn increment(&self) -> i64 {
            self.add(1)
        }

        /// Subtract one; the new value.
        pub fn decrement(&self) -> i64 {
            self.add(-1)
        }

        /// Add `n` (negative to subtract); the new value.
        pub fn add(&self, n: i64) -> i64 {
            self.value.fetch_add(n, Ordering::Relaxed).wrapping_add(n)
        }

        /// Set the value to `n`.
        pub fn set(&self, n: i64) {
            self.value.store(n, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("std:collections")"#).eval().unwrap();
        lua.globals().set("collections", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn a_map_round_trips_plain_data() {
        let ok: bool = eval(
            r#"
            local m = collections.Map.new()
            m:set("s", "hello"); m:set("n", 42); m:set("f", 1.5); m:set("b", false)
            m:set("user", { name = "alice", age = 30, tags = { "admin", "ops" } })
            local u = m:get("user")
            assert(m:get("s") == "hello" and m:get("n") == 42 and m:get("f") == 1.5 and m:get("b") == false)
            assert(u.name == "alice" and u.age == 30 and u.tags[1] == "admin" and u.tags[2] == "ops")
            -- what get gives is a copy
            u.name = "bob"
            return m:get("user").name == "alice" and m:get("nope") == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_map_counts_lists_and_forgets_its_keys() {
        let (keys, sizes, flags): (Vec<String>, Vec<usize>, Vec<bool>) = eval(
            r#"
            local m = collections.Map.new()
            local sizes, flags = { m:size() }, { m:has("b") }
            m:set("b", 1); m:set("a", 2); m:set("c", 3)
            local keys = m:keys()
            table.insert(sizes, m:size())
            table.insert(flags, m:has("b"))
            table.insert(flags, m:delete("b"))
            table.insert(flags, m:delete("b"))
            table.insert(flags, m:has("b"))
            table.insert(sizes, m:size())
            m:clear()
            table.insert(sizes, m:size())
            assert(#m:keys() == 0)
            return keys, sizes, flags
            "#,
        );
        assert_eq!(keys, ["a", "b", "c"]);
        assert_eq!(sizes, [0, 3, 2, 0]);
        assert_eq!(flags, [false, true, true, false, false]);
    }

    #[test]
    fn setting_nil_removes_the_key() {
        let ok: bool = eval(
            r#"
            local m = collections.Map.new()
            m:set("k", 1)
            m:set("k", nil)
            m:set("never", nil)
            return m:get("k") == nil and not m:has("k") and not m:has("never") and m:size() == 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn maps_are_independent_and_a_handle_is_shared() {
        let ok: bool = eval(
            r#"
            local a, b = collections.Map.new(), collections.Map.new()
            a:set("k", "from_a")
            local seen
            coroutine.wrap(function() seen = a:get("k"); a:set("other", 1) end)()
            return b:get("k") == nil and seen == "from_a" and a:has("other")
            "#,
        );
        assert!(ok);
    }

    /// A buffer is refused, never stored as an array of numbers.
    #[test]
    fn a_map_refuses_what_is_not_plain_data() {
        let refused: Vec<String> = eval(
            r#"
            local deep = { buffer.create(2) }
            for _ = 2, 100 do
                deep = { inner = deep }
            end
            local m = collections.Map.new()
            local out = {}
            for _, value in { { data = buffer.create(2) }, deep, print, { f = print } } do
                local ok, err = pcall(m.set, m, "k", value)
                assert(not ok and m:get("k") == nil)
                table.insert(out, tostring(err))
            end
            return out
            "#,
        );
        assert_eq!(refused.len(), 4);
        assert!(
            refused.iter().all(|e| e.contains("std:collections.Map:set: argument #2 'value'")),
            "{refused:#?}"
        );
        assert!(refused[0].contains("byte array") && refused[1].contains("byte array"), "{refused:#?}");
    }

    #[test]
    fn a_counter_counts() {
        let seen: Vec<i64> = eval(
            r#"
            local zero, c = collections.Counter.new(), collections.Counter.new(42)
            local out = { zero:get(), c:get(), zero:increment(), zero:increment(), zero:get(), c:decrement(), c:add(5), c:add(-2) }
            c:set(-7)
            table.insert(out, c:get())
            return out
            "#,
        );
        assert_eq!(seen, [0, 42, 1, 2, 2, 41, 46, 44, -7]);
    }

    /// `Counter` arithmetic wraps at the `i64` edge: a plain `+` panics on overflow in a debug build.
    #[test]
    fn counter_arithmetic_wraps_instead_of_overflowing() {
        // Lua numbers are doubles, so the edges are reached with powers of two
        let (add, dec): (f64, f64) = eval(
            r#"
            local c = collections.Counter.new()
            c:set(2^62)
            local add = c:add(2^62)       -- 2^63 does not fit an i64
            c:set(-2^63)
            local dec = c:decrement()     -- one below the minimum
            return add, dec
            "#,
        );
        assert_eq!((add, dec), (-(2f64.powi(63)), 2f64.powi(63)));
    }

    #[test]
    fn a_counter_takes_whole_numbers_that_fit() {
        let refused: Vec<String> = eval(
            r#"
            local c = collections.Counter.new()
            local out = {}
            for _, call in {
                function() collections.Counter.new(1.5) end,
                function() c:add(0.5) end,
                function() c:set(2^63) end,
                function() c:set("1") end,
            } do
                local ok, err = pcall(call)
                assert(not ok)
                table.insert(out, tostring(err))
            end
            return out
            "#,
        );
        assert!(
            refused[0]
                .contains("std:collections.Counter.new: argument #1 'initial': expected a whole number"),
            "{refused:#?}"
        );
        assert!(refused[1].contains("std:collections.Counter:add: argument #1 'n'"), "{refused:#?}");
        assert!(refused[2].contains("std:collections.Counter:set: argument #1 'n'"), "{refused:#?}");
        assert!(refused[3].contains("std:collections.Counter:set: argument #1 'n'"), "{refused:#?}");
    }
}
