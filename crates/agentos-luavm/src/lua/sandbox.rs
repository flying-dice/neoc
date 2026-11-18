//! Engine factory + (value, err) helpers shared by every lib module.

use std::sync::Arc;

use mlua::{
    chunk::{AsChunk, Chunk, ChunkMode},
    Function, IntoLua, Lua, StdLib, Value,
};

use crate::lua::{bit, modules};

/// Globals removed after the standard libraries load.
///
/// Each is a sandbox hazard: runtime code loading (`load`, `loadstring`,
/// `dofile`, `loadfile`, `string.dump`), environment/identity tampering
/// (`getfenv`, `setfenv`, `newproxy`, `module`), and GC control and
/// introspection (`collectgarbage`, `gcinfo`). Paths are dotted; the last
/// segment is the key removed from its parent table.
const STRIPPED_GLOBALS: &[&str] = &[
    "load",
    "loadstring",
    "dofile",
    "loadfile",
    "getfenv",
    "setfenv",
    "newproxy",
    "module",
    "collectgarbage",
    "gcinfo",
    "string.dump",
    // 5.1's deprecated alias of `string.gmatch`.
    "string.gfind",
    // Not loaded by StdLib selection, but strip any that leak in anyway.
    "io",
    "os",
    "package",
    "debug",
];

/// Build a fresh standard Lua 5.1 engine with our curated stdlib in place.
///
/// Only the language-semantics libraries load: TABLE/STRING/MATH, plus BASE which `mlua` always opens
/// (and which carries `coroutine` in 5.1: `pairs`, `ipairs`, `tostring`, `pcall`, ...). IO/OS/PACKAGE/DEBUG
/// never load: they are sandbox hazards and our `fs`/`os`/`process` modules cover their legitimate uses.
/// The unsafe parts of BASE and STRING are then stripped (see [`STRIPPED_GLOBALS`]), and the Rust `bit`
/// library (Lua 5.1 has none), yield-safe `pcall`/`xpcall` and the custom `require` are installed.
///
/// # Errors
/// Fails when the engine, a library or a module cannot be set up.
pub fn make_engine() -> mlua::Result<Lua> {
    let lua = base_engine()?;
    lua.set_app_data(crate::lua::std::task::Scheduler::new());
    modules::install(&lua)?;
    Ok(lua)
}

/// Build a bare standard Lua 5.1 engine for untrusted app code: the curated stdlib, the `bit` library and
/// the yield-safe `pcall`/`xpcall` of [`make_engine`], but NO `std:`/`lib:`/`vnd:` modules, no scheduler and
/// no `require` (the embedder installs its own, and gives each script its own environment, see
/// [`crate::lua::app_env`]).
///
/// The one shared string metatable is locked (`__metatable`), so `getmetatable("").__index` cannot be used
/// to reach, and mutate, the shared `string` table.
///
/// To offer a script some of the modules later, install them with [`modules::install_allowed`]: this
/// function deliberately exposes none.
///
/// # Errors
/// Fails when the engine or a library cannot be set up.
pub fn make_bare_engine() -> mlua::Result<Lua> {
    let lua = base_engine()?;
    lock_string_metatable(&lua)?;
    Ok(lua)
}

/// The libraries, the stripped globals, `bit` and the yield-safe `pcall` shared by both engines.
fn base_engine() -> mlua::Result<Lua> {
    let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH, mlua::LuaOptions::new())?;
    strip_unsafe_globals(&lua)?;
    bit::install(&lua)?;
    install_yieldable_pcall(&lua)?;
    Ok(lua)
}

/// Every string shares one metatable (`{ __index = string }`) that `getmetatable("")` returns. Locking it
/// makes `getmetatable("")` return the lock value instead, so a script cannot reach the table `__index`
/// names. (Scripts cannot `setmetatable` a string, and `debug.setmetatable` is not loaded.)
fn lock_string_metatable(lua: &Lua) -> mlua::Result<()> {
    let meta: Option<mlua::Table> = lua.load("return getmetatable('')").eval()?;
    if let Some(meta) = meta {
        meta.raw_set("__metatable", "locked")?;
    }
    Ok(())
}

/// Load script source as a text-only chunk.
///
/// Lua 5.1 bytecode is unverified — a crafted `\x1bLua` chunk corrupts memory
/// and escapes the sandbox — so every entry point that runs script-supplied
/// source goes through here and refuses binary chunks.
#[track_caller]
pub fn load_source<'a>(lua: &Lua, source: impl AsChunk + 'a) -> Chunk<'a> {
    lua.load(source).set_mode(ChunkMode::Text)
}

/// What the embedder tells the engine about the script's state, for the library's own Lua parts to act on (see
/// [`set_watchdog`]).
#[derive(Clone)]
pub struct Watchdog {
    /// Has the script been stopped for good (its CPU budget ran out, or it was unloaded)?
    pub stopped: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Is a host call that nobody is waiting for any more still alive? (A host call that cannot yield, in a
    /// `gsub` callback or a metamethod, fails and leaves its future in a dead coroutine until the garbage
    /// collector frees it; it can hold the database's lock, and the CPU slice stays "waiting" until it is gone.)
    pub orphaned: Arc<dyn Fn() -> bool + Send + Sync>,
}

struct WatchdogSlot(Watchdog);

/// Install the embedder's [`Watchdog`].
///
/// The embedder's count hook stops a script by raising an error, and a script can catch it (`pcall`, `xpcall`,
/// `coroutine.resume`) and carry on with a function that returns before the hook runs again. The library's own
/// `pcall`, `xpcall` and `coroutine.resume` therefore ask `stopped` after catching an error and, when it is
/// true, raise the error again: a stop cannot be swallowed, whatever the handler does and however deeply the
/// protected calls nest. And `pcall` / `xpcall`, having caught a failure, ask `orphaned` and, when it is true,
/// collect the dead coroutine's garbage at once (and with it the orphan's future). Without a watchdog (the
/// default) nothing is re-raised and nothing is collected.
pub fn set_watchdog(lua: &Lua, watchdog: Watchdog) {
    lua.set_app_data(WatchdogSlot(watchdog));
}

/// The Lua function `killed()` the library's Lua parts call: asks [`Watchdog::stopped`] when called.
pub(crate) fn killed_function(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, ()| {
        Ok(lua.app_data_ref::<WatchdogSlot>().is_some_and(|slot| (slot.0.stopped)()))
    })
}

/// The Lua function `reclaim()`: runs a full garbage collection when [`Watchdog::orphaned`] says a host call
/// was left behind.
pub(crate) fn reclaim_function(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, ()| {
        let orphaned = lua.app_data_ref::<WatchdogSlot>().is_some_and(|slot| (slot.0.orphaned)());
        if orphaned {
            lua.gc_collect()?;
        }
        Ok(())
    })
}

/// Replace `pcall`/`xpcall` with versions a coroutine can yield through (see `yieldable_pcall.lua`).
/// Standard Lua 5.1 cannot, and an async host call inside a protected call must.
fn install_yieldable_pcall(lua: &Lua) -> mlua::Result<()> {
    lua.load(include_str!("yieldable_pcall.lua"))
        .set_name("=yieldable_pcall")
        .set_mode(ChunkMode::Text)
        .call((killed_function(lua)?, reclaim_function(lua)?))
}

fn strip_unsafe_globals(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    for path in STRIPPED_GLOBALS {
        match path.split_once('.') {
            None => globals.raw_set(*path, Value::Nil)?,
            Some((table, key)) => {
                if let Value::Table(t) = globals.raw_get::<Value>(table)? {
                    t.raw_set(key, Value::Nil)?;
                }
            }
        }
    }
    Ok(())
}

/// Convert a Rust `i64` to a Lua number.
///
/// Lua 5.1 numbers are IEEE doubles, so integers beyond 2^53 lose precision on
/// the way in; callers that need exact 64-bit values must pass strings.
pub fn i64_to_lua(v: i64) -> Value {
    Value::Number(v as f64)
}

/// Helper: build a `(value, nil)` success tuple.
pub fn ok<V: IntoLua>(lua: &Lua, v: V) -> mlua::Result<(Value, Value)> {
    Ok((v.into_lua(lua)?, Value::Nil))
}

/// Helper: build a `(nil, "msg")` error tuple.
pub fn err(lua: &Lua, msg: impl Into<String>) -> mlua::Result<(Value, Value)> {
    let s = lua.create_string(msg.into().as_bytes())?;
    Ok((Value::Nil, Value::String(s)))
}

/// Helper: turn an `anyhow`-ish stringifiable error into the `(nil, err)` tuple.
pub fn from_err<E: ::std::fmt::Display>(lua: &Lua, prefix: &str, e: E) -> mlua::Result<(Value, Value)> {
    err(lua, format!("{prefix}: {e}"))
}

/// Real Lua 5.1 bytecode for `return 7`: the escape-vector fixture for the text-only loader tests.
///
/// Dumped in an unsandboxed state: the engine has stripped `string.dump`.
#[cfg(test)]
pub(crate) fn bytecode() -> Vec<u8> {
    let lua = Lua::new();
    let dump: mlua::LuaString = lua.load("return string.dump(function() return 7 end)").eval().expect("dump");
    dump.as_bytes().to_vec()
}

/// [`bytecode`] as a `String`, for loaders that take script source as text (`std:thread`'s `source` and
/// `file`). A dump is not valid UTF-8 (the closing `RETURN` word has a high byte), so the lossy
/// conversion replaces those bytes; the `\x1bLua` header the guard keys on is intact.
#[cfg(test)]
pub(crate) fn text_bytecode() -> String {
    String::from_utf8_lossy(&bytecode()).into_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const TWO_53: i64 = 1 << 53;

    fn engine() -> Lua {
        make_engine().expect("engine")
    }

    /// `name -> type(value)` for every key of the table at `path`.
    fn surface(lua: &Lua, path: &str) -> BTreeMap<String, String> {
        lua.load(format!("local t = {{}} for k, v in pairs({path}) do t[k] = type(v) end return t"))
            .eval()
            .unwrap_or_else(|e| panic!("surface({path}): {e}"))
    }

    fn expected(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries.iter().map(|(k, t)| ((*k).to_string(), (*t).to_string())).collect()
    }

    // Allowlists, not denylists: a global or string function a Lua/mlua
    // bump adds fails here until someone decides whether it is safe.
    #[test]
    fn given_fresh_engine_when_enumerating_globals_then_exactly_the_allowlist_remains() {
        let lua = engine();
        assert_eq!(
            surface(&lua, "_G"),
            expected(&[
                // Libraries.
                ("_G", "table"),
                ("bit", "table"),
                ("coroutine", "table"),
                ("math", "table"),
                ("string", "table"),
                ("table", "table"),
                // Base functions kept after stripping.
                ("assert", "function"),
                ("error", "function"),
                ("getmetatable", "function"),
                ("ipairs", "function"),
                ("next", "function"),
                ("pairs", "function"),
                ("pcall", "function"),
                ("print", "function"),
                ("rawequal", "function"),
                ("rawget", "function"),
                ("rawset", "function"),
                ("select", "function"),
                ("setmetatable", "function"),
                ("tonumber", "function"),
                ("tostring", "function"),
                ("type", "function"),
                ("unpack", "function"),
                ("xpcall", "function"),
                ("_VERSION", "string"),
                // The custom module resolver.
                ("require", "function"),
            ])
        );
    }

    #[test]
    fn given_fresh_engine_when_enumerating_string_library_then_exactly_the_allowlist_remains() {
        let lua = engine();
        assert_eq!(
            surface(&lua, "string"),
            expected(&[
                ("byte", "function"),
                ("char", "function"),
                ("find", "function"),
                ("format", "function"),
                ("gmatch", "function"),
                ("gsub", "function"),
                ("len", "function"),
                ("lower", "function"),
                ("match", "function"),
                ("rep", "function"),
                ("reverse", "function"),
                ("sub", "function"),
                ("upper", "function"),
            ])
        );
    }

    #[test]
    fn given_fresh_engine_when_requiring_a_module_then_custom_require_resolves_it() {
        let lua = engine();
        let ty: String = lua.load(r#"return type(require("lib:base64"))"#).eval().unwrap();
        assert_eq!(ty, "table");
    }

    #[test]
    fn given_fresh_engine_when_requiring_unknown_module_then_custom_require_errors() {
        let lua = engine();
        let err =
            lua.load(r#"return require("lib:no_such_module")"#).eval::<Value>().unwrap_err().to_string();
        assert!(err.contains("require: unknown module"), "got: {err}");
    }

    #[test]
    fn given_stripped_load_when_script_uses_it_then_call_fails() {
        let lua = engine();
        let res = lua.load(r#"return load("return 1")"#).eval::<Value>();
        assert!(res.is_err(), "load must be unreachable");
    }

    #[test]
    fn given_bytecode_when_loaded_as_source_then_rejected() {
        let lua = engine();
        let raw = bytecode();
        assert!(raw.starts_with(b"\x1bLua"), "fixture must be Lua 5.1 bytecode");
        let err =
            load_source(&lua, raw.clone()).eval::<Value>().expect_err("bytecode must not load").to_string();
        assert!(err.contains("binary"), "got: {err}");

        // The same bytes do load (and run) through a plain `Lua::load`, so the guard is what stops them.
        let n: i64 = lua.load(raw).eval().expect("unguarded load runs the bytecode");
        assert_eq!(n, 7);
    }

    #[test]
    fn given_text_bytecode_when_loaded_as_source_then_rejected() {
        let lua = engine();
        let source = text_bytecode();
        assert!(source.starts_with("\x1bLua"));
        let err = load_source(&lua, &source).eval::<Value>().expect_err("bytecode must not load").to_string();
        assert!(err.contains("binary"), "got: {err}");
    }

    #[test]
    fn given_text_when_loaded_as_source_then_runs() {
        let lua = engine();
        let n: i64 = load_source(&lua, "return 7").eval().unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn given_integers_up_to_2_pow_53_when_converted_then_exact() {
        for v in [0, 1, -1, -42, TWO_53, -TWO_53, i64::MIN] {
            let Value::Number(n) = i64_to_lua(v) else {
                panic!("{v}: expected a number");
            };
            assert_eq!(n as i64, v, "{v} must round-trip");
        }
    }

    #[test]
    fn given_integers_beyond_2_pow_53_when_converted_then_rounded_to_nearest_double() {
        for (v, want) in [
            (TWO_53 + 1, TWO_53 as f64),
            (-(TWO_53 + 1), -(TWO_53 as f64)),
            (i64::MAX, 9_223_372_036_854_775_808.0),
        ] {
            assert_eq!(i64_to_lua(v), Value::Number(want), "{v}");
        }
    }

    fn eval_str(lua: &Lua, src: &str) -> String {
        lua.load(src).eval::<String>().unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn given_pcall_when_function_succeeds_then_every_result_comes_back_nils_included() {
        let lua = engine();
        assert_eq!(
            eval_str(&lua, "return select('#', pcall(function(...) return ... end, 1, nil, 3, nil))"),
            "5"
        );
    }

    #[test]
    fn given_pcall_when_function_errors_then_message_and_error_objects_are_returned() {
        let lua = engine();
        assert_eq!(
            eval_str(&lua, r#"local ok, e = pcall(error, "boom", 0) return tostring(ok) .. ":" .. e"#),
            "false:boom"
        );
        assert_eq!(
            eval_str(&lua, "local t = {} local ok, e = pcall(error, t) return tostring(e == t)"),
            "true"
        );
        assert_eq!(
            eval_str(
                &lua,
                r#"local ok, e = pcall(function() local x = nil; return x.y end) return tostring(ok) .. ":" .. type(e)"#
            ),
            "false:string"
        );
    }

    #[test]
    fn given_xpcall_when_function_errors_then_the_handler_shapes_the_result() {
        let lua = engine();
        assert_eq!(
            eval_str(
                &lua,
                r#"local ok, e = xpcall(error, function(m) return "handled " .. m end, "boom", 0) return tostring(ok) .. ":" .. e"#
            ),
            "false:handled boom"
        );
        assert_eq!(
            eval_str(
                &lua,
                r#"local ok, e = xpcall(error, function(m) error("again") end, "boom") return tostring(ok) .. ":" .. e"#
            ),
            "false:error in error handling"
        );
    }

    #[test]
    fn given_nested_pcall_inside_a_coroutine_when_it_yields_then_the_yield_crosses_both() {
        let lua = engine();
        assert_eq!(
            eval_str(
                &lua,
                r#"
                local co = coroutine.create(function()
                    local ok, v = pcall(function()
                        local ok2, v2 = pcall(function() return coroutine.yield("first") end)
                        return v2 .. "!"
                    end)
                    return tostring(ok) .. ":" .. v
                end)
                local _, y = coroutine.resume(co)
                local _, r = coroutine.resume(co, "second")
                return y .. "," .. r
            "#
            ),
            "first,true:second!"
        );
    }

    #[test]
    fn given_a_yielding_function_when_called_outside_any_coroutine_then_pcall_reports_instead_of_raising() {
        let lua = engine();
        assert_eq!(
            eval_str(&lua, "local ok, e = pcall(coroutine.yield, 1) return tostring(ok) .. ':' .. e"),
            "false:attempt to yield from outside a coroutine"
        );
    }

    #[test]
    fn given_bit_library_when_xor_then_computes() {
        let lua = engine();
        let n: i64 = lua.load("return bit.bxor(5, 3)").eval().unwrap();
        assert_eq!(n, 6);
    }
}
