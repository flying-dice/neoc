//! The engine factory: a Luau state with the curated standard library, the runtime's state and a `require`
//! over the modules.

use mlua::chunk::{AsChunk, Chunk, ChunkMode};
use mlua::{Lua, StdLib, Value};

use crate::lifecycle::Life;
use crate::modules;
use crate::scheduler::Scheduler;

/// Globals removed after the standard libraries load. Each is a sandbox hazard: runtime code loading
/// (`loadstring`, which mlua adds to Luau), environment and identity tampering (`getfenv`, `setfenv`, `newproxy`),
/// GC control (`collectgarbage`, `gcinfo`) and mlua's file-system `require` (the engine's own is installed by
/// [`modules::install`]). The `io`, `os`, `package`, `debug`, `utf8`, `vector` and `integer` libraries are not opened
/// either and stay on the list so a library a future mlua opens by default is removed. Paths are dotted; the last
/// segment is the key removed from its parent table. `coroutine.close` goes too: it kills a suspended thread
/// whoever made it (a task's thread, whose `await` would then return nothing at all). The function is kept for a
/// [script environment](crate::script_env), whose `close` holds it to the ownership rule of `resume` (see
/// [`STASHED_CLOSE`]).
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
    "coroutine.close",
    "io",
    "os",
    "package",
    "debug",
    "utf8",
    "vector",
    "integer",
];

/// A fresh engine: the language libraries only (`coroutine`, `table`, `string`, `math`, `bit32`, `buffer`, and the
/// base library mlua always opens), the unsafe globals stripped, the string metatable locked, every module behind
/// `require("<namespace>:<name>")`.
///
/// # Errors
/// Fails when the engine or a module cannot be set up.
pub fn make_engine() -> mlua::Result<Lua> {
    make_engine_in(Life::new())
}

/// [`make_engine`] whose spawned work registers with `life`: an embedder that unloads the script calls
/// [`Life::shutdown`].
///
/// # Errors
/// Fails when the engine or a module cannot be set up.
pub fn make_engine_in(life: Life) -> mlua::Result<Lua> {
    let lua = make_bare_engine()?;
    install_runtime_state(&lua, life);
    modules::install(&lua)?;
    Ok(lua)
}

/// What the modules keep per engine: the registry of what the script spawns and the `std:task` scheduler over it.
/// [`make_engine`] sets it; an embedder that builds its own engine from [`make_bare_engine`] and offers the modules
/// itself calls this first.
pub fn install_runtime_state(lua: &Lua, life: Life) {
    Life::adopt(lua, life.clone());
    lua.set_app_data(Scheduler::with_life(life));
}

/// An engine with the curated standard library and nothing else: no modules, no scheduler and no `require`. For an
/// embedder that offers some of the modules itself ([`modules::build`], after [`install_runtime_state`]) and runs
/// each script in its own [environment](crate::script_env).
///
/// # Errors
/// Fails when the engine or a library cannot be set up.
pub fn make_bare_engine() -> mlua::Result<Lua> {
    crate::host::install();
    let lua = Lua::new_with(
        StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT | StdLib::BUFFER,
        mlua::LuaOptions::new(),
    )?;
    stash_coroutine_close(&lua)?;
    strip_unsafe_globals(&lua)?;
    lock_string_metatable(&lua)?;
    Ok(lua)
}

/// Where the registry keeps Luau's own `coroutine.close`, taken out of `coroutine` before the strip: the script
/// environment's `close` (`async_coroutines.lua`) calls it after its ownership check.
pub(crate) const STASHED_CLOSE: &str = "neoc.coroutine_close";

fn stash_coroutine_close(lua: &Lua) -> mlua::Result<()> {
    let coroutine: mlua::Table = lua.globals().raw_get("coroutine")?;
    let close: Value = coroutine.raw_get("close")?;
    lua.set_named_registry_value(STASHED_CLOSE, close)
}

/// Every string shares one metatable (`{ __index = string }`) that `getmetatable("")` returns. Locking it makes
/// `getmetatable("")` return the lock value, so a script cannot reach and change the shared `string` table.
fn lock_string_metatable(lua: &Lua) -> mlua::Result<()> {
    let meta: Option<mlua::Table> = lua.load("return getmetatable('')").eval()?;
    if let Some(meta) = meta {
        meta.raw_set("__metatable", "locked")?;
    }
    Ok(())
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

/// Loads script source as a text-only chunk. Luau's loader does not verify bytecode, so every entry point that runs
/// script-supplied source goes through here and refuses binary chunks. A leading byte-order mark is dropped and a
/// first line that starts with `#` (a shebang) is blanked with its newline kept, so line numbers do not move.
#[track_caller]
pub fn load_source<'a>(lua: &Lua, source: impl AsChunk + 'a) -> Chunk<'a> {
    lua.load(Prepared(source)).set_mode(ChunkMode::Text)
}

/// `source` without a leading byte-order mark, and with a first line that starts with `#` emptied.
#[must_use]
pub fn prepare_script(source: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let without_bom = source.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(source);
    if without_bom.first() == Some(&b'#') {
        let rest =
            without_bom.iter().position(|&b| b == b'\n').map_or(&[][..], |newline| &without_bom[newline..]);
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
            std::borrow::Cow::Owned(bytes) => std::borrow::Cow::Owned(prepare_script(&bytes).into_owned()),
        })
    }
}

/// Luau bytecode for `return 7`, as the text the script-facing loaders take. Compiled with the debug level at 0 it
/// has no debug info or chunk name, and every byte is below 0x80, so it survives the `String` conversion
/// script-facing loaders apply. The fixture of the text-only loader tests: Luau's bytecode is not verified when it
/// is loaded.
#[cfg(test)]
pub(crate) fn luau_bytecode() -> String {
    let bytes = mlua::chunk::Compiler::new().set_debug_level(0).compile("return 7").expect("compile");
    assert!(bytes[0] != 0, "a chunk starting with 0 is the compiler's error marker");
    String::from_utf8(bytes).expect("the bytecode of `return 7` is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sandbox_is_stripped_and_require_is_ours() {
        let lua = make_engine().expect("engine");
        let (load, os, close, require): (String, String, String, String) = lua
            .load("return type(loadstring), type(os), type(coroutine.close), type(require)")
            .eval()
            .expect("script");
        assert_eq!(
            (load.as_str(), os.as_str(), close.as_str(), require.as_str()),
            ("nil", "nil", "nil", "function")
        );
    }

    #[test]
    fn the_string_metatable_is_locked() {
        let lua = make_engine().expect("engine");
        let meta: String = lua.load("return getmetatable('')").eval().expect("script");
        assert_eq!(meta, "locked");
    }

    #[test]
    fn a_shebang_and_a_bom_are_dropped_and_line_numbers_stay() {
        assert_eq!(&*prepare_script(b"#!/usr/bin/env neoc\nreturn 1"), b"\nreturn 1");
        assert_eq!(&*prepare_script(b"\xEF\xBB\xBFreturn 1"), b"return 1");
    }

    #[test]
    fn binary_chunks_are_refused() {
        let lua = make_engine().expect("engine");
        assert!(load_source(&lua, &b"\x03\x01garbage"[..]).exec().is_err());
    }
}
