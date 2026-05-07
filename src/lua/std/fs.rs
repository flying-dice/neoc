//! Filesystem access — `lib.fs` (mirrors `std::fs`).
//!
//! `open` returns canonical `ReaderHandle` / `WriterHandle` wrappers so the
//! result plugs directly into `lib.io` operations and any future consumer
//! that accepts a stream handle.

use std::io::{BufReader, BufWriter};

use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::io::{ReaderHandle, WriterHandle};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("create_dir", lua.create_function(create_dir)?)?;
    t.set("create_dir_all", lua.create_function(create_dir_all)?)?;
    t.set("open", lua.create_function(open)?)?;
    t.set("read", lua.create_function(read)?)?;
    t.set("write", lua.create_function(write)?)?;
    Ok(t)
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
