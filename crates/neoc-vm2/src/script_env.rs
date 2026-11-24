//! The environment an untrusted script runs in.
//!
//! Luau keeps Lua 5.1's function environments: a chunk loaded with `Chunk::set_environment` and every closure
//! created inside it see only that table as their globals (the engine's `setfenv` and `getfenv` are stripped, so
//! the script cannot change or read it). [`new_env`] builds it, [`freeze`] seals it. An embedder runs a script
//! in it so that scripts do not share state by accident; it is not a security boundary (a script still has every
//! module the embedder gives it):
//!
//! - the **base functions** a script may call are copied in one by one (an allow-list: a function a Luau
//!   upgrade adds is not available until someone adds it here): no `load*`, `dofile`, `getfenv`,
//!   `setfenv`, `newproxy`, `collectgarbage`, `require`, `print` (it writes to the process's stdout) and no `os`/`debug`/`buffer`/`vector`/`utf8`.
//!   `typeof` is there: it names a module's userdata type as the declarations do (`typeof(db) == "vos_app_Db"`,
//!   where `type` says `userdata`), which is what narrows a Luau extern type;
//! - the **libraries** (`string`, `table`, `math`, `coroutine`, `bit32`) are private copies (`coroutine.create`,
//!   `resume`, `wrap` and `close` are versions that let an async host call run inside a coroutine of the script's own
//!   and let a script resume or close only the coroutines it made, see `async_coroutines.lua`; `neoc run`'s engine has
//!   no `coroutine.close` at all), so the host may add
//!   its own functions to them before [`freeze`], and the script can never reach the
//!   engine's own shared tables;
//! - [`freeze`] then marks every library in the environment read-only with Luau's own `readonly` table flag:
//!   assigning to it, `rawset`, `table.insert`, `table.sort`, `table.move`, `table.clear` and `setmetatable` on it
//!   all fail with "attempt to modify a readonly table" (`table.freeze` says "table is already frozen"; `table.remove` too, except that on a table
//!   with no array part, which a library is, it removes nothing and raises nothing). LuaJIT had no such flag: the
//!   libraries were empty proxies behind a guarded `rawset` and guarded `table` writers, which Luau makes redundant,
//!   and `pairs(string)` saw nothing; it sees the functions now. The flag is set on the table itself, except for a
//!   host table that has a metatable, which is replaced by a read-only proxy (see [`freeze`]).
//!
//! The string metatable is a different matter: it is shared by every string in the engine, and
//! [`make_bare_engine`](crate::engine::make_bare_engine) locks it.
//!
//! Ported from the old VM's `script_env.rs`, unchanged.

use std::sync::Arc;

use mlua::{Lua, Table, Value};
use parking_lot::Mutex;

use crate::engine::load_source;

/// Base functions a script gets, copied from the engine's globals.
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
    "rawlen",
    "rawset",
    "select",
    "setmetatable",
    "tonumber",
    "tostring",
    "type",
    "typeof",
    "unpack",
    "xpcall",
];

/// Libraries a script gets a private copy of.
const LIBRARIES: &[&str] = &["string", "table", "math", "coroutine", "bit32"];

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
/// Add the host's own functions to it if there are any, then [`freeze`] it.
///
/// # Errors
/// Fails when the engine's globals are not the ones [`make_bare_engine`](crate::engine::make_bare_engine) sets up.
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
    let coroutine: Table = env.raw_get("coroutine")?;
    install_async_coroutines(lua, &coroutine, &globals.raw_get::<Table>("coroutine")?)?;
    env.raw_set("_G", env.clone())?;
    Ok(env)
}

/// `math.random` and `math.randomseed` over a generator of the script's own, seeded from the operating system
/// (xorshift64*; not for secrets, scripts get no source of those). Luau's own generator is seeded at start (LuaJIT's was
/// not: the first `math.random()` of a fresh engine was the same number on every run), but a script environment keeps
/// its own because `math.random(1e300)` is not an error on Luau (it gives an integer of no particular meaning) and
/// because one script must not be able to reseed another's draws. This one keeps Lua 5.1's shape (an empty interval is
/// an error) and refuses a bound that has no integer representation.
fn install_private_random(lua: &Lua, math: &Table) -> mlua::Result<()> {
    let mut seed = [0u8; 8];
    crate::modules::lib::random::fill_random(&mut seed).map_err(mlua::Error::runtime)?;
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

/// A bound of `math.random` as an integer: truncated toward zero like the `(int)` cast of Lua 5.1's `math.random`, but a value
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

/// Replace `create`, `resume`, `wrap` and `close` in the script's copy of `coroutine` with versions that let an async host call
/// inside a coroutine of the script's own reach the executor, and that refuse to resume or close a coroutine the script did not
/// make (see `async_coroutines.lua`).
fn install_async_coroutines(lua: &Lua, copy: &Table, raw: &Table) -> mlua::Result<()> {
    let pending = Value::LightUserData(Lua::poll_pending());
    // the coroutines the script made, and those whose driver is somewhere else (which the task scheduler shares:
    // [`mark_driven`]). Both are keyed weakly, so a coroutine is not kept alive by being listed; `driven`
    // is also valued weakly, so a driver thread nobody refers to any more (its call was dropped) clears the entry
    // instead of refusing the coroutine for ever (see `async_coroutines.lua`)
    let owned = weak_table(lua, "k")?;
    let driven = weak_table(lua, "kv")?;
    lua.set_named_registry_value(OWNED, owned.clone())?;
    lua.set_named_registry_value(DRIVEN, driven.clone())?;
    let close: Value = lua.named_registry_value(crate::engine::STASHED_CLOSE)?;
    let wrappers: Table = load_source(lua, include_str!("async_coroutines.lua"))
        .set_name("=async_coroutines")
        .call((pending, raw.clone(), driven, owned, close))?;
    for name in ["create", "resume", "wrap", "close"] {
        copy.raw_set(name, wrappers.raw_get::<Value>(name)?)?;
    }
    Ok(())
}

/// An empty table with weak `mode` (`"k"`, or `"kv"`): being listed does not keep a coroutine (or a thread) alive.
fn weak_table(lua: &Lua, mode: &str) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.raw_set("__mode", mode)?;
    table.set_metatable(Some(meta))?;
    Ok(table)
}

/// Where the Lua registry keeps the table of coroutines the script made (`async_coroutines.lua`): the only ones it may
/// resume.
const OWNED: &str = "neoc.owned_coroutines";

/// Where the Lua registry keeps the table of coroutines whose driver is somewhere else (`async_coroutines.lua`).
const DRIVEN: &str = "neoc.driven_coroutines";

/// A table of the script environment kept in the registry (the ones [`new_env`] installs); `None` outside it (a worker, a test), which has no such tables.
fn environment_table(lua: &Lua, name: &str) -> mlua::Result<Option<mlua::Table>> {
    match lua.named_registry_value::<mlua::Value>(name)? {
        mlua::Value::Nil => Ok(None),
        mlua::Value::Table(table) => Ok(Some(table)),
        other => Err(mlua::Error::runtime(format!(
            "the registry entry `{name}` is a {}, not a table",
            other.type_name()
        ))),
    }
}

/// A coroutine the script made and handed to the task scheduler (`task.spawn`, `task.defer`, `task.delay`): the
/// scheduler drives it from here on, and the script may no longer resume it itself, as with a coroutine that is
/// running; two drivers of one coroutine would fight over the host call it is waiting at. Without a script environment
/// (the registry tables it keeps are missing: a worker, a test, `neoc run`) it does nothing.
///
/// # Errors
/// The coroutine is not one the script made (a thread it reached through `coroutine.running()`: the host's own),
/// or it is already being driven (a thread is waiting at its host call, or the scheduler has it).
pub fn mark_driven(lua: &Lua, thread: &mlua::Thread) -> mlua::Result<()> {
    let (Some(owned), Some(driven)) = (environment_table(lua, OWNED)?, environment_table(lua, DRIVEN)?)
    else {
        return Ok(());
    };
    if !owned.raw_get::<bool>(thread.clone()).unwrap_or(false) {
        return Err(mlua::Error::runtime(
            "cannot schedule a coroutine that is not yours: only one made by coroutine.create or coroutine.wrap",
        ));
    }
    let live = match driven.raw_get::<mlua::Value>(thread.clone())? {
        mlua::Value::Boolean(true) => true,
        mlua::Value::Thread(driver) => driver.status() == mlua::thread::ThreadStatus::Resumable,
        _ => false,
    };
    if live {
        return Err(mlua::Error::runtime("cannot schedule a coroutine that is already running"));
    }
    driven.raw_set(thread.clone(), true)
}

/// Seal `env`: every table in it other than `_G` becomes read-only. Call it after the host's own additions and
/// before the script runs.
///
/// A table with no metatable (the libraries) is flagged `readonly` itself. A table **with** a metatable (an object the
/// host added, with `__index` methods or a `__newindex`) is replaced in the environment by a read-only proxy over it:
/// the flag alone would leave the script `getmetatable(host).__index = ...` (a write the host's own table and every
/// other environment holding it would see) and a `__newindex` function that takes writes to absent keys past the flag.
/// The proxy is empty, reads through to the table, refuses every write, and locks its metatable (`getmetatable`
/// answers a string), as every library was behind a proxy on LuaJIT.
///
/// **The proxy forwards reads only** (`__index`). What does not pass through it: `__call` (calling the object says
/// "attempt to call a table value"), `__len`, `__tostring`, `__eq`, `__concat`, `__iter` and the arithmetic and
/// comparison metamethods; `pairs` and `next` (the proxy is empty, so they see nothing); and any write a method makes
/// to `self`, because `host:m()` passes the proxy as `self` and the proxy refuses writes (a method that keeps its state
/// in an upvalue, or in a table it holds, still works). A host object that needs one of these goes into the
/// environment as a plain table without a metatable, or behind functions that carry the state themselves.
/// (A plain table is flagged *itself*, not wrapped:
/// an embedder that shares one between environments, or changes it later, gets a read-only table everywhere, and its
/// own `set` fails on it.)
///
/// # Errors
/// A proxy could not be created.
pub fn freeze(lua: &Lua, env: &Table) -> mlua::Result<()> {
    // a key that is not a string or a number is skipped; a number key is its text here (`pairs::<String, _>`)
    let tables: Vec<(String, Table)> = env
        .pairs::<String, Value>()
        .flatten()
        .filter_map(|(name, value)| match value {
            Value::Table(table) if name != "_G" => Some((name, table)),
            _ => None,
        })
        .collect();
    for (name, table) in tables {
        if table.metatable().is_some() {
            env.raw_set(name.as_str(), read_only(lua, &name, table)?)?;
        } else {
            table.set_readonly(true);
        }
    }
    Ok(())
}

/// A read-only view of `target`: an empty read-only table whose `__index` is `target`, whose `__newindex` raises, and
/// whose metatable is locked.
fn read_only(lua: &Lua, name: &str, target: Table) -> mlua::Result<Table> {
    let message = format!("attempt to modify the readonly table `{name}`");
    let meta = lua.create_table()?;
    meta.raw_set("__index", target)?;
    meta.raw_set(
        "__newindex",
        lua.create_function(move |_, _: mlua::MultiValue| -> mlua::Result<()> {
            Err(mlua::Error::runtime(message.clone()))
        })?,
    )?;
    meta.raw_set("__metatable", format!("readonly {name}"))?;
    meta.set_readonly(true);
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(meta))?;
    proxy.set_readonly(true);
    Ok(proxy)
}

#[cfg(test)]
mod tests {
    use mlua::{Function, Lua, Table, Value, chunk::ChunkMode};

    use super::{freeze, new_env};
    use crate::engine::make_bare_engine;

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
            assert!(err.to_lowercase().contains("readonly"), "{source}: {err}");
        }
    }

    #[test]
    fn pcall_xpcall_and_resume_catch_errors_and_nest() {
        let rig = Rig::new();
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
    fn math_random_rejects_an_empty_interval() {
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
                "bit32",
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
                "rawlen",
                "rawset",
                "select",
                "setmetatable",
                "string",
                "table",
                "tonumber",
                "tostring",
                "type",
                "typeof",
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
        for library in ["string", "table", "math", "coroutine", "bit32"] {
            rig.denied(&format!("{library}.evil = 1"));
            rig.denied(&format!("{library}.len = nil"));
            rig.denied(&format!("{library}[1] = 1"));
        }
        assert_eq!(rig.string("return ('abc'):len() .. string.len('abcd')"), "34");
        // nothing was added to the engine's own tables either
        let real: Value = rig.lua.load("return string.evil").eval().unwrap();
        assert!(matches!(real, Value::Nil));
    }

    /// `table.insert`, `table.sort` and `table.move` check Luau's `readonly` flag themselves (LuaJIT's stored with raw
    /// sets, past a proxy): each is refused on a frozen library, in each spelling (`table.remove` is the exception,
    /// asserted below: it removes nothing from a table with no array part and raises nothing), and works on the
    /// script's own tables.
    #[test]
    fn the_table_functions_refuse_a_frozen_library_and_work_on_a_plain_table() {
        let rig = Rig::new();
        for library in ["string", "table", "math", "coroutine", "bit32"] {
            rig.denied(&format!("table.insert({library}, 'x')"));
            rig.denied(&format!("table.insert({library}, 1, 'x')"));
            // Luau's `table.remove` of a table with no array part (a library has none) removes nothing and
            // raises nothing, frozen or not: the library is unchanged, which `denied` cannot tell
            assert_eq!(
                rig.string(&format!(
                    "local before = 0 for _ in pairs({library}) do before = before + 1 end \
                     local removed = table.remove({library}) local again = table.remove({library}, 1) \
                     local after = 0 for _ in pairs({library}) do after = after + 1 end \
                     return tostring(removed) .. tostring(again) .. (before == after and 'same' or 'changed')"
                )),
                "nilnilsame"
            );
            rig.denied(&format!("table.sort({library})"));
            rig.denied(&format!("table.sort({library}, function(a, b) return a < b end)"));
            rig.denied(&format!("setmetatable({library}, nil)"));
            // into a frozen library as the target, and out of it (a read, which is allowed)
            rig.denied(&format!("table.move({{ 'a', 'b' }}, 1, 2, 1, {library})"));
            rig.denied(&format!("table.move({library}, 1, 2, 3)"));
        }
        assert_eq!(
            rig.string(
                "local to = table.move({ 'a', 'b' }, 1, 2, 2, { 'x' }) return table.concat(to, ',') .. '|' \
                 .. #table.move(string, 1, 2, 1, {})"
            ),
            "x,a,b|0"
        );
        assert_eq!(
            rig.string(
                "local t = { 2, 1 } table.insert(t, 3) table.sort(t) local x = table.remove(t, 1) \
                 return table.concat(t, ',') .. '|' .. x"
            ),
            "2,3|1"
        );
        // the refusal names the table, and a non-table argument is the engine's own error
        assert!(rig.string("return select(2, pcall(table.insert, string, 'x'))").contains("readonly table"));
        assert!(
            rig.string("return select(2, pcall(table.insert, nil, 'x'))")
                .contains("invalid argument #1 to 'insert'")
        );
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
    fn a_library_s_metatable_cannot_be_set() {
        let rig = Rig::new();
        // a library is a read-only table with no metatable (LuaJIT's was a proxy that answered a string)
        assert_eq!(rig.string("return tostring(getmetatable(string))"), "nil");
        rig.denied("setmetatable(string, {})");
        rig.denied("setmetatable(table, { __index = function() return 1 end })");
        assert_eq!(rig.string("return type(getmetatable(math))"), "nil");
    }

    #[test]
    fn the_shared_string_metatable_cannot_be_used_to_mutate_string() {
        let rig = Rig::new();
        assert_eq!(rig.string("return tostring(getmetatable(''))"), "locked");
        // the lock makes `getmetatable('')` a string, so there is no table to write to: an index error, not `readonly`
        for source in ["getmetatable('').__index.evil = 1", "getmetatable('').__index = {}"] {
            let err = rig.run(source).expect_err(source).to_string();
            assert!(err.contains("attempt to index"), "{source}: {err}");
        }
        rig.denied("string.rep = nil");
        // methods on strings still work
        assert_eq!(rig.string("return ('x'):rep(3) .. ('Ab'):lower()"), "xxxab");
        let real: Value = rig.lua.load("return string.evil").eval().unwrap();
        assert!(matches!(real, Value::Nil));
    }

    /// An object the host adds before `freeze` (a table with `__index` methods and a `__newindex`) cannot be changed by
    /// the script in either of the spellings the readonly flag alone left open.
    #[test]
    fn a_host_table_with_a_metatable_cannot_be_rewritten_by_the_script() {
        let lua = make_bare_engine().expect("engine");
        let env = new_env(&lua).expect("env");
        let host: Table = lua
            .load(
                "local writes = {} return setmetatable({}, {
                   __index = { greet = function() return 'hi' end },
                   __newindex = function(t, k, v) rawset(writes, k, v) end,
                 })",
            )
            .eval()
            .unwrap();
        env.set("host", host.clone()).unwrap();
        freeze(&lua, &env).expect("freeze");
        let run = |source: &str| {
            lua.load(source).set_mode(ChunkMode::Text).set_environment(env.clone()).eval::<Value>()
        };
        // the shared metatable: `getmetatable` answers a string, so there is nothing to write `__index` into
        for source in [
            "getmetatable(host).__index = { greet = function() return 'pwned' end }",
            "getmetatable(host).__index.greet = function() return 'pwned' end",
            "setmetatable(host, {})",
        ] {
            assert!(run(source).is_err(), "{source} must fail");
        }
        // a write to an absent key does not reach the host's `__newindex`
        let err = run("host.injected = 'yes'").unwrap_err().to_string();
        assert!(err.contains("readonly"), "{err}");
        assert!(run("rawset(host, 'injected', 'yes')").is_err());
        let read = |source: &str| match run(source).unwrap() {
            Value::String(s) => s.to_string_lossy(),
            other => format!("{other:?}"),
        };
        assert_eq!(read("return host.greet() .. tostring(host.injected)"), "hinil");
        // and the host's own table, which every other environment would hold, is untouched
        let greet: Function = host.get("greet").unwrap();
        assert_eq!(greet.call::<String>(()).unwrap(), "hi");
        assert!(host.metatable().is_some());
        assert!(host.raw_get::<Value>("injected").unwrap().is_nil());
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
        let raw = crate::engine::luau_bytecode();
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
    fn math_random_has_the_classic_lua_shape() {
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
