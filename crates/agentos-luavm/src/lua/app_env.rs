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
//!   `async_coroutines.lua`), so the host may add
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
    let coroutine: Table = env.raw_get("coroutine")?;
    install_async_coroutines(lua, &coroutine, &globals.raw_get::<Table>("coroutine")?)?;
    env.raw_set("_G", env.clone())?;
    Ok(env)
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
            (None, _) => return Ok((bits >> 11) as f64 / (1u64 << 53) as f64), // [0, 1)
            (Some(upper), None) => (1.0, upper.trunc()),
            (Some(lower), Some(upper)) => (lower.trunc(), upper.trunc()),
        };
        if low > high {
            let position = if n.is_some() { 2 } else { 1 };
            return Err(mlua::Error::runtime(format!(
                "bad argument #{position} to 'random' (interval is empty)"
            )));
        }
        // the span of [low, high] as an exact integer, and a draw reduced into it (the bias is below 2^-32)
        let span = (high - low) as u128 + 1;
        Ok(low + (u128::from(bits) % span) as f64)
    })?;
    let reseed = lua.create_function(move |_, seed: f64| {
        *state.lock() = splitmix(seed.to_bits());
        Ok(())
    })?;
    math.raw_set("random", random)?;
    math.raw_set("randomseed", reseed)
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
        .call((pending, raw.clone()))?;
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
