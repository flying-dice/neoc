//! The engine factory: a Luau state with the curated standard library, the runtime's state and a `require`
//! over the modules.

use mlua::chunk::{AsChunk, Chunk, ChunkMode};
use mlua::{Lua, StdLib, Value};

use crate::lifecycle::Life;
use crate::modules;

/// Globals removed after the standard libraries load. Each is a sandbox hazard: runtime code loading
/// (`loadstring`, which mlua adds to Luau), environment and identity tampering (`getfenv`, `setfenv`, `newproxy`),
/// GC control (`collectgarbage`, `gcinfo`) and mlua's file-system `require` (the engine's own is installed by
/// [`modules::install`]). The `io`, `os`, `package`, `debug`, `utf8`, `vector` and `integer` libraries are not opened
/// either and stay on the list so a library a future mlua opens by default is removed. Paths are dotted; the last
/// segment is the key removed from its parent table. `coroutine.close` goes too: it kills a suspended thread
/// whoever made it (the script environment that holds it to an ownership rule is not ported to this crate yet).
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
    Life::adopt(&lua, life);
    modules::install(&lua)?;
    Ok(lua)
}

/// An engine with the curated standard library and nothing else: no modules and no `require`. For an embedder that
/// offers some of the modules itself ([`modules::build`]).
///
/// # Errors
/// Fails when the engine or a library cannot be set up.
pub fn make_bare_engine() -> mlua::Result<Lua> {
    crate::host::install();
    let lua = Lua::new_with(
        StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT | StdLib::BUFFER,
        mlua::LuaOptions::new(),
    )?;
    strip_unsafe_globals(&lua)?;
    lock_string_metatable(&lua)?;
    Ok(lua)
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
