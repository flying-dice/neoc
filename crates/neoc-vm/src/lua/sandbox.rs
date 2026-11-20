//! Engine factory + (value, err) helpers shared by every lib module.

use mlua::chunk::{AsChunk, Chunk, ChunkMode};
use mlua::{IntoLua, Lua, StdLib, Value};

use crate::lua::lifecycle::Life;
use crate::lua::modules;

pub use crate::lua::guard::call_guarded_async;

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
    // Not loaded by StdLib selection, but `luaopen_base` and the LuaJIT
    // runtime may register them regardless — remove any that leak.
    "io",
    "os",
    "package",
    "jit",
    "ffi",
    "debug",
];

/// Build a fresh LuaJIT engine with our curated stdlib in place.
///
/// Only the language-semantics libraries load: TABLE/STRING/MATH/
/// BIT, plus BASE which `mlua` always opens along with `coroutine` (`pairs`, `ipairs`, `tostring`,
/// `pcall`, ...). IO/OS/PACKAGE/DEBUG/JIT/FFI never load — they are sandbox
/// hazards and our `fs`/`os`/`process` modules cover their legitimate uses.
/// The unsafe parts of BASE and STRING are then stripped
/// (see `STRIPPED_GLOBALS`) and the custom `require` is installed.
///
/// # Errors
/// Fails when the engine, a library or a module cannot be set up.
pub fn make_engine() -> mlua::Result<Lua> {
    make_engine_in(Life::new())
}

/// [`make_engine`] whose spawned work (tasks, servers, workers) registers with `life`: an embedder that
/// shuts the script down calls [`Life::shutdown`], and a `std:thread` worker joins the registry of the engine
/// that started it. Dropping the engine is not a shutdown: a `vnd:hyper` server's accept loop and its
/// connections hold the VM they call into, so they (and the VM) outlive the drop until [`Life::shutdown`].
///
/// # Errors
/// Fails when the engine, a library or a module cannot be set up.
pub fn make_engine_in(life: Life) -> mlua::Result<Lua> {
    let lua = base_engine()?;
    install_runtime_state(&lua, life);
    modules::install(&lua)?;
    // a worker of this script is a full engine like it, joining the registry it is given
    set_worker_engine(&lua, std::sync::Arc::new(make_engine_in));
    Ok(lua)
}

/// The engine of a worker script offered only the `allowed` modules: what `std:thread` builds for a worker whose
/// parent used [`modules::install_allowed`].
///
/// # Errors
/// Fails when the engine, a library or a module cannot be set up.
pub(crate) fn make_engine_with(life: Life, allowed: &[&str]) -> mlua::Result<Lua> {
    let lua = base_engine()?;
    install_runtime_state(&lua, life);
    modules::install_allowed(&lua, allowed)?;
    Ok(lua)
}

/// How an embedder builds the engine of a `std:thread` worker its script starts: given the script's [`Life`] (the
/// worker's tasks, servers and workers join it), the engine, offered what the embedder offers its scripts.
pub type WorkerEngineFn = std::sync::Arc<dyn Fn(Life) -> mlua::Result<Lua> + Send + Sync>;

struct WorkerEngine(WorkerEngineFn);

/// Install the [`WorkerEngineFn`] of this engine's scripts. [`make_engine_in`] installs one that builds a full
/// engine (so `neoc run` workers get what they always did) and [`modules::install_allowed`] one that builds the
/// same subset. A bare engine ([`make_bare_engine`]) has none, and `std:thread` refuses to start a worker on it
/// until this is called: a worker is never offered more than its parent without the embedder saying so. An
/// embedder that replaces the `require` of a [`make_engine_in`] engine with a narrower one must call this too.
pub fn set_worker_engine(lua: &Lua, factory: WorkerEngineFn) {
    lua.set_app_data(WorkerEngine(factory));
}

/// How `std:thread` builds a worker's engine for the script that runs in `lua`: the engine's [`WorkerEngineFn`],
/// or an error when it has none (a bare engine whose embedder did not say how).
pub(crate) fn worker_engine_builder(lua: &Lua) -> Result<WorkerEngineFn, String> {
    match lua.app_data_ref::<WorkerEngine>() {
        Some(factory) => Ok(factory.0.clone()),
        None => Err("this engine offers its script modules through its own `require` and has no worker \
             engine: the embedder must call `set_worker_engine` (or `install_allowed`) before a script \
             can start a worker"
            .to_owned()),
    }
}

/// What the `std:`/`lib:`/`vnd:` modules keep per engine: the registry of what the script spawns and the
/// `std:task` scheduler over it. [`make_engine`] sets it; an embedder that builds its own engine from
/// [`make_bare_engine`] and offers the modules itself calls this first.
pub fn install_runtime_state(lua: &Lua, life: Life) {
    Life::adopt(lua, life.clone());
    lua.set_app_data(crate::lua::std::task::Scheduler::with_life(life));
}

/// Build a bare engine for a script an embedder runs in its own [script environment](crate::lua::script_env):
/// the curated stdlib of [`make_engine`], but NO `std:`/`lib:`/`vnd:` modules, no scheduler and no `require`
/// (the embedder installs its own and gives each script its own environment).
///
/// The one shared string metatable is locked (`__metatable`), so `getmetatable("").__index` cannot be used
/// to reach, and mutate, the shared `string` table.
///
/// To offer a script some of the modules, build them with [`modules::build`] or install a subset with
/// [`modules::install_allowed`]: this function deliberately exposes none. For the same reason a script on it
/// cannot start a `std:thread` worker until the embedder says how to build one ([`set_worker_engine`], which
/// `install_allowed` calls): a worker is never offered more than its parent.
///
/// # Errors
/// Fails when the engine or a library cannot be set up.
pub fn make_bare_engine() -> mlua::Result<Lua> {
    let lua = base_engine()?;
    lock_string_metatable(&lua)?;
    Ok(lua)
}

/// The libraries, the stripped globals and the guarded `xpcall` shared by both engines.
fn base_engine() -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT,
        mlua::LuaOptions::new(),
    )?;
    strip_unsafe_globals(&lua)?;
    // before `xpcall` is replaced: the guarded thread bodies use the builtin
    crate::lua::guard::install(&lua)?;
    install_guarded_xpcall(&lua)?;
    Ok(lua)
}

/// Replace `xpcall` with a version whose message handler cannot fail the error handling (see
/// `guarded_xpcall.lua`): on LuaJIT a handler that errors after a protected call of its own is called again by
/// its own error, until a stack overflow that is past the stack of a 2 MiB thread.
fn install_guarded_xpcall(lua: &Lua) -> mlua::Result<()> {
    load_source(lua, include_str!("guarded_xpcall.lua"))
        .set_name("=guarded_xpcall")
        .exec()
}

/// Every string shares one metatable (`{ __index = string }`) that `getmetatable("")` returns. Locking it
/// makes `getmetatable("")` return the lock value instead, so a script cannot reach the table `__index`
/// names. (Scripts cannot `setmetatable` a string, and the `debug` library is not loaded.)
fn lock_string_metatable(lua: &Lua) -> mlua::Result<()> {
    let meta: Option<mlua::Table> = lua.load("return getmetatable('')").eval()?;
    if let Some(meta) = meta {
        meta.raw_set("__metatable", "locked")?;
    }
    Ok(())
}

/// Load script source as a text-only chunk.
///
/// LuaJIT bytecode is unverified — a crafted `\x1bLJ` chunk corrupts memory
/// and escapes the sandbox — so every entry point that runs script-supplied
/// source goes through here and refuses binary chunks.
#[track_caller]
pub fn load_source<'a>(lua: &Lua, source: impl AsChunk + 'a) -> Chunk<'a> {
    lua.load(source).set_mode(ChunkMode::Text)
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

/// Call a script function from Rust, with `args`, so that an error it raises reaches the caller as a Lua error with
/// text, whatever the error value is. Use this, not `Function::call`, wherever a host function calls a callback the
/// script gave it: on LuaJIT an error value whose `__tostring` makes a protected call and then fails sends the
/// message handler of a plain `Function::call` into a recursion that overflows a runtime worker's stack and
/// aborts the process (`guarded_call.lua`). It is not for a call that may suspend: that is [`call_guarded_async`].
///
/// # Errors
/// The function raised: the error is its message (a value that is not text is converted without running a
/// `__tostring` that fails).
pub fn call_guarded<R: mlua::FromLuaMulti>(
    lua: &Lua,
    function: &mlua::Function,
    args: impl mlua::IntoLuaMulti,
) -> mlua::Result<R> {
    const KEY: &str = "neoc.guarded_call";
    let trampoline = match lua.named_registry_value::<Value>(KEY)? {
        Value::Function(f) => f,
        _ => {
            let f: mlua::Function = load_source(lua, include_str!("guarded_call.lua"))
                .set_name("=guarded_call")
                .eval()?;
            lua.set_named_registry_value(KEY, f.clone())?;
            f
        }
    };
    let mut returned: mlua::MultiValue = trampoline.call((function.clone(), args))?;
    let ok = matches!(returned.pop_front(), Some(Value::Boolean(true)));
    if ok {
        R::from_lua_multi(returned, lua)
    } else {
        let message = match returned.pop_front() {
            Some(Value::String(s)) => s.to_string_lossy(),
            _ => "<error object>".to_owned(),
        };
        Err(mlua::Error::runtime(message))
    }
}

/// Convert a Rust `i64` to a Lua number.
///
/// LuaJIT numbers are IEEE doubles, so integers beyond 2^53 lose precision on
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
pub fn from_err<E: ::std::fmt::Display>(
    lua: &Lua,
    prefix: &str,
    e: E,
) -> mlua::Result<(Value, Value)> {
    err(lua, format!("{prefix}: {e}"))
}

/// Stripped LuaJIT bytecode for `return 7` — no debug info or chunk name, so
/// every byte is ASCII and it survives the `String` conversion script-facing
/// loaders apply. The escape-vector fixture for the text-only loader tests.
///
/// `string.dump(f, true)` in an unsandboxed state: `Function::dump(true)` does
/// not strip under `LuaJIT`.
#[cfg(test)]
pub(crate) fn ascii_bytecode() -> String {
    let lua = Lua::new();
    let dump: mlua::LuaString = lua
        .load("return string.dump(function() return 7 end, true)")
        .eval()
        .expect("dump");
    let dump = dump.as_bytes().to_vec();
    // Header: `\x1bLJ`, version, flags; `BCDUMP_F_STRIP` = 0x02.
    assert_eq!(dump[4] & 0x02, 0x02, "dump must be stripped");
    String::from_utf8(dump).expect("stripped dump of `return 7` is ASCII")
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
        lua.load(format!(
            "local t = {{}} for k, v in pairs({path}) do t[k] = type(v) end return t"
        ))
        .eval()
        .unwrap_or_else(|e| panic!("surface({path}): {e}"))
    }

    fn expected(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, t)| ((*k).to_string(), (*t).to_string()))
            .collect()
    }

    // Allowlists, not denylists: a global or string function a LuaJIT/mlua
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
        let ty: String = lua
            .load(r#"return type(require("lib:base64"))"#)
            .eval()
            .unwrap();
        assert_eq!(ty, "table");
    }

    #[test]
    fn given_fresh_engine_when_requiring_unknown_module_then_custom_require_errors() {
        let lua = engine();
        let err = lua
            .load(r#"return require("lib:no_such_module")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
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
        let source = ascii_bytecode();
        assert!(
            source.starts_with("\x1bLJ"),
            "fixture must be LuaJIT bytecode"
        );
        let err = load_source(&lua, &source)
            .eval::<Value>()
            .expect_err("bytecode must not load")
            .to_string();
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

    #[test]
    fn given_bit_library_when_xor_then_computes() {
        let lua = engine();
        let n: i64 = lua.load("return bit.bxor(5, 3)").eval().unwrap();
        assert_eq!(n, 6);
    }
}
