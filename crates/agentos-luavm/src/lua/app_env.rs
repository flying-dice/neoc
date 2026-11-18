//! The environment an untrusted script runs in.
//!
//! Lua 5.1 gives every function an environment table (`setfenv`); a chunk loaded with
//! `Chunk::set_environment` and every closure created inside it see only that table as their globals.
//! [`new_env`] builds it, [`freeze`] seals it:
//!
//! - the **base functions** a script may call are copied in one by one (an allow-list: a function a Lua
//!   upgrade adds is not available until someone adds it here): no `load*`, `dofile`, `getfenv`, `setfenv`,
//!   `newproxy`, `collectgarbage`, `module`, `require`, `print` and no `io`/`os`/`debug`/`package`;
//! - the **libraries** (`string`, `table`, `math`, `coroutine`, `bit`) are private copies (`coroutine.resume`
//!   and `wrap` are versions that let an async host call run inside a coroutine of the script's own, see
//!   `async_coroutines.lua`; `table.sort` is the counted one of `sort.lua`), so the host may add
//!   to them (the prelude's `table.clone`, `utf8`) before [`freeze`], and the script can never reach the
//!   engine's own shared tables;
//! - [`freeze`] then replaces every library in the environment with an empty read-only proxy whose
//!   `__index` is the copy: assigning to it fails (the key is absent, so `__newindex` always fires), its
//!   metatable is locked (`getmetatable` returns a string, `setmetatable` is refused), and `rawset` is
//!   replaced by a version that refuses the proxies.
//!
//! Known limits of a proxy: `pairs(string)` and `next(string)` see no entries (Lua 5.1 has no `__pairs`), and
//! `rawget(string, "len")` is `nil`; the functions are all reachable by name.
//!
//! The string metatable is a different matter: it is shared by every string in the engine, and
//! [`make_bare_engine`](crate::lua::sandbox::make_bare_engine) locks it.

use std::sync::Arc;

use mlua::{Lua, Table, Value};
use parking_lot::Mutex;

/// Base functions a script gets, copied from the engine's globals (after `pcall`/`xpcall` were replaced
/// by their yield-safe versions). `rawset` is installed by [`freeze`], not copied.
const BASE_FUNCTIONS: &[&str] = &[
    "assert",
    "error",
    "getmetatable",
    "ipairs",
    "next",
    "pairs",
    "pcall",
    "rawequal",
    "rawget",
    "select",
    "setmetatable",
    "tonumber",
    "tostring",
    "type",
    "unpack",
    "xpcall",
];

/// Libraries a script gets a private copy of.
const LIBRARIES: &[&str] = &["string", "table", "math", "coroutine", "bit"];

/// A shallow copy of `source`: a new table holding the same functions.
fn copy(lua: &Lua, source: &Table) -> mlua::Result<Table> {
    let copy = lua.create_table()?;
    for pair in source.pairs::<Value, Value>() {
        let (key, value) = pair?;
        copy.raw_set(key, value)?;
    }
    Ok(copy)
}

/// A fresh environment: the base functions, private copies of the libraries and `_G` pointing at itself.
/// Run the prelude in it, then [`freeze`] it.
///
/// # Errors
/// Fails when the engine's globals are not the ones [`make_bare_engine`](crate::lua::sandbox::make_bare_engine) sets up.
pub fn new_env(lua: &Lua) -> mlua::Result<Table> {
    let globals = lua.globals();
    let env = lua.create_table()?;
    for name in BASE_FUNCTIONS {
        env.raw_set(*name, globals.raw_get::<Value>(*name)?)?;
    }
    env.raw_set("_VERSION", globals.raw_get::<Value>("_VERSION")?)?;
    for name in LIBRARIES {
        let library: Table = globals.raw_get(*name)?;
        env.raw_set(*name, copy(lua, &library)?)?;
    }
    install_private_random(lua, &env.raw_get::<Table>("math")?)?;
    install_counted_sort(lua, &env.raw_get::<Table>("table")?)?;
    let coroutine: Table = env.raw_get("coroutine")?;
    install_async_coroutines(lua, &coroutine, &globals.raw_get::<Table>("coroutine")?)?;
    env.raw_set("_G", env.clone())?;
    Ok(env)
}

/// Replace `table.sort` in the script's copy of `table` with the Lua implementation of `sort.lua`: the C one is
/// a single VM instruction, so the CPU budget cannot count inside it and a quadratic input outruns every wall.
fn install_counted_sort(lua: &Lua, table: &Table) -> mlua::Result<()> {
    let sort: mlua::Function =
        lua.load(include_str!("sort.lua")).set_name("=sort").set_mode(mlua::chunk::ChunkMode::Text).eval()?;
    table.raw_set("sort", sort)
}

/// `math.random` and `math.randomseed` over a generator of the script's own. The C library's `rand` is one
/// state shared by every VM in the process: one script's `math.randomseed(42)` would make every other script's
/// numbers predictable, and an unseeded state gives the same numbers on every start. Seeded from the operating
/// system, xorshift64*; not for secrets (scripts get no source of those).
fn install_private_random(lua: &Lua, math: &Table) -> mlua::Result<()> {
    let mut seed = [0u8; 8];
    getrandom::fill(&mut seed)
        .map_err(|e| mlua::Error::runtime(format!("operating system random source failed: {e}")))?;
    let state = Arc::new(Mutex::new(splitmix(u64::from_le_bytes(seed))));
    let next = |state: &Mutex<u64>| {
        let mut x = state.lock();
        *x ^= *x >> 12;
        *x ^= *x << 25;
        *x ^= *x >> 27;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let source = state.clone();
    let random = lua.create_function(move |_, (m, n): (Option<f64>, Option<f64>)| {
        let bits = next(&source);
        let (low, high) = match (m, n) {
            (None, _) => return Ok(unit_interval(bits)),
            (Some(upper), None) => (1, random_bound(upper, 1)?),
            (Some(lower), Some(upper)) => (random_bound(lower, 1)?, random_bound(upper, 2)?),
        };
        if low > high {
            let position = if n.is_some() { 2 } else { 1 };
            return Err(mlua::Error::runtime(format!(
                "bad argument #{position} to 'random' (interval is empty)"
            )));
        }
        Ok(draw(bits, low, high))
    })?;
    let reseed = lua.create_function(move |_, seed: f64| {
        *state.lock() = splitmix(seed.to_bits());
        Ok(())
    })?;
    math.raw_set("random", random)?;
    math.raw_set("randomseed", reseed)
}

/// `[0, 1)` from the top 53 bits of a draw.
#[allow(clippy::cast_precision_loss)] // 53 bits are exactly representable
fn unit_interval(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

/// A bound of `math.random` as an integer: truncated toward zero like Lua 5.1's `(int)` cast, but a value
/// that has no `i64` (`NaN`, an infinity, `1e300`) is an error rather than a wrapped or saturated number.
fn random_bound(value: f64, position: usize) -> mlua::Result<i64> {
    // 2^63, exactly: every finite double below it truncates to an i64
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if !value.is_finite() || value.abs() >= LIMIT {
        return Err(mlua::Error::runtime(format!(
            "bad argument #{position} to 'random' (number has no integer representation)"
        )));
    }
    #[allow(clippy::cast_possible_truncation)] // in range, checked above
    Ok(value.trunc() as i64)
}

/// An integer in `[low, high]` (`low <= high`) from a draw: the span is at most 2^64, so it is computed in
/// `u128`, and the offset (below the span) always fits back. Reducing a 64-bit draw modulo the span is biased
/// by less than 2^-32 for any span up to 2^32; this generator is not for secrets.
#[allow(clippy::cast_precision_loss)] // a result past 2^53 is the nearest double, still within [low, high]
fn draw(bits: u64, low: i64, high: i64) -> f64 {
    let span = u128::from(high.abs_diff(low)) + 1;
    let offset = i128::try_from(u128::from(bits) % span).unwrap_or(0);
    i64::try_from(i128::from(low) + offset).unwrap_or(low) as f64
}

/// A well-mixed non-zero xorshift state from any 64 bits.
fn splitmix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) | 1
}

/// Replace `resume` and `wrap` in the script's copy of `coroutine` with versions that let an async host call
/// inside a coroutine of the script's own reach the executor (see `async_coroutines.lua`).
fn install_async_coroutines(lua: &Lua, copy: &Table, raw: &Table) -> mlua::Result<()> {
    let pending = Value::LightUserData(Lua::poll_pending());
    let wrappers: Table = lua
        .load(include_str!("async_coroutines.lua"))
        .set_name("=async_coroutines")
        .set_mode(mlua::chunk::ChunkMode::Text)
        .call((pending, raw.clone(), crate::lua::sandbox::killed_function(lua)?))?;
    copy.raw_set("resume", wrappers.raw_get::<Value>("resume")?)?;
    copy.raw_set("wrap", wrappers.raw_get::<Value>("wrap")?)
}

/// A read-only view of `target`: an empty table whose `__index` is `target`.
fn read_only(lua: &Lua, name: &str, target: Table) -> mlua::Result<Table> {
    let message = format!("attempt to modify the read-only table `{name}`");
    let meta = lua.create_table()?;
    meta.raw_set("__index", target)?;
    meta.raw_set(
        "__newindex",
        lua.create_function(move |_, _: mlua::MultiValue| -> mlua::Result<()> {
            Err(mlua::Error::runtime(message.clone()))
        })?,
    )?;
    // makes `getmetatable` return this string and `setmetatable` raise "cannot change a protected metatable"
    meta.raw_set("__metatable", format!("read-only {name}"))?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(meta))?;
    Ok(proxy)
}

/// Seal `env`: every table in it other than `_G` becomes a read-only proxy and `rawset` refuses them.
/// Call it after the host's own additions (the prelude) and before the script runs.
///
/// # Errors
/// Fails when a proxy or the guarded `rawset` cannot be created.
pub fn freeze(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let guarded = lua.create_table()?; // proxy -> true, so `rawset` can refuse it
    let libraries: Vec<(String, Table)> = env
        .pairs::<String, Value>()
        .filter_map(|pair| match pair {
            Ok((name, Value::Table(table))) if name != "_G" => Some((name, table)),
            _ => None,
        })
        .collect();
    for (name, library) in libraries {
        let proxy = read_only(lua, &name, library)?;
        guarded.raw_set(proxy.clone(), true)?;
        env.raw_set(name, proxy)?;
    }
    let raw_set = lua.create_function(move |_, (table, key, value): (Table, Value, Value)| {
        if guarded.raw_get::<bool>(table.clone()).unwrap_or(false) {
            return Err(mlua::Error::runtime("attempt to modify a read-only table"));
        }
        table.raw_set(key, value)?;
        Ok(table)
    })?;
    env.raw_set("rawset", raw_set)
}

#[cfg(test)]
mod tests {
    use mlua::{chunk::ChunkMode, Lua, Table, Value};

    use super::{freeze, new_env};
    use crate::lua::sandbox::make_bare_engine;

    /// An engine, a frozen environment, and a way to run source in it.
    struct Rig {
        lua: Lua,
        env: Table,
    }

    impl Rig {
        fn new() -> Self {
            let lua = make_bare_engine().expect("engine");
            let env = new_env(&lua).expect("env");
            freeze(&lua, &env).expect("freeze");
            Self { lua, env }
        }

        fn run(&self, source: &str) -> mlua::Result<Value> {
            self.lua.load(source).set_mode(ChunkMode::Text).set_environment(self.env.clone()).eval()
        }

        fn string(&self, source: &str) -> String {
            match self.run(source) {
                Ok(Value::String(s)) => s.to_string_lossy(),
                other => panic!("{source}: {other:?}"),
            }
        }

        fn denied(&self, source: &str) {
            let err = self.run(source).expect_err(source).to_string();
            assert!(
                err.contains("read-only") || err.contains("protected") || err.contains("attempt to"),
                "{source}: {err}"
            );
        }
    }

    /// `table.sort` of the script's environment next to the C one (as `c_sort`).
    fn sort_rig() -> Rig {
        let rig = Rig::new();
        let c_sort: Value = rig.lua.load("return table.sort").eval().unwrap();
        rig.env.raw_set("c_sort", c_sort).unwrap();
        rig
    }

    #[test]
    fn table_sort_sorts_like_the_c_one_for_every_size_and_comparison() {
        let rig = sort_rig();
        let out = rig.string(
            "local seed = 7
             local function rnd(n) seed = (seed * 1103515245 + 12345) % 2147483648 return seed % n + 1 end
             local bad = {}
             local function same(a, b) for i = 1, #a do if a[i] ~= b[i] then return false end end return #a == #b end
             for _, n in ipairs { 0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 64, 100, 257, 1000 } do
               for _, spread in ipairs { 2, 10, 1000000 } do
                 local a, b, c, d = {}, {}, {}, {}
                 for i = 1, n do local v = rnd(spread) a[i], b[i], c[i], d[i] = v, v, v, v end
                 c_sort(a) table.sort(b)
                 if not same(a, b) then bad[#bad + 1] = 'default n=' .. n .. ' spread=' .. spread end
                 local function desc(x, y) return x > y end
                 c_sort(c, desc) table.sort(d, desc)
                 if not same(c, d) then bad[#bad + 1] = 'desc n=' .. n .. ' spread=' .. spread end
               end
             end
             local words = { 'pear', 'apple', 'fig', 'banana', 'cherry', 'apple' }
             table.sort(words)
             if table.concat(words, ',') ~= 'apple,apple,banana,cherry,fig,pear' then bad[#bad + 1] = 'strings' end
             return table.concat(bad, ';')",
        );
        assert_eq!(out, "");
    }

    #[test]
    fn table_sort_is_stable_and_never_hangs_on_an_inconsistent_comparison() {
        let rig = sort_rig();
        let out = rig.string(
            "local t = {}
             for i = 1, 200 do t[i] = { key = i % 5, order = i } end
             table.sort(t, function(a, b) return a.key < b.key end)
             for i = 2, #t do
               if t[i - 1].key == t[i].key and t[i - 1].order > t[i].order then return 'unstable at ' .. i end
             end
             -- a comparison that is not an order at all: some order of the same elements, no error, no hang
             local u = {}
             for i = 1, 300 do u[i] = i end
             table.sort(u, function() return true end)
             table.sort(u, function() return false end)
             table.sort(u, function(a, b) return (a + b) % 3 == 0 end)
             local sum = 0
             for i = 1, #u do sum = sum + u[i] end
             return #u == 300 and sum == 300 * 301 / 2 and 'ok' or 'lost elements'",
        );
        assert_eq!(out, "ok");
    }

    #[test]
    fn table_sort_reports_the_same_errors_as_the_c_one() {
        let rig = sort_rig();
        for call in [
            "table.sort({ 3, 'x', 1 })",
            "table.sort({ 3, {}, 1 })",
            "table.sort({ 1, 2, nil, 4, 3 })",
            "table.sort(nil)",
            "table.sort('abc')",
            "table.sort({ 2, 1 }, 'nope')",
            "table.sort({ 2, 1 }, function() error('boom', 0) end)",
        ] {
            let both = rig.string(&format!(
                "local a, ea = pcall(function() {call} end)
                 local b, eb = pcall(function() {} end)
                 return tostring(a) .. '/' .. tostring(b)",
                call.replace("table.sort", "c_sort")
            ));
            assert_eq!(both, "false/false", "{call}");
        }
        let message = rig.string("local ok, e = pcall(table.sort, { 3, 'x', 1 }) return tostring(e)");
        assert!(message.contains("attempt to compare"), "{message}");
        let message = rig.string("local ok, e = pcall(table.sort, { 2, 1 }, 'nope') return tostring(e)");
        assert!(message.contains("bad argument #2 to 'sort'"), "{message}");
        // sorting leaves a table it could not sort as it was
        assert_eq!(
            rig.string("local t = { 3, 'x', 1 } pcall(table.sort, t) return tostring(t[1]) .. t[2] .. t[3]"),
            "3x1"
        );
    }

    #[test]
    fn table_sort_is_counted_by_the_count_hook_and_n_log_n_whatever_the_input() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let rig = sort_rig();
        let ticks = std::sync::Arc::new(AtomicU64::new(0));
        let counter = ticks.clone();
        rig.lua
            .set_global_hook(mlua::HookTriggers::new().every_nth_instruction(100), move |_, _| {
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(mlua::VmState::Continue)
            })
            .unwrap();
        let cost = |setup: &str| -> u64 {
            ticks.store(0, Ordering::Relaxed);
            rig.run(&format!("local t = {{}} for i = 1, 20000 do t[i] = {setup} end table.sort(t)")).unwrap();
            ticks.load(Ordering::Relaxed)
        };
        let before_c = {
            ticks.store(0, Ordering::Relaxed);
            rig.run("local t = {} for i = 1, 20000 do t[i] = (i * 7919) % 20011 end c_sort(t)").unwrap();
            ticks.load(Ordering::Relaxed)
        };
        let random = cost("(i * 7919) % 20011");
        let sorted = cost("i");
        let reversed = cost("20001 - i");
        let equal = cost("5");
        assert!(
            random > before_c * 20,
            "the C sort costs {before_c} ticks, this one {random}: it is counted"
        );
        // 20000 * log2(20000) ~ 286k comparisons, a few instructions each: tens of thousands of ticks, not millions
        for (name, ticks) in
            [("random", random), ("sorted", sorted), ("reversed", reversed), ("equal", equal)]
        {
            assert!(ticks < 60_000, "{name}: {ticks} ticks");
        }
    }

    #[test]
    fn a_table_sort_comparison_may_yield() {
        let rig = Rig::new();
        let out = rig.string(
            "local co = coroutine.create(function()
               local t = { 3, 1, 2 }
               table.sort(t, function(a, b) coroutine.yield('asking') return a < b end)
               return table.concat(t, ',')
             end)
             local asked = 0
             local ok, v = coroutine.resume(co)
             while coroutine.status(co) == 'suspended' do asked = asked + 1 ok, v = coroutine.resume(co) end
             return tostring(ok) .. ':' .. tostring(v) .. ':' .. (asked > 0 and 'yielded' or 'never')",
        );
        assert_eq!(out, "true:1,2,3:yielded");
    }

    /// A rig whose script can call `stop()`: it marks the script as stopped for good and raises, as the CPU
    /// budget's count hook does. Returns the flag as well.
    fn stoppable() -> (Rig, std::sync::Arc<std::sync::atomic::AtomicBool>) {
        use std::sync::atomic::{AtomicBool, Ordering};
        let rig = Rig::new();
        let stopped = std::sync::Arc::new(AtomicBool::new(false));
        let check = stopped.clone();
        crate::lua::sandbox::set_watchdog(
            &rig.lua,
            crate::lua::sandbox::Watchdog {
                stopped: std::sync::Arc::new(move || check.load(Ordering::SeqCst)),
                orphaned: std::sync::Arc::new(|| false),
            },
        );
        let flag = stopped.clone();
        let stop = rig
            .lua
            .create_function(move |_, ()| -> mlua::Result<()> {
                flag.store(true, Ordering::SeqCst);
                Err(mlua::Error::runtime("cpu budget exceeded"))
            })
            .unwrap();
        rig.env.raw_set("stop", stop).unwrap();
        (rig, stopped)
    }

    #[test]
    fn a_stop_is_raised_again_by_pcall_and_xpcall_whatever_the_handler_does() {
        for source in [
            "pcall(stop) return 'swallowed'",
            "pcall(function() stop() end) return 'swallowed'",
            "xpcall(stop, function() return 'handled' end) return 'swallowed'",
            "xpcall(stop, stop) return 'swallowed'",
            "xpcall(stop, function() error('again') end) return 'swallowed'",
            "pcall(function() pcall(function() pcall(stop) end) end) return 'swallowed'",
            "pcall(function() xpcall(function() pcall(stop) return 1 end, function() return 2 end) end) return 'swallowed'",
            "local co = coroutine.wrap(function() pcall(stop) return 'swallowed' end) return co()",
            "local ok = coroutine.resume(coroutine.create(function() stop() end)) return 'swallowed'",
            "local ok = coroutine.resume(coroutine.create(function() pcall(stop) end)) return 'swallowed'",
        ] {
            let (rig, stopped) = stoppable();
            let result = rig.run(source);
            let err = result.expect_err(source).to_string();
            assert!(err.contains("cpu budget exceeded"), "{source}: {err}");
            assert!(stopped.load(std::sync::atomic::Ordering::SeqCst));
        }
    }

    #[test]
    fn without_a_stop_pcall_xpcall_and_resume_still_catch_ordinary_errors() {
        let (rig, _) = stoppable();
        let out = rig.string(
            "local a = pcall(error, 'x')
             local b, c = xpcall(function() error('y', 0) end, function(e) return 'h:' .. e end)
             local d, e = coroutine.resume(coroutine.create(function() error('z', 0) end))
             local f, g = pcall(function() local ok = pcall(error, 'inner') return ok end)
             return tostring(a) .. tostring(b) .. c .. tostring(d) .. e .. tostring(f) .. tostring(g)",
        );
        assert_eq!(out, "falsefalseh:yfalseztruefalse");
    }

    #[test]
    fn math_random_rejects_a_bound_with_no_integer_instead_of_panicking() {
        let rig = Rig::new();
        for call in [
            "math.random(1e300)",
            "math.random(-1e300, 1e300)",
            "math.random(1, 1e300)",
            "math.random(-1e300, 1)",
            "math.random(1/0)",
            "math.random(-1/0)",
            "math.random(1, 1/0)",
            "math.random(-1/0, 1)",
            "math.random(0/0)",
            "math.random(1, 0/0)",
            "math.random(0/0, 1)",
            "math.random(2^63)",
            "math.random(-2^63, 0)",
        ] {
            let message = rig.string(&format!(
                "local ok, e = pcall(function() return {call} end) return tostring(ok) .. tostring(e)"
            ));
            assert!(
                message.starts_with("false")
                    && message.contains("bad argument")
                    && message.contains("'random'"),
                "{call}: {message}"
            );
        }
    }

    #[test]
    fn math_random_rejects_an_empty_interval_like_lua_5_1() {
        let rig = Rig::new();
        for call in [
            "math.random(0)",
            "math.random(-5)",
            "math.random(5, 1)",
            "math.random(-1, -2)",
            "math.random(0.5)",
        ] {
            let message = rig.string(&format!(
                "local ok, e = pcall(function() return {call} end) return tostring(ok) .. tostring(e)"
            ));
            assert!(
                message.starts_with("false") && message.contains("interval is empty"),
                "{call}: {message}"
            );
        }
    }

    #[test]
    fn math_random_stays_inside_every_range_including_the_widest() {
        let rig = Rig::new();
        let out = rig.string(
            "local bad = {}
             local function check(name, lo, hi, f)
               for _ = 1, 200 do
                 local v = f()
                 if v < lo or v > hi or v ~= math.floor(v) then bad[#bad + 1] = name .. '=' .. tostring(v) break end
               end
             end
             check('one', 1, 1, function() return math.random(1) end)
             check('upper', 1, 10, function() return math.random(10) end)
             check('neg', -5, -3, function() return math.random(-5, -3) end)
             check('mixed', -2, 2, function() return math.random(-2, 2) end)
             check('point', 7, 7, function() return math.random(7, 7) end)
             check('trunc', 3, 4, function() return math.random(3.9, 4.9) end)
             check('wide', -2^62, 2^62, function() return math.random(-2^62, 2^62) end)
             check('widest', -2^63 + 1024, 2^63 - 1024, function() return math.random(-2^63 + 1024, 2^63 - 1024) end)
             local u = math.random()
             if u < 0 or u >= 1 then bad[#bad + 1] = 'unit' end
             return table.concat(bad, ',')",
        );
        assert_eq!(out, "");
    }

    #[test]
    fn draw_covers_the_full_range_without_overflow() {
        use super::draw;
        for bits in [0, 1, u64::MAX, u64::MAX / 2, 1 << 63] {
            let v = draw(bits, i64::MIN, i64::MAX);
            assert!((-9.3e18..=9.3e18).contains(&v), "{bits}: {v}");
            assert!(draw(bits, -3, 3).abs() <= 3.0);
            assert!((draw(bits, 5, 5) - 5.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn the_environment_holds_exactly_the_allow_list() {
        let rig = Rig::new();
        let mut names: Vec<String> = rig.env.pairs::<String, Value>().map(|p| p.unwrap().0).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "_G",
                "_VERSION",
                "assert",
                "bit",
                "coroutine",
                "error",
                "getmetatable",
                "ipairs",
                "math",
                "next",
                "pairs",
                "pcall",
                "rawequal",
                "rawget",
                "rawset",
                "select",
                "setmetatable",
                "string",
                "table",
                "tonumber",
                "tostring",
                "type",
                "unpack",
                "xpcall"
            ]
        );
    }

    #[test]
    fn the_escape_hatches_are_not_there() {
        let rig = Rig::new();
        let out = rig
            .string(
                "local out = {}
                 for _, n in ipairs { 'load', 'loadstring', 'dofile', 'loadfile', 'getfenv', 'setfenv', 'newproxy',
                   'module', 'collectgarbage', 'gcinfo', 'require', 'print', 'io', 'os', 'debug', 'package' } do
                   if _G[n] ~= nil then out[#out + 1] = n end
                 end
                 if string.dump or string.gfind then out[#out + 1] = 'string.dump/gfind' end
                 return table.concat(out, ',')",
            );
        assert_eq!(out, "");
    }

    #[test]
    fn writing_to_a_library_fails_and_the_library_is_unchanged() {
        let rig = Rig::new();
        for library in ["string", "table", "math", "coroutine", "bit"] {
            rig.denied(&format!("{library}.evil = 1"));
            rig.denied(&format!("{library}.len = nil"));
            rig.denied(&format!("{library}[1] = 1"));
        }
        assert_eq!(rig.string("return ('abc'):len() .. string.len('abcd')"), "34");
        // nothing was added to the engine's own tables either
        let real: Value = rig.lua.load("return string.evil").eval().unwrap();
        assert!(matches!(real, Value::Nil));
    }

    #[test]
    fn rawset_refuses_a_library_and_still_works_on_a_plain_table() {
        let rig = Rig::new();
        rig.denied("rawset(string, 'evil', 1)");
        rig.denied("rawset(table, 'insert', nil)");
        assert_eq!(rig.string("local t = {} rawset(t, 'k', 'v') return t.k"), "v");
        assert_eq!(rig.string("return type(rawset({}, 1, 2))"), "table");
    }

    #[test]
    fn a_library_s_metatable_is_locked() {
        let rig = Rig::new();
        assert_eq!(rig.string("return tostring(getmetatable(string))"), "read-only string");
        rig.denied("setmetatable(string, {})");
        rig.denied("setmetatable(table, { __index = function() return 1 end })");
        // and the proxy cannot be unwrapped to the table behind it
        assert_eq!(rig.string("return type(getmetatable(math))"), "string");
    }

    #[test]
    fn the_shared_string_metatable_cannot_be_used_to_mutate_string() {
        let rig = Rig::new();
        assert_eq!(rig.string("return tostring(getmetatable(''))"), "locked");
        rig.denied("getmetatable('').__index.evil = 1");
        rig.denied("getmetatable('').__index = {}");
        rig.denied("string.rep = nil");
        // methods on strings still work
        assert_eq!(rig.string("return ('x'):rep(3) .. ('Ab'):lower()"), "xxxab");
        let real: Value = rig.lua.load("return string.evil").eval().unwrap();
        assert!(matches!(real, Value::Nil));
    }

    #[test]
    fn globals_a_script_sets_land_in_its_own_environment_only() {
        let rig = Rig::new();
        rig.run("counter = 1").unwrap();
        assert_eq!(rig.string("return tostring(counter)"), "1");
        let other = Rig::new();
        assert_eq!(other.string("return tostring(counter)"), "nil");
        let engine_global: Value = rig.lua.globals().get("counter").unwrap();
        assert!(matches!(engine_global, Value::Nil));
        // `_G` is the environment, not the engine's globals
        assert_eq!(
            rig.string("return tostring(_G.counter) .. tostring(_G._G == _G) .. tostring(_G.load)"),
            "1truenil"
        );
    }

    #[test]
    fn bytecode_is_not_loadable_by_the_script_or_by_a_text_only_load() {
        let rig = Rig::new();
        assert_eq!(rig.string("return type(load) .. type(loadstring) .. type(dofile)"), "nilnilnil");
        let raw = crate::lua::sandbox::bytecode();
        let err = rig
            .lua
            .load(raw)
            .set_mode(ChunkMode::Text)
            .set_environment(rig.env.clone())
            .exec()
            .expect_err("bytecode")
            .to_string();
        assert!(err.contains("binary"), "{err}");
    }

    #[test]
    fn math_random_is_private_to_each_environment() {
        let a = Rig::new();
        let b = Rig::new();
        // the same seed gives the same run, and seeding another script's generator changes nothing here
        a.run("math.randomseed(42)").unwrap();
        let first = a.string("return tostring(math.random(1, 1000000))");
        a.run("math.randomseed(42)").unwrap();
        b.run("math.randomseed(7)").unwrap();
        assert_eq!(a.string("return tostring(math.random(1, 1000000))"), first);
        // unseeded environments differ (a shared or fixed seed would repeat)
        let draw = || Rig::new().string("return tostring(math.random(1, 1e15))");
        assert_ne!(draw(), draw());
    }

    #[test]
    fn math_random_has_the_lua_5_1_shape() {
        let rig = Rig::new();
        let out = rig.string(
            "local lo, hi = 1, 0
             for _ = 1, 2000 do
               local f = math.random() if f < 0 or f >= 1 then return 'float ' .. f end
               local a = math.random(6) if a < 1 or a > 6 or a ~= math.floor(a) then return 'one ' .. a end
               local b = math.random(-3, 3) if b < -3 or b > 3 or b ~= math.floor(b) then return 'two ' .. b end
               if b == -3 then lo = 0 elseif b == 3 then hi = 1 end
             end
             local ok1, e1 = pcall(math.random, 0)
             local ok2, e2 = pcall(math.random, 5, 1)
             return (lo == 0 and hi == 1 and 'edges' or 'no edges') .. tostring(ok1) .. tostring(ok2) .. tostring(e1) .. tostring(e2)",
        );
        assert!(out.starts_with("edgesfalsefalse") && out.contains("interval is empty"), "{out}");
    }

    #[test]
    fn a_library_can_be_extended_before_it_is_frozen() {
        let lua = make_bare_engine().unwrap();
        let env = new_env(&lua).unwrap();
        lua.load("local env = ... env.table.clone = function(t) return t end env.utf8 = { len = function() return 0 end }")
            .call::<()>(env.clone())
            .unwrap();
        freeze(&lua, &env).unwrap();
        let rig = Rig { lua, env };
        assert_eq!(rig.string("return tostring(table.clone ~= nil) .. tostring(utf8.len(''))"), "true0");
        rig.denied("utf8.len = nil");
        rig.denied("table.clone = nil");
    }
}
