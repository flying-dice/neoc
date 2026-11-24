//! A bounded in-memory cache: `lib:cache` (the `lru` crate).
//!
//! A `Cache` holds at most `max_size` entries, drops the least recently used one to make room, and can give each
//! entry a time to live. It is for memoisation, request deduplication and hot lookups: where a plain table would
//! grow without bound.
//!
//! Eviction: `get` and `set` both count as a use, so the entry dropped is always the one untouched the longest.
//!
//! Expiry is lazy: an entry is dropped when it is seen past its deadline (by `get`, or by the sweep `len` does),
//! never by a background timer. An entry that is never read again costs its slot, which `max_size` already bounds.
//!
//! Statistics and the eviction callback: `stats()` counts `hits`, `misses` and `evictions`. An eviction is the
//! cache dropping an entry on its own (no room, or expired); `delete`, `clear` and overwriting are the caller's
//! doing and count for nothing. `on_evict(key, value)` is called for exactly those drops, after the cache's lock is
//! released: a callback may use the same cache.
//!
//! The API's rules:
//! - `Cache.new(max_size, ttl?, on_evict?)`: the size is required and so is not an option, and each part has its type.
//! - A time to live is a `std:time` `Duration`. `Duration.zero()` is "never expires".
//! - A size that is not a whole number from 0 up is refused as an argument; 0 and a size past the limit are the typed
//!   `NewError` (`out_of_range`). A key that is not a string, a number or a boolean is refused as an argument.
//! - Storing `nil` raises the typed `SetError` (`nil_value`).
//! - `stats()` is a declared record.

use std::time::Instant;

use lru::LruCache;
use mlua::Value;

/// The one NaN every NaN key is stored as. NaN is not equal to itself, so without this two NaN keys would never
/// meet, and one could never be read back.
const CANONICAL_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

/// The largest `max_size`. `LruCache::new` reserves its index up front (about 16 bytes a slot), so an unchecked
/// size is a reservation of gigabytes, or a panic past hashbrown's limit. 16 million entries (about 256 MiB of
/// index) is already past what an in-memory cache is for.
const MAX_CAPACITY: u32 = 1 << 24;

/// A key as it is stored. Tables and functions are not keys: Lua compares them by identity, and two tables that
/// look the same would miss each other.
#[derive(Clone, PartialEq, Eq, Hash)]
enum CacheKey {
    Str(Vec<u8>),
    /// `f64::to_bits`, with `0.0` and `-0.0` as one zero and every NaN as one NaN. `1` and `1.0` are the same
    /// number and the same bits.
    Num(u64),
    Bool(bool),
}

fn canonical_num_bits(n: f64) -> u64 {
    if n == 0.0 {
        0
    } else if n.is_nan() {
        CANONICAL_NAN_BITS
    } else {
        n.to_bits()
    }
}

/// A stored value and when it expires.
struct Entry {
    value: Value,
    expires_at: Option<Instant>,
}

impl Entry {
    fn is_expired(&self, now: Instant) -> bool {
        self.expires_at.is_some_and(|deadline| now >= deadline)
    }
}

/// What the lock guards: the map and the counters. Each method returns the entries it dropped, for the caller to
/// tell `on_evict` of once the lock is released.
struct CacheState {
    map: LruCache<CacheKey, Entry>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl CacheState {
    /// The value under `key` (a hit, and now the most recently used), or `None` (a miss). An entry found past its
    /// deadline is dropped: a miss and an eviction.
    fn lookup(&mut self, key: &CacheKey, now: Instant) -> (Option<Value>, Option<(CacheKey, Entry)>) {
        if matches!(self.map.peek(key), Some(entry) if entry.is_expired(now)) {
            let dropped = self.map.pop_entry(key);
            if dropped.is_some() {
                self.evictions += 1;
            }
            self.misses += 1;
            return (None, dropped);
        }
        if let Some(entry) = self.map.get(key) {
            self.hits += 1;
            (Some(entry.value.clone()), None)
        } else {
            self.misses += 1;
            (None, None)
        }
    }

    /// Store `entry` as the most recently used. The entry pushed out for room is returned (an eviction); the old
    /// value of the same key is not (overwriting is the caller's doing).
    fn insert(&mut self, key: CacheKey, entry: Entry) -> Option<(CacheKey, Entry)> {
        match self.map.push(key.clone(), entry) {
            Some((old_key, old_entry)) if old_key != key => {
                self.evictions += 1;
                Some((old_key, old_entry))
            }
            _ => None,
        }
    }

    /// Drop every entry past its deadline, each an eviction.
    fn sweep_expired(&mut self, now: Instant) -> Vec<(CacheKey, Entry)> {
        let stale: Vec<CacheKey> =
            self.map.iter().filter(|(_, entry)| entry.is_expired(now)).map(|(key, _)| key.clone()).collect();
        let mut dropped = Vec::with_capacity(stale.len());
        for key in &stale {
            if let Some(pair) = self.map.pop_entry(key) {
                self.evictions += 1;
                dropped.push(pair);
            }
        }
        dropped
    }
}

#[mlua_ir::module("lib:cache")]
pub mod cache {
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::time::{Duration as StdDuration, Instant};

    use lru::LruCache;
    use mlua_ir::{Bytes, Callback, Live, LuaError, Object, OneOf, Raise, Shape};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};

    use super::{CacheKey, CacheState, Entry, MAX_CAPACITY, canonical_num_bits};
    use crate::modules::std::time::time::Duration;

    mlua_ir::unknown_reason!(
        /// Why a stored value has no declared shape.
        Stored = "whatever a script stored: it comes back as it is, the same table or function"
    );

    /// What an entry is stored under. `1` and `1.0` are the same key.
    #[derive(Debug, Clone, OneOf)]
    pub enum Key {
        /// A string, byte for byte.
        Text(Bytes),
        /// A number.
        Number(f64),
        /// `true` or `false`.
        Boolean(bool),
    }

    impl From<Key> for CacheKey {
        fn from(key: Key) -> Self {
            match key {
                Key::Text(bytes) => CacheKey::Str(bytes.0),
                Key::Number(n) => CacheKey::Num(canonical_num_bits(n)),
                Key::Boolean(b) => CacheKey::Bool(b),
            }
        }
    }

    /// The key a script gave, for `on_evict`: a string's bytes are exact, a number is its `f64`.
    impl From<CacheKey> for Key {
        fn from(key: CacheKey) -> Self {
            match key {
                CacheKey::Str(bytes) => Key::Text(Bytes(bytes)),
                CacheKey::Num(bits) => Key::Number(f64::from_bits(bits)),
                CacheKey::Bool(b) => Key::Boolean(b),
            }
        }
    }

    /// Called when the cache drops an entry on its own: no room, or expired.
    pub type OnEvict = Callback<fn(key: Key, value: Live<Stored>)>;

    /// Why a cache could not be made.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum NewError {
        /// The size is 0, or more entries than a cache holds.
        #[error("max_size must be from 1 to {max}, got {max_size}")]
        OutOfRange {
            /// The size asked for.
            max_size: u32,
            /// The largest size there is.
            max: u32,
        },
    }

    /// Why a value was not stored.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum SetError {
        /// The value is `nil`, which `get` could not tell from a miss.
        #[error("the value must not be nil: get could not tell it from a miss")]
        NilValue,
    }

    /// What a cache has counted since it was made. `clear` does not reset it.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, Shape)]
    pub struct Stats {
        /// The `get`s that found their key.
        pub hits: u64,
        /// The `get`s that did not, expired entries included.
        pub misses: u64,
        /// The entries the cache dropped on its own: no room, or expired.
        pub evictions: u64,
    }

    struct Inner {
        state: Mutex<CacheState>,
        on_evict: Option<OnEvict>,
        default_ttl: Option<StdDuration>,
        max_size: usize,
    }

    /// A time to live as a deadline's length; zero is "never expires".
    fn positive(ttl: Duration) -> Option<StdDuration> {
        let ttl = StdDuration::from(ttl);
        (!ttl.is_zero()).then_some(ttl)
    }

    /// A map of at most `max_size` entries that drops the least recently used. `tostring` gives
    /// `Cache(len=1, max_size=4)`.
    #[derive(Clone, Object)]
    pub struct Cache {
        inner: Arc<Inner>,
    }

    impl Cache {
        /// Tell `on_evict` of each dropped entry. The lock is not held: the callback may use this cache.
        fn notify_evicted(&self, evicted: impl IntoIterator<Item = (CacheKey, Entry)>) -> mlua::Result<()> {
            if let Some(on_evict) = &self.inner.on_evict {
                for (key, entry) in evicted {
                    on_evict.call(key.into(), Live::new(entry.value))?;
                }
            }
            Ok(())
        }
    }

    // `len` sweeps expired entries and may call `on_evict`: it is not the cheap getter the lint has in mind, and a
    // script writes `c:len() == 0`.
    #[allow(clippy::len_without_is_empty)]
    #[mlua_ir::object]
    impl Cache {
        /// A cache of at most `max_size` entries (1 or more). `ttl` is how long an entry lives unless `set` says
        /// otherwise; with none, or zero, entries do not expire. `on_evict` is called with the key and the value
        /// of each entry the cache drops on its own (no room, or expired), not for `delete`, `clear` or an
        /// overwrite.
        ///
        /// ```luau
        /// local c = assert(cache.Cache.new(2))
        /// c:set("a", 1)
        /// c:set("b", 2)
        /// c:get("a") -- a is now the most recently used
        /// c:set("c", 3) -- b goes
        /// assert(c:get("a") == 1 and c:get("b") == nil and c:len() == 2)
        /// ```
        pub fn new(
            max_size: u32,
            ttl: Option<Duration>,
            on_evict: Option<OnEvict>,
        ) -> Result<Cache, NewError> {
            let capacity = usize::try_from(max_size)
                .ok()
                .filter(|_| max_size <= MAX_CAPACITY)
                .and_then(NonZeroUsize::new)
                .ok_or(NewError::OutOfRange { max_size, max: MAX_CAPACITY })?;
            Ok(Cache {
                inner: Arc::new(Inner {
                    state: Mutex::new(CacheState {
                        map: LruCache::new(capacity),
                        hits: 0,
                        misses: 0,
                        evictions: 0,
                    }),
                    on_evict,
                    default_ttl: ttl.and_then(positive),
                    max_size: capacity.get(),
                }),
            })
        }

        /// The value under `key`, or `nil` when there is none or it has expired. A hit makes the entry the most
        /// recently used.
        pub fn get(&self, key: Key) -> Result<Option<Live<Stored>>, mlua::Error> {
            let (value, evicted) = self.inner.state.lock().lookup(&key.into(), Instant::now());
            self.notify_evicted(evicted)?;
            Ok(value.map(Live::new))
        }

        /// Store `value` under `key` as the most recently used; the least recently used entry goes when there is
        /// no room. `ttl` is this entry's time to live in place of the cache's: zero is "never expires". `nil` is
        /// not a value.
        ///
        /// ```luau
        /// local time = require("std:time")
        /// local c = assert(cache.Cache.new(100, time.Duration.from_secs(60)))
        /// c:set("session", { user = "ada" }) -- lives a minute
        /// c:set("config", { debug = false }, time.Duration.zero()) -- does not expire
        /// ```
        pub fn set(
            &self,
            key: Key,
            value: Live<Stored>,
            ttl: Option<Duration>,
        ) -> Result<Result<(), Raise<SetError>>, mlua::Error> {
            if value.value.is_nil() {
                return Ok(Err(Raise(SetError::NilValue)));
            }
            let lifetime = match ttl {
                None => self.inner.default_ttl,
                Some(ttl) => positive(ttl),
            };
            // a deadline past the end of the clock is no deadline
            let expires_at = lifetime.and_then(|lifetime| Instant::now().checked_add(lifetime));
            let evicted =
                self.inner.state.lock().insert(key.into(), Entry { value: value.value, expires_at });
            self.notify_evicted(evicted)?;
            Ok(Ok(()))
        }

        /// Remove `key`; whether it was there. Not an eviction: `on_evict` is not called.
        pub fn delete(&self, key: Key) -> bool {
            self.inner.state.lock().map.pop(&key.into()).is_some()
        }

        /// Remove every entry. Not an eviction: `on_evict` is not called, and `stats` keeps its counts.
        pub fn clear(&self) {
            self.inner.state.lock().map.clear();
        }

        /// How many entries are live. Expired entries are dropped first (each an eviction), so they are not
        /// counted.
        pub fn len(&self) -> Result<usize, mlua::Error> {
            let (live, evicted) = {
                let mut state = self.inner.state.lock();
                let evicted = state.sweep_expired(Instant::now());
                (state.map.len(), evicted)
            };
            self.notify_evicted(evicted)?;
            Ok(live)
        }

        /// The counts so far.
        pub fn stats(&self) -> Stats {
            let state = self.inner.state.lock();
            Stats { hits: state.hits, misses: state.misses, evictions: state.evictions }
        }

        /// `Cache(len=1, max_size=4)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("Cache(len={}, max_size={})", self.inner.state.lock().map.len(), self.inner.max_size)
        }
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use crate::make_engine;

    const PRELUDE: &str = "local cache = require(\"lib:cache\")\nlocal time = require(\"std:time\")\n";

    fn engine() -> Lua {
        make_engine().unwrap()
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(format!("{PRELUDE}{code}")).eval().unwrap()
    }

    #[test]
    fn set_then_get_round_trips() {
        let v: String = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            c:set("k", "v")
            return c:get("k")
        "#,
        );
        assert_eq!(v, "v");
    }

    #[test]
    fn a_stored_table_comes_back_as_the_same_table() {
        let same: bool = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            local t = { f = function() end }
            c:set("k", t)
            return c:get("k") == t
        "#,
        );
        assert!(same);
    }

    #[test]
    fn miss_returns_nil() {
        let (v, count): (Option<String>, usize) = eval(
            r##"
            local c = assert(cache.Cache.new(8))
            return c:get("absent"), select("#", c:get("absent"))
        "##,
        );
        assert!(v.is_none());
        assert_eq!(count, 1);
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        // Capacity 2: a, b; touch a; c goes in and b (the untouched one) goes out.
        let (a, b, c): (Option<String>, Option<String>, Option<String>) = eval(
            r#"
            local c = assert(cache.Cache.new(2))
            c:set("a", "A")
            c:set("b", "B")
            c:get("a")
            c:set("c", "C")
            return c:get("a"), c:get("b"), c:get("c")
        "#,
        );
        assert_eq!(a.as_deref(), Some("A"));
        assert!(b.is_none(), "b should have been evicted");
        assert_eq!(c.as_deref(), Some("C"));
    }

    #[test]
    fn overwrite_replaces_without_eviction() {
        let (v, evictions): (String, u64) = eval(
            r#"
            local c = assert(cache.Cache.new(2))
            c:set("k", "first")
            c:set("k", "second")
            return c:get("k"), c:stats().evictions
        "#,
        );
        assert_eq!(v, "second");
        assert_eq!(evictions, 0, "overwriting a key is not an eviction");
    }

    #[test]
    fn delete_reports_presence_and_removes() {
        let (first, second, present): (bool, bool, Option<String>) = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            c:set("k", "v")
            local first = c:delete("k")
            local second = c:delete("k")
            return first, second, c:get("k")
        "#,
        );
        assert!(first, "first delete finds the key");
        assert!(!second, "second delete finds nothing");
        assert!(present.is_none());
    }

    #[test]
    fn clear_empties_but_keeps_stats() {
        let (len, hits): (usize, u64) = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            c:set("a", 1)
            c:get("a")
            c:clear()
            return c:len(), c:stats().hits
        "#,
        );
        assert_eq!(len, 0);
        assert_eq!(hits, 1, "clear keeps cumulative stats");
    }

    #[test]
    fn stats_track_hits_misses_evictions() {
        let (hits, misses, evictions): (u64, u64, u64) = eval(
            r#"
            local c = assert(cache.Cache.new(1))
            c:set("a", 1)
            c:get("a")
            c:get("a")
            c:get("nope")
            c:set("b", 2)
            local s = c:stats()
            return s.hits, s.misses, s.evictions
        "#,
        );
        assert_eq!((hits, misses, evictions), (2, 1, 1));
    }

    /// Runs `setup`, waits 20ms, then evaluates `after`. The wait is here because a script has no synchronous
    /// sleep.
    fn after_a_wait<T: mlua::FromLuaMulti>(setup: &str, after: &str) -> T {
        let lua = engine();
        lua.load(format!("{PRELUDE}{setup}")).exec().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        lua.load(after).eval().unwrap()
    }

    #[test]
    fn ttl_entry_expires_lazily() {
        let (v, misses, evictions): (Option<String>, u64, u64) = after_a_wait(
            r#"
            c = assert(cache.Cache.new(8))
            c:set("k", "v", time.Duration.from_millis(1))
            "#,
            r#"
            local v = c:get("k")
            local s = c:stats()
            return v, s.misses, s.evictions
            "#,
        );
        assert!(v.is_none(), "expired entry reads as a miss");
        assert_eq!(misses, 1);
        assert_eq!(evictions, 1, "expiry counts as an eviction");
    }

    #[test]
    fn the_default_ttl_applies_to_every_entry() {
        let (v, len): (Option<String>, usize) = after_a_wait(
            r#"
            c = assert(cache.Cache.new(8, time.Duration.from_millis(1)))
            c:set("k", "v")
            c:set("long", "v", time.Duration.from_secs(60))
            "#,
            r#"return c:get("k"), c:len()"#,
        );
        assert!(v.is_none());
        assert_eq!(len, 1, "the entry with its own longer ttl is still there");
    }

    #[test]
    fn a_zero_ttl_pins_against_the_default() {
        let v: Option<String> = after_a_wait(
            r#"
            c = assert(cache.Cache.new(8, time.Duration.from_millis(1)))
            c:set("pinned", "v", time.Duration.zero())
            "#,
            r#"return c:get("pinned")"#,
        );
        assert_eq!(v.as_deref(), Some("v"), "a zero ttl pins past the default");
    }

    #[test]
    fn a_ttl_past_the_end_of_the_clock_is_no_expiry_not_a_panic() {
        // the longest duration there is (about 584 years in nanoseconds) may be past what `Instant` can add
        let (per_call, default, zero): (Option<String>, Option<String>, Option<String>) = eval(
            r#"
            local forever = assert(time.Duration.parse("5000000h"))
            local a = assert(cache.Cache.new(8))
            a:set("k", "v", forever)
            local b = assert(cache.Cache.new(8, forever))
            b:set("k", "v")
            local z = assert(cache.Cache.new(8, time.Duration.zero()))
            z:set("k", "v")
            return a:get("k"), b:get("k"), z:get("k")
        "#,
        );
        assert_eq!(per_call.as_deref(), Some("v"));
        assert_eq!(default.as_deref(), Some("v"));
        assert_eq!(zero.as_deref(), Some("v"), "a zero default is no default");
    }

    #[test]
    fn len_purges_expired_entries() {
        let (len, evictions): (usize, u64) = after_a_wait(
            r#"
            c = assert(cache.Cache.new(8))
            c:set("slow", "v", time.Duration.from_millis(1))
            c:set("kept", "v")
            "#,
            r"local n = c:len() return n, c:stats().evictions",
        );
        assert_eq!(len, 1, "only the live entry is counted");
        assert_eq!(evictions, 1, "the swept entry counts as an eviction");
    }

    #[test]
    fn on_evict_fires_for_capacity_and_expiry_only() {
        let seen: Vec<String> = eval(
            r#"
            local seen = {}
            local c = assert(cache.Cache.new(1, nil, function(k, v) table.insert(seen, k .. "=" .. tostring(v)) end))
            c:set("a", "A")
            c:set("a", "A2")    -- overwrite: no callback
            c:delete("a")       -- delete: no callback
            c:set("b", "B")
            c:set("c", "C")     -- no room: b goes, and the callback hears "b=B"
            c:clear()           -- clear: no callback
            return seen
        "#,
        );
        assert_eq!(seen, ["b=B"]);

        let expired: Vec<String> = after_a_wait(
            r#"
            seen = {}
            c = assert(cache.Cache.new(8, time.Duration.from_millis(1), function(k, v)
                table.insert(seen, tostring(k) .. "=" .. v)
            end))
            c:set(1, "one")
            c:set(true, "yes", time.Duration.zero())
            "#,
            r"c:len() return seen",
        );
        assert_eq!(expired, ["1=one"]);
    }

    #[test]
    fn on_evict_may_reenter_the_cache() {
        // the callback runs with the lock released: a held lock would deadlock here
        let (key, value, live): (String, String, usize) = eval(
            r#"
            local seen
            local c
            c = assert(cache.Cache.new(1, nil, function(k, v)
                seen = { key = k, value = v, len = c:len() }
            end))
            c:set("a", "A")
            c:set("b", "B")
            return seen.key, seen.value, seen.len
        "#,
        );
        assert_eq!((key.as_str(), value.as_str(), live), ("a", "A", 1));
    }

    #[test]
    fn what_on_evict_raises_is_raised_by_the_call() {
        let (ok, text): (bool, String) = eval(
            r#"
            local c = assert(cache.Cache.new(1, nil, function() error("the callback failed") end))
            c:set("a", "A")
            local ok, err = pcall(c.set, c, "b", "B")
            return ok, tostring(err)
        "#,
        );
        assert!(!ok);
        assert!(text.contains("the callback failed"), "{text}");
    }

    #[test]
    fn numeric_and_boolean_keys_work() {
        let (n, b, zero, nan, bytes): (String, String, String, String, String) = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            c:set(1, "one")
            c:set(true, "yes")
            c:set(0, "zero")
            c:set(0 / 0, "nan")
            c:set("\255", "bytes")
            return c:get(1.0), c:get(true), c:get(-0), c:get(0 / 0), c:get("\255")
        "#,
        );
        assert_eq!((n.as_str(), b.as_str()), ("one", "yes"), "1 and 1.0 are the same key");
        assert_eq!((zero.as_str(), nan.as_str(), bytes.as_str()), ("zero", "nan", "bytes"));
    }

    #[test]
    fn a_number_and_its_text_are_different_keys() {
        let (number, text): (String, String) = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            c:set(1, "number")
            c:set("1", "text")
            return c:get(1), c:get("1")
        "#,
        );
        assert_eq!((number.as_str(), text.as_str()), ("number", "text"));
    }

    #[test]
    fn new_requires_max_size() {
        let lua = engine();
        for arguments in ["", "{}", "-1", "1.5", "'8'"] {
            let err = lua
                .load(format!(r#"return require("lib:cache").Cache.new({arguments})"#))
                .eval::<mlua::Value>()
                .expect_err(arguments)
                .to_string();
            assert!(err.contains("lib:cache.Cache.new: argument #1 'max_size'"), "{arguments}: {err}");
        }
    }

    #[test]
    fn new_refuses_a_size_out_of_range() {
        // a huge size is an error, not gigabytes reserved or a panic in hashbrown
        let (zero, kind, huge, max, text): (mlua::Value, String, String, u32, String) = eval(
            r"
            local zero, e = cache.Cache.new(0)
            local _, h = cache.Cache.new(1000000000)
            return zero, e.kind, h.kind, h.max, tostring(h)
        ",
        );
        assert!(zero.is_nil());
        assert_eq!((kind.as_str(), huge.as_str(), max), ("out_of_range", "out_of_range", 1 << 24));
        assert_eq!(
            text,
            "lib:cache.Cache.new: out_of_range: max_size must be from 1 to 16777216, got 1000000000"
        );
    }

    #[test]
    fn a_key_that_is_not_a_scalar_is_refused() {
        let lua = engine();
        for (call, place) in [
            ("c:get({})", "lib:cache.Cache:get: argument #1 'key'"),
            ("c:set(function() end, 1)", "lib:cache.Cache:set: argument #1 'key'"),
            ("c:delete(nil)", "lib:cache.Cache:delete: argument #1 'key'"),
        ] {
            let err = lua
                .load(format!("{PRELUDE}local c = assert(cache.Cache.new(8))\nreturn {call}"))
                .eval::<mlua::Value>()
                .expect_err(call)
                .to_string();
            assert!(err.contains(place), "{call}: {err}");
        }
    }

    #[test]
    fn set_refuses_a_nil_value() {
        let (ok, text, len): (bool, String, usize) = eval(
            r#"
            local c = assert(cache.Cache.new(8))
            local ok, e = pcall(function() c:set("k", nil) end)
            return ok, tostring(e), c:len()
        "#,
        );
        assert!(!ok);
        assert!(text.contains("lib:cache.Cache:set: nil_value: the value must not be nil"), "{text}");
        assert_eq!(len, 0);
    }

    #[test]
    fn tostring_reports_len_and_capacity() {
        let s: String = eval(
            r#"
            local c = assert(cache.Cache.new(4))
            c:set("a", 1)
            return tostring(c)
        "#,
        );
        assert_eq!(s, "Cache(len=1, max_size=4)");
    }
}
