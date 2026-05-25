//! Filesystem access — `lib.fs` (mirrors `std::fs`).
//!
//! `open` returns canonical `ReaderHandle` / `WriterHandle` wrappers so the
//! result plugs directly into `lib.io` operations and any future consumer
//! that accepts a stream handle.

use std::io::{BufReader, BufWriter};

use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:fs` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]:
/// each already carries its own `#[instrument(name = "fs.<fn>", target =
/// "luau::lib::fs")]` attribute, so routing through the facade's `try_func`
/// would add a second span and break byte-identical tracing. `open` returns a
/// `ReaderHandle` or `WriterHandle` whose type depends on the mode, so it has
/// no single facade return type. Each function is recorded for the header via
/// [`crate::lua::facade::Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:fs")?;

    m.set("create_dir", lua.create_function(create_dir)?)?;
    m.record_fn(
        "create_dir",
        &[("path", "string")],
        "Result<bool, string>",
        "Create a single directory. Mirrors `std::fs::create_dir` — errors if the path already exists or the parent is missing.",
    );

    m.set("create_dir_all", lua.create_function(create_dir_all)?)?;
    m.record_fn(
        "create_dir_all",
        &[("path", "string")],
        "Result<bool, string>",
        "Create a directory and every missing ancestor. Mirrors `std::fs::create_dir_all`; succeeds if the directory already exists.",
    );

    m.set("open", lua.create_function(open)?)?;
    m.record_fn(
        "open",
        &[("opts", "LuaValue")],
        "Result<LuaValue, string>",
        "Open a file given `{ path, mode }`. Mode `\"r\"` returns a reader handle; `\"w\"` / `\"a\"` return a writer handle.",
    );

    m.set("read", lua.create_function(read)?)?;
    m.record_fn(
        "read",
        &[("path", "string")],
        "Result<string, string>",
        "Read the entire contents of `path` into a binary-safe string. Mirrors `std::fs::read`.",
    );

    m.set("write", lua.create_function(write)?)?;
    m.record_fn(
        "write",
        &[("path", "string"), ("data", "string")],
        "Result<bool, string>",
        "Write `data` to `path`, replacing any existing content. Mirrors `std::fs::write`; binary-safe.",
    );

    Ok(m)
}

/// Creates a single directory. Mirrors `std::fs::create_dir` — errors if the
/// path already exists or the parent does not exist.
#[instrument(name = "fs.create_dir", target = "luau::lib::fs", skip_all)]
fn create_dir(lua: &Lua, path: String) -> mlua::Result<(Value, Value)> {
    match std::fs::create_dir(&path) {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "fs.create_dir", e),
    }
}

/// Creates a directory and all missing parent directories. Mirrors
/// `std::fs::create_dir_all` — recursive creation. Returns `(true, nil)` on
/// success or if the directory already exists; `(nil, err)` otherwise.
#[instrument(name = "fs.create_dir_all", target = "luau::lib::fs", skip_all)]
fn create_dir_all(lua: &Lua, path: String) -> mlua::Result<(Value, Value)> {
    match std::fs::create_dir_all(&path) {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "fs.create_dir_all", e),
    }
}

// TODO: clean-code - 0.65 - DRY: three open-arms repeat the Ok→handle / Err→from_err shape; collapse to one match producing a Result<Handle> then a single ok/from_err tail
#[instrument(name = "fs.open", target = "luau::lib::fs", skip_all)]
fn open(lua: &Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let path: String = opts.get("path")?;
    let mode: String = opts.get("mode")?;
    match mode.as_str() {
        "r" => match std::fs::File::open(&path) {
            Ok(f) => ok(lua, ReaderHandle::new(BufReader::new(f))),
            Err(e) => from_err(lua, "fs.open", e),
        },
        "w" => match std::fs::File::create(&path) {
            Ok(f) => ok(lua, WriterHandle::new(BufWriter::new(f))),
            Err(e) => from_err(lua, "fs.open", e),
        },
        "a" => match std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
        {
            Ok(f) => ok(lua, WriterHandle::new(BufWriter::new(f))),
            Err(e) => from_err(lua, "fs.open", e),
        },
        // TODO: clean-code - 0.55 - DRY: "fs.open" literal repeated 4× in this function and also as instrument name — extract const ERR_CTX: &str = "fs.open"
        other => from_err(lua, "fs.open", format!("unknown mode \"{other}\"")),
    }
}

#[instrument(name = "fs.read", target = "luau::lib::fs", skip_all)]
fn read(lua: &Lua, path: String) -> mlua::Result<(Value, Value)> {
    match std::fs::read(&path) {
        Ok(bytes) => ok(lua, lua.create_string(&bytes)?),
        Err(e) => from_err(lua, "fs.read", e),
    }
}

#[instrument(name = "fs.write", target = "luau::lib::fs", skip_all)]
fn write(lua: &Lua, (path, data): (String, mlua::String)) -> mlua::Result<(Value, Value)> {
    match std::fs::write(&path, data.as_bytes()) {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, "fs.write", e),
    }
}
