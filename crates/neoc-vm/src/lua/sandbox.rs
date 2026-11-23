//! Engine factory + (value, err) helpers shared by every lib module.

use mlua::chunk::{AsChunk, Chunk, ChunkMode};
use mlua::{IntoLua, Lua, StdLib, Value};

use crate::lua::lifecycle::Life;
use crate::lua::modules;

/// Globals removed after the standard libraries load.
///
/// Each is a sandbox hazard: runtime code loading (`loadstring`, which mlua adds to Luau), environment/identity
/// tampering (`getfenv`, `setfenv`, `newproxy`), GC control and introspection (`collectgarbage`, which mlua
/// adds, `gcinfo`) and mlua's file-system `require` (the engine's own is installed by `modules::install`).
/// Luau has no `load`, `dofile`, `loadfile`, `module` or `string.dump`; the `io`, `os`, `package`, `debug`,
/// `utf8`, `vector` and `integer` names are not opened either and stay on the list so a library a future
/// mlua opens by default is removed (`jit` and `ffi` went with LuaJIT). Paths are dotted; the last segment is the key removed from its parent table.
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
    "require",
    "string.dump",
    // Luau's `coroutine.close` kills a suspended thread whoever made it: in `neoc run` that is a task's thread, whose
    // `await` would then return nothing at all. LuaJIT had none. The function is kept for a script environment, whose
    // `close` holds it to the ownership rule of `resume` (see `STASHED_CLOSE`).
    "coroutine.close",
    "io",
    "os",
    "package",
    "debug",
    "utf8",
    "vector",
    "integer",
];

/// Build a fresh Luau engine with our curated stdlib in place.
///
/// Only the language-semantics libraries load: COROUTINE/TABLE/STRING/MATH/BIT (Luau's `bit32`) and BUFFER (Luau's
/// fixed-size byte buffer), plus BASE which `mlua` always opens (`pairs`, `ipairs`, `tostring`, `pcall`, ...).
/// OS/DEBUG/UTF8/VECTOR never load: `os` and `debug` are sandbox hazards that our `std:` modules cover, and the
/// others are Luau extensions neoc does not offer (`INTEGER` is not opened either). The unsafe parts of BASE are
/// then stripped (see `STRIPPED_GLOBALS`) and the custom `require` is installed.
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
/// Its globals hold every library [`make_engine`] opens, `buffer` included. A
/// [script environment](crate::lua::script_env) copies only some of them, but a `std:thread` worker (built by
/// [`modules::install_allowed`]'s worker engine, or the embedder's own) runs on its engine's globals: an embedder
/// that keeps a library from its scripts removes it there too, in every worker engine it builds.
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

/// The libraries and the stripped globals shared by both engines.
fn base_engine() -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        StdLib::COROUTINE
            | StdLib::TABLE
            | StdLib::STRING
            | StdLib::MATH
            | StdLib::BIT
            | StdLib::BUFFER,
        mlua::LuaOptions::new(),
    )?;
    stash_coroutine_close(&lua)?;
    strip_unsafe_globals(&lua)?;
    Ok(lua)
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
/// Luau's loader checks the bytecode version and does not verify the instructions (LuaJIT's did not either), so a
/// crafted chunk is treated as able to corrupt memory: every entry point that runs script-supplied source goes
/// through here and refuses binary chunks.
///
/// A leading UTF-8 byte-order mark is dropped and a first line that starts with `#` (a shebang) is blanked, with its
/// newline kept so line numbers do not move: LuaJIT's loader skipped both, Luau's lexer rejects both, and `neoc run
/// script.lua` of an executable script with `#!/usr/bin/env neoc` must go on working (see [`prepare_script`]).
#[track_caller]
pub fn load_source<'a>(lua: &Lua, source: impl AsChunk + 'a) -> Chunk<'a> {
    lua.load(Prepared(source)).set_mode(ChunkMode::Text)
}

/// `source` without a leading UTF-8 byte-order mark, and with a first line that starts with `#` emptied (its newline
/// stays, so line numbers are those of the file).
#[must_use]
pub fn prepare_script(source: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let without_bom = source.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(source);
    if without_bom.first() == Some(&b'#') {
        let rest = without_bom
            .iter()
            .position(|&b| b == b'\n')
            .map_or(&[][..], |newline| &without_bom[newline..]);
        return std::borrow::Cow::Owned(rest.to_vec());
    }
    std::borrow::Cow::Borrowed(without_bom)
}

/// A chunk source passed through [`prepare_script`].
struct Prepared<S>(S);

impl<S: AsChunk> AsChunk for Prepared<S> {
    fn name(&self) -> Option<String> {
        self.0.name()
    }

    fn environment(&self, lua: &Lua) -> mlua::Result<Option<mlua::Table>> {
        self.0.environment(lua)
    }

    fn mode(&self) -> Option<ChunkMode> {
        self.0.mode()
    }

    fn source<'a>(&self) -> std::io::Result<std::borrow::Cow<'a, [u8]>>
    where
        Self: 'a,
    {
        Ok(match self.0.source()? {
            std::borrow::Cow::Borrowed(bytes) => prepare_script(bytes),
            std::borrow::Cow::Owned(bytes) => {
                std::borrow::Cow::Owned(prepare_script(&bytes).into_owned())
            }
        })
    }
}

/// Where the registry keeps Luau's own `coroutine.close`, taken out of `coroutine` before the strip: the script
/// environment's `close` (`async_coroutines.lua`) calls it after its ownership check.
pub(crate) const STASHED_CLOSE: &str = "neoc.coroutine_close";

fn stash_coroutine_close(lua: &Lua) -> mlua::Result<()> {
    let coroutine: mlua::Table = lua.globals().raw_get("coroutine")?;
    let close: Value = coroutine.raw_get("close")?;
    lua.set_named_registry_value(STASHED_CLOSE, close)
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
/// Luau numbers are IEEE doubles, so integers beyond 2^53 lose precision on
/// the way in; callers that need exact 64-bit values must pass strings.
pub fn i64_to_lua(v: i64) -> Value {
    Value::Number(v as f64)
}

/// What a function that can fail hands a script: the value and `nil`, or `nil` and the reason.
///
/// It is the `(value, err)` pair of the runtime as a Rust type, so that the function's signature says what the
/// value is (`mlua::Result<Fallible<String>>`) and the declarations read it from there
/// (`Result<string, string>`). The outer `mlua::Result` is for what raises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallible<T> {
    Ok(T),
    Err(String),
}

impl<T: IntoLua> mlua::IntoLuaMulti for Fallible<T> {
    fn into_lua_multi(self, lua: &Lua) -> mlua::Result<mlua::MultiValue> {
        let pair = match self {
            Self::Ok(value) => [value.into_lua(lua)?, Value::Nil],
            Self::Err(reason) => [Value::Nil, Value::String(lua.create_string(reason)?)],
        };
        Ok(mlua::MultiValue::from_iter(pair))
    }
}

/// The value of a function that can fail: `(value, nil)` to a script.
pub fn ok<T>(value: T) -> mlua::Result<Fallible<T>> {
    Ok(Fallible::Ok(value))
}

/// The failure of a function that can fail: `(nil, "msg")` to a script.
pub fn err<T>(msg: impl Into<String>) -> mlua::Result<Fallible<T>> {
    Ok(Fallible::Err(msg.into()))
}

/// The failure of a function that can fail, from an error and the function's prefix: `(nil, "<prefix>: <e>")`.
pub fn from_err<T, E: ::std::fmt::Display>(prefix: &str, e: E) -> mlua::Result<Fallible<T>> {
    err(format!("{prefix}: {e}"))
}

/// Luau bytecode for `return 7`, as the text the script-facing loaders take. Compiled with the debug level at 0 it
/// has no debug info or chunk name, and every byte is below 0x80 (a version byte, small counts and the instruction
/// words), so it survives the `String` conversion script-facing loaders apply. The escape-vector fixture for the
/// text-only loader tests: Luau's bytecode is not verified when it is loaded.
#[cfg(test)]
pub(crate) fn luau_bytecode() -> String {
    let bytes = mlua::chunk::Compiler::new()
        .set_debug_level(0)
        .compile("return 7")
        .expect("compile");
    assert!(
        bytes[0] != 0,
        "a chunk starting with 0 is the compiler's error marker"
    );
    String::from_utf8(bytes).expect("the bytecode of `return 7` is ASCII")
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

    // Allowlists, not denylists: a global or string function a Luau/mlua
    // bump adds fails here until someone decides whether it is safe.
    #[test]
    fn given_fresh_engine_when_enumerating_globals_then_exactly_the_allowlist_remains() {
        let lua = engine();
        assert_eq!(
            surface(&lua, "_G"),
            expected(&[
                // Libraries.
                ("_G", "table"),
                ("bit32", "table"),
                ("buffer", "table"),
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
                ("rawlen", "function"),
                ("rawset", "function"),
                ("select", "function"),
                ("setmetatable", "function"),
                ("tonumber", "function"),
                ("tostring", "function"),
                ("type", "function"),
                ("typeof", "function"),
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
                ("pack", "function"),
                ("packsize", "function"),
                ("rep", "function"),
                ("reverse", "function"),
                ("split", "function"),
                ("sub", "function"),
                ("unpack", "function"),
                ("upper", "function"),
            ])
        );
    }

    // Luau's byte buffer: fixed size, read and written at offsets, little-endian. Nothing in it reaches outside
    // the buffer, and a function a Luau bump adds fails here until someone looks at it.
    #[test]
    fn given_fresh_engine_when_enumerating_buffer_library_then_exactly_the_allowlist_remains() {
        let lua = engine();
        assert_eq!(
            surface(&lua, "buffer"),
            expected(&[
                ("copy", "function"),
                ("create", "function"),
                ("fill", "function"),
                ("fromstring", "function"),
                ("len", "function"),
                ("readbits", "function"),
                ("readf32", "function"),
                ("readf64", "function"),
                ("readi16", "function"),
                ("readi32", "function"),
                ("readi8", "function"),
                ("readstring", "function"),
                ("readu16", "function"),
                ("readu32", "function"),
                ("readu8", "function"),
                ("tostring", "function"),
                ("writebits", "function"),
                ("writef32", "function"),
                ("writef64", "function"),
                ("writei16", "function"),
                ("writei32", "function"),
                ("writei8", "function"),
                ("writestring", "function"),
                ("writeu16", "function"),
                ("writeu32", "function"),
                ("writeu8", "function"),
            ])
        );
    }

    // `STRIPPED_GLOBALS` seen from a script: the engine of `neoc run` has its own `require` and nothing else on the
    // list; a bare engine has none of them (mlua installs a file-system `require`, `loadstring` and
    // `collectgarbage`, which are stripped; `utf8`, `vector` and `integer` are not opened)
    #[test]
    fn given_either_engine_when_probing_the_stripped_names_then_none_is_reachable() {
        let names = "{ 'load', 'loadstring', 'dofile', 'loadfile', 'getfenv', 'setfenv', 'newproxy', 'module', \
             'collectgarbage', 'gcinfo', 'io', 'os', 'package', 'debug', 'utf8', 'vector', 'integer' }";
        let probe = format!(
            "local found = {{}} for _, n in ipairs({names}) do if _G[n] ~= nil then found[#found + 1] = n end end \
             if string.dump then found[#found + 1] = 'string.dump' end if coroutine.close then found[#found + 1] = 'coroutine.close' end return table.concat(found, ',') .. '|' .. type(require)"
        );
        let full: String = make_engine().unwrap().load(&probe).eval().unwrap();
        assert_eq!(
            full, "|function",
            "the engine's own require, nothing stripped left"
        );
        let bare: String = make_bare_engine().unwrap().load(&probe).eval().unwrap();
        assert_eq!(
            bare, "|nil",
            "a bare engine has no require, not even mlua's file-system one"
        );
        // and both open `buffer` (an embedder that keeps it from its scripts relies on knowing where it is)
        for lua in [make_engine().unwrap(), make_bare_engine().unwrap()] {
            let ty: String = lua.load("return type(buffer)").eval().unwrap();
            assert_eq!(ty, "table");
        }
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
        let source = luau_bytecode();
        assert!(
            source.starts_with(|c: char| c < ' '),
            "fixture must be Luau bytecode (a version byte, not source text)"
        );
        let err = load_source(&lua, &source)
            .eval::<Value>()
            .expect_err("bytecode must not load")
            .to_string();
        assert!(err.contains("binary"), "got: {err}");
    }

    #[test]
    fn given_a_shebang_or_a_byte_order_mark_when_loaded_as_source_then_it_runs_and_lines_stay() {
        let lua = engine();
        for (name, source) in [
            (
                "shebang",
                "#!/usr/bin/env neoc\nreturn debug_line()".to_owned(),
            ),
            ("bom", "\u{feff}return debug_line()".to_owned()),
            (
                "bom and shebang",
                "\u{feff}#!/usr/bin/env neoc\nreturn debug_line()".to_owned(),
            ),
        ] {
            lua.globals()
                .set("debug_line", lua.create_function(|_, ()| Ok(1)).unwrap())
                .unwrap();
            let n: i64 = load_source(&lua, &source)
                .eval()
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(n, 1, "{name}");
        }
        // the shebang line keeps its newline, so an error on line 2 says line 2
        let err = load_source(&lua, "#!/usr/bin/env neoc\nerror('boom')")
            .set_name("=script")
            .exec()
            .unwrap_err()
            .to_string();
        assert!(err.contains("script:2: boom"), "{err}");
        // a file that is only a shebang, with or without a newline
        for source in ["#!x", "#!x\n", "\u{feff}#!x"] {
            load_source(&lua, source)
                .exec()
                .unwrap_or_else(|e| panic!("{source:?}: {e}"));
        }
        // only the first line: a later `#` is still a syntax error
        assert!(load_source(&lua, "return 1\n#!x").exec().is_err());
        // bytecode still starts with its version byte and is still refused
        assert!(load_source(&lua, luau_bytecode()).exec().is_err());
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
        let n: i64 = lua.load("return bit32.bxor(5, 3)").eval().unwrap();
        assert_eq!(n, 6);
    }
}
