//! Bounded in-memory cache — `lib:cache`.
//!
//! A `Cache` is a fixed-capacity map with least-recently-used eviction and
//! optional per-entry time-to-live. It is the tool for memoisation, request
//! deduplication, and hot-path lookups — anywhere a plain `table` would grow
//! without bound. The surface is the cross-runtime consensus shape:
//! `Cache.new({ max_size, ttl })` returning a handle with `get`, `set`,
//! `delete`, `clear`, `len`, plus a `stats` snapshot.
//!
//! ## Eviction
//!
//! Capacity is mandatory — this is a *bounded* cache, so `max_size` (≥ 1) is
//! required. When a `set` would exceed it, the least-recently-used entry is
//! dropped. `get` and `set` both count as a use and move the entry to the
//! most-recently-used end, so the entry evicted is always the one untouched
//! the longest. This backs onto the `lru` crate's `LruCache`.
//!
//! ## Expiry
//!
//! TTL is layered on top, not delegated: each entry carries an optional
//! `Instant` deadline and expiry is **lazy** — an entry is dropped the moment
//! it is observed past its deadline (`get`, or the sweep `len` performs), never
//! by a background thread. A synchronous embedding has no clock to hang a timer
//! on, and lazy expiry needs none: an entry that is never read again costs only
//! its slot, which `max_size` already bounds. `ttl` is in seconds and may be
//! fractional. The cache-wide default from the constructor applies to every
//! entry unless `set` overrides it per call.
//!
//! ## Statistics and the eviction callback
//!
//! `stats()` reports cumulative `hits`, `misses`, and `evictions`. An
//! "eviction" is the cache dropping an entry on its own initiative — capacity
//! pressure or TTL expiry; explicit `delete` / `clear` / overwrite are
//! caller-driven and count for none of it. The optional `on_evict` callback
//! fires on exactly those self-initiated drops, with `(key, value)`. It runs
//! *after* the cache's own lock is released, so a callback that reaches back
//! into the same cache neither deadlocks nor observes a half-updated map.

use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use lru::LruCache;
use mlua::{Function, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use parking_lot::Mutex;

use crate::lua::facade::Module;

/// The bit pattern every NaN key collapses to. NaN is never equal to itself,
/// so without canonicalisation two NaN keys would never collide *or* round-trip
/// — degenerate either way. One quiet-NaN pattern keeps NaN-keyed entries at
/// least self-consistent.
const CANONICAL_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A hashable cache key. Lua's hashable scalars — strings, numbers, booleans —
/// are the keys a cache is reached by; tables and functions key by identity in
/// Lua, which is a footgun for a cache (two equal-looking tables miss each
/// other), so they are rejected at the boundary rather than silently
/// identity-keyed.
#[derive(Clone, PartialEq, Eq, Hash)]
enum CacheKey {
    Str(Vec<u8>),
    /// `f64::to_bits`, normalised so `0.0`/`-0.0` and the various NaNs each map
    /// to one key. Lua has no integer subtype, so `1` and `1.0` are the same
    /// number and land on the same bits.
    Num(u64),
    Bool(bool),
}

/// Normalises an `f64` to its canonical key bits: `±0.0` collapse to one zero
/// and every NaN to one quiet NaN, so equal numbers always share a key.
fn canonical_num_bits(n: f64) -> u64 {
    if n == 0.0 {
        0
    } else if n.is_nan() {
        CANONICAL_NAN_BITS
    } else {
        n.to_bits()
    }
}

/// Derives a hashable [`CacheKey`] from a Lua value, rejecting types a cache
/// cannot sanely key on. `method` (e.g. `"cache.get"`) prefixes the error so a
/// caller sees which call rejected the key.
fn cache_key(v: &Value, method: &str) -> mlua::Result<CacheKey> {
    let key = match v {
        Value::String(s) => CacheKey::Str(s.as_bytes().to_vec()),
        Value::Integer(i) => CacheKey::Num(canonical_num_bits(*i as f64)),
        Value::Number(n) => CacheKey::Num(canonical_num_bits(*n)),
        Value::Boolean(b) => CacheKey::Bool(*b),
        other => {
            return Err(mlua::Error::external(format!(
                "{method}: key must be a string, number, or boolean, got {}",
                other.type_name()
            )));
        }
    };
    Ok(key)
}

/// Reconstructs the Lua value a [`CacheKey`] was derived from, for handing back
/// to the eviction callback. String bytes round-trip exactly; a number returns
/// as its `f64`; a normalised zero comes back as `0.0`.
fn key_to_value(lua: &Lua, key: &CacheKey) -> mlua::Result<Value> {
    Ok(match key {
        CacheKey::Str(b) => Value::String(lua.create_string(b)?),
        CacheKey::Num(bits) => Value::Number(f64::from_bits(*bits)),
        CacheKey::Bool(b) => Value::Boolean(*b),
    })
}

// ---------------------------------------------------------------------------
// Entries and state
// ---------------------------------------------------------------------------

/// A stored value and its optional expiry deadline.
struct Entry {
    value: Value,
    expires_at: Option<Instant>,
}

impl Entry {
    fn is_expired(&self, now: Instant) -> bool {
        self.expires_at.is_some_and(|deadline| now >= deadline)
    }
}

/// The mutable interior, guarded by one lock: the LRU map plus the cumulative
/// counters. Config (`max_size`, `default_ttl`, `on_evict`) is immutable after
/// construction and lives on [`Cache`] outside the lock. Its methods own the
/// cache policy and the counter bookkeeping; each returns the entries it
/// dropped so the caller can fire `on_evict` once the lock is released.
struct CacheState {
    map: LruCache<CacheKey, Entry>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl CacheState {
    /// Look up a key. A live entry is promoted to most-recently-used and counts
    /// a hit; a plain miss counts a miss; an entry found past its deadline is
    /// dropped (counted as both a miss and an eviction). Returns the value (nil
    /// on miss or expiry) and any entry the lookup evicted.
    fn lookup(&mut self, key: &CacheKey, now: Instant) -> (Value, Option<(CacheKey, Entry)>) {
        if matches!(self.map.peek(key), Some(e) if e.is_expired(now)) {
            let dropped = self.map.pop_entry(key);
            if dropped.is_some() {
                self.evictions += 1;
            }
            self.misses += 1;
            return (Value::Nil, dropped);
        }
        match self.map.get(key) {
            Some(e) => {
                let value = e.value.clone();
                self.hits += 1;
                (value, None)
            }
            None => {
                self.misses += 1;
                (Value::Nil, None)
            }
        }
    }

    /// Insert `entry` as most-recently-used. Returns the entry forced out by
    /// capacity (counted as an eviction) — never the old value of an overwritten
    /// key, which is caller-driven and not an eviction.
    fn insert(&mut self, key: CacheKey, entry: Entry) -> Option<(CacheKey, Entry)> {
        match self.map.push(key.clone(), entry) {
            Some((old_key, old_entry)) if old_key != key => {
                self.evictions += 1;
                Some((old_key, old_entry))
            }
            _ => None,
        }
    }

    /// The active-expiry sweep: drop every entry past its deadline, counting
    /// each as an eviction, and return them.
    fn sweep_expired(&mut self, now: Instant) -> Vec<(CacheKey, Entry)> {
        let stale: Vec<CacheKey> = self
            .map
            .iter()
            .filter(|(_, e)| e.is_expired(now))
            .map(|(k, _)| k.clone())
            .collect();
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

/// Upper bound on `max_size`. `LruCache::new` eagerly preallocates the index
/// (~16 bytes per slot), so an unchecked `max_size` turns construction into a
/// multi-gigabyte reservation — or, past hashbrown's layout ceiling, a panic.
/// Capping the eager reservation at ~256 MiB (the budget `lib:compress` uses)
/// folds both into a clean `(nil, err)`; 16M live entries is already well past
/// what an in-memory cache is for.
const MAX_CAPACITY: usize = 1 << 24;

/// The Lua-facing cache handle.
pub struct Cache {
    state: Mutex<CacheState>,
    on_evict: Option<Function>,
    default_ttl: Option<Duration>,
    max_size: usize,
}

impl Cache {
    /// Builds a cache from the constructor options table. `max_size` (≥ 1) is
    /// required; `ttl` (seconds) and `on_evict` (`function(key, value)`) are
    /// optional. A `ttl` ≤ 0 or non-finite means no default expiry.
    fn build(_: &Lua, opts: Table) -> Result<Self, String> {
        let max_size = match opts.get::<Option<i64>>("max_size") {
            Ok(Some(n)) => n,
            Ok(None) => return Err("max_size is required".into()),
            Err(_) => return Err("max_size must be a positive integer".into()),
        };
        let cap = usize::try_from(max_size)
            .ok()
            .and_then(NonZeroUsize::new)
            .ok_or_else(|| format!("max_size must be at least 1, got {max_size}"))?;
        if cap.get() > MAX_CAPACITY {
            return Err(format!(
                "max_size must be at most {MAX_CAPACITY}, got {max_size}"
            ));
        }

        let default_ttl = match opts.get::<Option<f64>>("ttl") {
            Ok(ttl) => positive_duration(ttl),
            Err(_) => return Err("ttl must be a number of seconds".into()),
        };

        let on_evict = opts
            .get::<Option<Function>>("on_evict")
            .map_err(|_| "on_evict must be a function".to_string())?;

        Ok(Self {
            state: Mutex::new(CacheState {
                map: LruCache::new(cap),
                hits: 0,
                misses: 0,
                evictions: 0,
            }),
            on_evict,
            default_ttl,
            max_size: cap.get(),
        })
    }

    /// Resolves the expiry deadline for a `set`: an explicit `ttl` argument
    /// overrides the cache default — omit (`None`) for the default, pass a
    /// positive number for a custom expiry, pass ≤ 0 to pin the entry. Returns
    /// `None` (never expires) when no positive duration applies or the deadline
    /// would overflow the clock.
    fn deadline_for(&self, ttl: Option<f64>) -> Option<Instant> {
        let dur = match ttl {
            None => self.default_ttl,
            Some(secs) => positive_duration(Some(secs)),
        };
        dur.and_then(|d| Instant::now().checked_add(d))
    }

    /// Invokes `on_evict` for each dropped entry, once the state lock is no
    /// longer held — so a callback may reach back into the cache without
    /// deadlocking. A `None` callback simply drops the entries.
    fn notify_evicted(
        &self,
        lua: &Lua,
        evicted: impl IntoIterator<Item = (CacheKey, Entry)>,
    ) -> mlua::Result<()> {
        if let Some(cb) = &self.on_evict {
            for (key, entry) in evicted {
                cb.call::<()>((key_to_value(lua, &key)?, entry.value))?;
            }
        }
        Ok(())
    }
}

/// A positive `ttl` in seconds as a `Duration`; anything else — absent, zero,
/// negative, NaN, infinite, or large enough to overflow `Duration` — is `None`
/// ("no expiry"). The fallible conversion folds the cases the `> 0.0` guard
/// lets through (infinite, overflow) into that same `None`, so a finite,
/// in-range positive is the only input that yields a deadline.
fn positive_duration(ttl: Option<f64>) -> Option<Duration> {
    match ttl {
        Some(secs) if secs > 0.0 => Duration::try_from_secs_f64(secs).ok(),
        _ => None,
    }
}

impl UserData for Cache {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // get — look up under the lock, then notify outside it.
        methods.add_method("get", |lua, this, key: Value| {
            let ck = cache_key(&key, "cache.get")?;
            let now = Instant::now();
            let (value, evicted) = this.state.lock().lookup(&ck, now);
            this.notify_evicted(lua, evicted)?;
            Ok(value)
        });

        // set — insert as MRU, evicting the LRU entry if over capacity.
        methods.add_method(
            "set",
            |lua, this, (key, value, ttl): (Value, Value, Option<f64>)| {
                let ck = cache_key(&key, "cache.set")?;
                if value.is_nil() {
                    return Err(mlua::Error::external(
                        "cache.set: value must not be nil (a nil value is indistinguishable from a miss)",
                    ));
                }
                let entry = Entry {
                    value,
                    expires_at: this.deadline_for(ttl),
                };
                let evicted = this.state.lock().insert(ck, entry);
                this.notify_evicted(lua, evicted)?;
                Ok(())
            },
        );

        // delete — caller-driven, so no eviction accounting or callback.
        methods.add_method("delete", |_, this, key: Value| {
            let ck = cache_key(&key, "cache.delete")?;
            Ok(this.state.lock().map.pop(&ck).is_some())
        });

        // clear — caller-driven; entries go, cumulative stats stay.
        methods.add_method("clear", |_, this, ()| {
            this.state.lock().map.clear();
            Ok(())
        });

        // len — sweep expired entries (active expiry), then report live count.
        methods.add_method("len", |lua, this, ()| {
            let now = Instant::now();
            let (live, evicted) = {
                let mut st = this.state.lock();
                let evicted = st.sweep_expired(now);
                (st.map.len(), evicted)
            };
            this.notify_evicted(lua, evicted)?;
            Ok(live)
        });

        // stats — a snapshot of the cumulative counters.
        methods.add_method("stats", |lua, this, ()| {
            let st = this.state.lock();
            let t = lua.create_table()?;
            t.set("hits", st.hits)?;
            t.set("misses", st.misses)?;
            t.set("evictions", st.evictions)?;
            Ok(t)
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "Cache(len={}, max_size={})",
                this.state.lock().map.len(),
                this.max_size
            ))
        });
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:cache` surface through the facade. `Cache` is a
/// constructor-bearing type: `new` is recorded and registered on its sub-table,
/// each method is recorded for the header while its runtime registration stays
/// in `impl UserData for Cache`.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:cache")?;

    m.type_("Cache", |t| {
        t.try_constructor(
            "new",
            &[("options", "LuaValue")],
            "Create a bounded LRU cache from an options table: `max_size` (required, ≥ 1) caps the entry count; `ttl` (optional, seconds, fractional allowed) sets a default per-entry expiry; `on_evict` (optional, `function(key, value)`) fires when the cache drops an entry itself — capacity eviction or TTL expiry — but not on `delete`, `clear`, or overwrite.",
            |lua, opts: Table| Cache::build(lua, opts),
        )?
        .method(
            "get",
            &[("key", "LuaValue")],
            "LuaValue",
            "Look up `key`, returning the stored value or nil on a miss or expiry. A hit promotes the entry to most-recently-used and counts a hit; a miss or a freshly-expired entry counts a miss (an expired entry is also dropped, counting as an eviction). Key must be a string, number, or boolean.",
        )
        .method(
            "set",
            &[("key", "LuaValue"), ("value", "LuaValue"), ("ttl", "number?")],
            "()",
            "Store `value` under `key` as most-recently-used, evicting the least-recently-used entry when over `max_size`. `ttl` (seconds) overrides the cache default: omit it for the default, pass > 0 for a custom expiry, pass ≤ 0 to pin the entry so it never expires. Key must be a string, number, or boolean; `value` must not be nil.",
        )
        .method(
            "delete",
            &[("key", "LuaValue")],
            "bool",
            "Remove `key`, returning true if it was present. Caller-driven, so it never fires `on_evict` nor counts as an eviction.",
        )
        .method(
            "clear",
            &[],
            "()",
            "Remove every entry. Caller-driven: no `on_evict`, no eviction count; the cumulative statistics are preserved.",
        )
        .method(
            "len",
            &[],
            "number",
            "Number of live entries. Expired entries are purged first (each counts as an eviction and fires `on_evict`), so the count never includes stale data.",
        )
        .method(
            "stats",
            &[],
            "LuaValue",
            "Snapshot of the cumulative counters as a table with `hits`, `misses`, and `evictions`. Counts persist across `clear`.",
        );
        Ok(())
    })?;

    Ok(m)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:cache")"#).eval().unwrap();
        lua.globals().set("cache", m).unwrap();
        lua
    }

    #[test]
    fn set_then_get_round_trips() {
        let lua = engine();
        let v: String = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                c:set("k", "v")
                return c:get("k")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(v, "v");
    }

    #[test]
    fn miss_returns_nil() {
        let lua = engine();
        let v: Option<String> = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                return c:get("absent")
                "#,
            )
            .eval()
            .unwrap();
        assert!(v.is_none());
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        // Capacity 2: insert a, b; touch a; insert c → b (the untouched one) goes.
        let lua = engine();
        let (a, b, c): (Option<String>, Option<String>, Option<String>) = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 2 }))
                c:set("a", "A")
                c:set("b", "B")
                c:get("a")          -- a is now most-recently-used
                c:set("c", "C")     -- evicts b
                return c:get("a"), c:get("b"), c:get("c")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a.as_deref(), Some("A"));
        assert!(b.is_none(), "b should have been evicted");
        assert_eq!(c.as_deref(), Some("C"));
    }

    #[test]
    fn overwrite_replaces_without_eviction() {
        let lua = engine();
        let (v, evictions): (String, u64) = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 2 }))
                c:set("k", "first")
                c:set("k", "second")
                return c:get("k"), c:stats().evictions
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(v, "second");
        assert_eq!(evictions, 0, "overwriting a key is not an eviction");
    }

    #[test]
    fn delete_reports_presence_and_removes() {
        let lua = engine();
        let (first, second, present): (bool, bool, Option<String>) = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                c:set("k", "v")
                local first = c:delete("k")
                local second = c:delete("k")
                return first, second, c:get("k")
                "#,
            )
            .eval()
            .unwrap();
        assert!(first, "first delete finds the key");
        assert!(!second, "second delete finds nothing");
        assert!(present.is_none());
    }

    #[test]
    fn clear_empties_but_keeps_stats() {
        let lua = engine();
        let (len, hits): (usize, u64) = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                c:set("a", 1)
                c:get("a")          -- one hit
                c:clear()
                return c:len(), c:stats().hits
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(len, 0);
        assert_eq!(hits, 1, "clear keeps cumulative stats");
    }

    #[test]
    fn stats_track_hits_misses_evictions() {
        let lua = engine();
        let (hits, misses, evictions): (u64, u64, u64) = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 1 }))
                c:set("a", 1)
                c:get("a")          -- hit
                c:get("a")          -- hit
                c:get("nope")       -- miss
                c:set("b", 2)       -- evicts a
                local s = c:stats()
                return s.hits, s.misses, s.evictions
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(hits, 2);
        assert_eq!(misses, 1);
        assert_eq!(evictions, 1);
    }

    #[test]
    fn ttl_entry_expires_lazily() {
        // A 1ms ttl is past after a 20ms sleep: the next get is a miss and an
        // eviction. Timing lives in Rust because Lua has no synchronous sleep.
        let lua = engine();
        lua.load(
            r#"
            c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("k", "v", 0.001)
            "#,
        )
        .exec()
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));

        let (v, misses, evictions): (Option<String>, u64, u64) = lua
            .load(
                r#"
                local v = c:get("k")
                local s = c:stats()
                return v, s.misses, s.evictions
                "#,
            )
            .eval()
            .unwrap();
        assert!(v.is_none(), "expired entry reads as a miss");
        assert_eq!(misses, 1);
        assert_eq!(evictions, 1, "expiry counts as an eviction");
    }

    #[test]
    fn negative_ttl_pins_against_default() {
        // Default ttl 1ms, but this entry is set with ttl <= 0 → never expires.
        let lua = engine();
        lua.load(
            r#"
            c = assert(cache.Cache.new({ max_size = 8, ttl = 0.001 }))
            c:set("pinned", "v", -1)
            "#,
        )
        .exec()
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));

        let v: Option<String> = lua.load(r#"return c:get("pinned")"#).eval().unwrap();
        assert_eq!(v.as_deref(), Some("v"), "ttl <= 0 pins past the default");
    }

    #[test]
    fn overflow_ttl_is_no_expiry_not_panic() {
        // A finite ttl past Duration's ~1.84e19 s ceiling must fold to "no
        // expiry" like inf/NaN, never panic in from_secs_f64. Reached via both
        // the per-call ttl and the constructor default.
        let lua = engine();
        let (per_call, default): (Option<String>, Option<String>) = lua
            .load(
                r#"
                local a = assert(cache.Cache.new({ max_size = 8 }))
                a:set("k", "v", 2e19)                       -- per-call overflow
                local b = assert(cache.Cache.new({ max_size = 8, ttl = 2e19 }))
                b:set("k", "v")                             -- default overflow
                return a:get("k"), b:get("k")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            per_call.as_deref(),
            Some("v"),
            "per-call overflow ttl never expires"
        );
        assert_eq!(
            default.as_deref(),
            Some("v"),
            "default overflow ttl never expires"
        );
    }

    #[test]
    fn len_purges_expired_entries() {
        let lua = engine();
        lua.load(
            r#"
            c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("slow", "v", 0.001)
            c:set("kept", "v")          -- no ttl
            "#,
        )
        .exec()
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));

        let (len, evictions): (usize, u64) = lua
            .load(r#"local n = c:len() return n, c:stats().evictions"#)
            .eval()
            .unwrap();
        assert_eq!(len, 1, "only the live entry is counted");
        assert_eq!(evictions, 1, "the swept entry counts as an eviction");
    }

    #[test]
    fn on_evict_fires_for_capacity_and_expiry_only() {
        // Capacity eviction and TTL expiry call back; delete/clear/overwrite do
        // not. The callback records (key, value) pairs into a Lua table.
        let lua = engine();
        let log: Vec<String> = lua
            .load(
                r#"
                local seen = {}
                local c = assert(cache.Cache.new({
                    max_size = 1,
                    on_evict = function(k, v) table.insert(seen, k .. "=" .. tostring(v)) end,
                }))
                c:set("a", "A")
                c:set("a", "A2")    -- overwrite: no callback
                c:delete("a")       -- delete: no callback
                c:set("b", "B")
                c:set("c", "C")     -- capacity eviction of b → callback "b=B"
                c:clear()           -- clear: no callback
                return seen
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(log, vec!["b=B".to_string()]);
    }

    #[test]
    fn on_evict_may_reenter_the_cache() {
        // The callback runs after the lock is dropped, so reaching back into the
        // same cache from on_evict neither deadlocks nor panics. The callback
        // reads through the handle (len/stats); a held lock would deadlock here.
        let lua = engine();
        let (key, value, live): (String, String, usize) = lua
            .load(
                r#"
                local seen
                local c
                c = assert(cache.Cache.new({
                    max_size = 1,
                    on_evict = function(k, v)
                        seen = { key = k, value = v, len = c:len() }
                    end,
                }))
                c:set("a", "A")
                c:set("b", "B")     -- evicts a → callback re-enters with reads
                return seen.key, seen.value, seen.len
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(key, "a");
        assert_eq!(value, "A");
        assert_eq!(live, 1, "the surviving entry is observable mid-callback");
    }

    #[test]
    fn numeric_and_boolean_keys_work() {
        let lua = engine();
        let (n, b): (String, String) = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                c:set(1, "one")
                c:set(true, "yes")
                return c:get(1.0), c:get(true)   -- 1 and 1.0 are the same key
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(n, "one");
        assert_eq!(b, "yes");
    }

    #[test]
    fn new_requires_max_size() {
        let lua = engine();
        let (val, err): (Option<mlua::Value>, Option<String>) =
            lua.load(r#"return cache.Cache.new({})"#).eval().unwrap();
        assert!(val.is_none() || matches!(val, Some(mlua::Value::Nil)));
        let err = err.unwrap();
        assert!(err.starts_with("Cache.new: "), "bad prefix: {err}");
        assert!(err.contains("max_size is required"), "bad body: {err}");
    }

    #[test]
    fn new_rejects_zero_max_size() {
        let lua = engine();
        let err: String = lua
            .load(r#"local _, e = cache.Cache.new({ max_size = 0 }) return e"#)
            .eval()
            .unwrap();
        assert!(err.contains("max_size must be at least 1"), "{err}");
    }

    #[test]
    fn new_rejects_max_size_over_ceiling() {
        // A huge max_size must hit the (nil, err) channel, not preallocate
        // gigabytes or panic in hashbrown's capacity math.
        let lua = engine();
        let (val, err): (Option<mlua::Value>, Option<String>) = lua
            .load(r#"return cache.Cache.new({ max_size = 1000000000000000000 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none() || matches!(val, Some(mlua::Value::Nil)));
        let err = err.unwrap();
        assert!(err.contains("max_size must be at most"), "{err}");
    }

    #[test]
    fn get_rejects_unhashable_key() {
        let lua = engine();
        let err: String = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                local ok, e = pcall(function() return c:get({}) end)
                return tostring(e)
                "#,
            )
            .eval()
            .unwrap();
        assert!(err.contains("cache.get:"), "{err}");
        assert!(
            err.contains("must be a string, number, or boolean"),
            "{err}"
        );
    }

    #[test]
    fn set_rejects_nil_value() {
        let lua = engine();
        let err: String = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 8 }))
                local ok, e = pcall(function() c:set("k", nil) end)
                return tostring(e)
                "#,
            )
            .eval()
            .unwrap();
        assert!(err.contains("cache.set:"), "{err}");
        assert!(err.contains("must not be nil"), "{err}");
    }

    #[test]
    fn tostring_reports_len_and_capacity() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local c = assert(cache.Cache.new({ max_size = 4 }))
                c:set("a", 1)
                return tostring(c)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "Cache(len=1, max_size=4)");
    }
}
