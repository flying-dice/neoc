//! The engine factory: a Luau state with the curated standard library, the runtime's state and a `require`
//! over the modules.

use std::sync::Arc;

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

/// [`make_engine`] whose spawned work (tasks, workers) registers with `life`: an embedder that unloads the script
/// calls [`Life::shutdown`], and a `std:thread` worker joins the registry of the engine that started it.
///
/// # Errors
/// Fails when the engine or a module cannot be set up.
pub fn make_engine_in(life: Life) -> mlua::Result<Lua> {
    let lua = make_bare_engine()?;
    install_runtime_state(&lua, life);
    modules::install(&lua)?;
    // a worker of this script is a full engine like it, joining the registry it is given
    set_worker_engine(&lua, Arc::new(make_engine_in));
    Ok(lua)
}

/// The engine of a worker script offered only the `allowed` modules: what `std:thread` builds for a worker whose
/// parent used [`modules::install_allowed`].
pub(crate) fn make_engine_with(life: Life, allowed: &[&str]) -> mlua::Result<Lua> {
    let lua = make_bare_engine()?;
    install_runtime_state(&lua, life);
    modules::install_allowed(&lua, allowed)?;
    Ok(lua)
}

/// How an embedder builds the engine of a `std:thread` worker its script starts: given the script's [`Life`] (the
/// worker's tasks and workers join it), the engine, offered what the embedder offers its scripts.
pub type WorkerEngineFn = Arc<dyn Fn(Life) -> mlua::Result<Lua> + Send + Sync>;

struct WorkerEngine(WorkerEngineFn);

/// Install the [`WorkerEngineFn`] of this engine's scripts. [`make_engine_in`] installs one that builds a full
/// engine and [`modules::install_allowed`] one that builds the same subset. A bare engine ([`make_bare_engine`])
/// has none, and `std:thread` refuses to start a worker on it until this is called: a worker is never offered more
/// than its parent without the embedder saying so. An embedder that replaces the `require` of a [`make_engine_in`]
/// engine with a narrower one must call this too.
pub fn set_worker_engine(lua: &Lua, factory: WorkerEngineFn) {
    lua.set_app_data(WorkerEngine(factory));
}

/// How `std:thread` builds a worker's engine for the script that runs in `lua`, or `None` when the engine has no
/// [`WorkerEngineFn`] (a bare engine whose embedder did not say how).
#[must_use]
pub fn worker_engine(lua: &Lua) -> Option<WorkerEngineFn> {
    lua.app_data_ref::<WorkerEngine>().map(|factory| factory.0.clone())
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
/// each script in its own [environment](crate::script_env). A script on it cannot start a `std:thread` worker
/// until the embedder says how to build one ([`set_worker_engine`], which [`modules::install_allowed`] calls). A
/// worker runs on its engine's globals, not in a script environment: an embedder that keeps a library from its
/// scripts removes it there too, in every worker engine it builds.
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
    install_assert(&lua)?;
    Ok(lua)
}

/// The global `assert`, as Lua 5.x has it: a falsy first argument raises the second **as it is** (any value;
/// `"assertion failed!"` when there is none), anything else returns every argument. Luau's own refuses a message
/// that is not a string, which loses the typed error of `assert(fs.read(path))`: with this one a `pcall` around it
/// catches the error table, `kind` and fields intact.
const ASSERT: &str = r##"
local select, error = select, error
return function(...)
    if select("#", ...) == 0 then
        error("missing argument #1 to 'assert' (value expected)", 2)
    end
    local value, message = ...
    if value then
        return ...
    end
    if message == nil then
        message = "assertion failed!"
    end
    error(message, 0)
end
"##;

fn install_assert(lua: &Lua) -> mlua::Result<()> {
    let assert: mlua::Function = lua.load(ASSERT).set_name("=assert").eval()?;
    lua.globals().raw_set("assert", assert)
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

    /// A worker is offered what its parent was: everything for a full engine, the same subset after
    /// `install_allowed`, and nothing at all (no worker) on a bare engine.
    #[test]
    fn a_worker_engine_offers_what_its_parent_was_offered() {
        let require =
            |lua: &Lua, name: &str| lua.load(format!("return require({name:?})")).eval::<mlua::Table>();

        let full = make_engine().expect("engine");
        let worker =
            worker_engine(&full).expect("a full engine has a worker engine")(Life::new()).expect("worker");
        assert!(require(&worker, "std:time").is_ok() && require(&worker, "lib:base64").is_ok());
        assert!(worker_engine(&worker).is_some(), "a worker can start workers of its own");

        let narrow = make_bare_engine().expect("engine");
        assert!(worker_engine(&narrow).is_none(), "a bare engine has no worker engine");
        modules::install_allowed(&narrow, &["lib:base64"]).expect("install");
        let worker = worker_engine(&narrow).expect("install_allowed sets one")(Life::new()).expect("worker");
        assert!(require(&worker, "lib:base64").is_ok());
        assert!(require(&worker, "std:time").is_err(), "a worker is not offered more than its parent");
        let grandchild = worker_engine(&worker).expect("and so on down")(Life::new()).expect("worker");
        assert!(require(&grandchild, "std:time").is_err());
    }

    /// The registry a worker engine is given is the one its work joins.
    #[test]
    fn a_worker_engine_joins_the_life_it_is_given() {
        let life = Life::new();
        let full = make_engine().expect("engine");
        let worker = worker_engine(&full).expect("worker engine")(life.clone()).expect("worker");
        life.shutdown();
        assert!(Life::of(&worker).is_closed());
        assert!(!Life::of(&full).is_closed());
    }

    /// `assert` keeps the typed error of a failed call: the table itself is raised, and `pcall` hands it back.
    #[test]
    fn assert_raises_a_typed_error_as_it_is() {
        let lua = make_engine().expect("engine");
        let (ok, kind, is_table, op): (bool, String, bool, String) = lua
            .load(
                r#"
                local fs = require("std:fs")
                local ok, err = pcall(function() return assert(fs.read("no/such/file/anywhere")) end)
                return ok, err.kind, type(err) == "table", err.op
                "#,
            )
            .eval()
            .expect("script");
        assert!(!ok && is_table);
        assert_eq!((kind.as_str(), op.as_str()), ("not_found", "std:fs.read"));
    }

    #[test]
    fn assert_returns_every_argument_and_keeps_string_messages() {
        let lua = make_engine().expect("engine");
        let (a, b, c, count): (i64, String, bool, i64) = lua
            .load(
                r##"local a, b, c = assert(1, "two", false) return a, b, c, select("#", assert(1, nil, 3))"##,
            )
            .eval()
            .expect("script");
        assert_eq!((a, b.as_str(), c, count), (1, "two", false, 3));

        let (message, default, falsy, none): (String, String, String, bool) = lua
            .load(
                r#"
                local _, message = pcall(assert, false, "it broke")
                local _, default = pcall(assert, nil)
                local _, falsy = pcall(assert, false, nil, "ignored")
                local none = pcall(assert)
                return message, default, falsy, none
                "#,
            )
            .eval()
            .expect("script");
        assert_eq!(message, "it broke", "a string message is raised without a position");
        assert_eq!(default, "assertion failed!");
        assert_eq!(falsy, "assertion failed!");
        assert!(!none, "assert() with no value is refused");
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
