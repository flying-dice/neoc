//! `app.files`: the app's managed files, found from the app's identity, one shared set per app (not per person
//! or device).
//!
//! ```lua
//! local docs = app.files.documents                 -- also app.files.cache and app.files.temp
//! local ok, err = docs:createDir("notes")          -- true | nil, err (and the missing directories above it)
//! ok, err = docs:write("notes/a.txt", "hello")     -- replaces the file in one step; the parent must exist
//! local text, err = docs:read("notes/a.txt")       -- the whole file, binary-safe
//! local entries = docs:list("notes")               -- { { name, kind, size, modified } }, sorted by name
//! local info = docs:info("notes/a.txt")            -- { name = "a.txt", kind = "file", size = 5, modified = ... }
//! docs:move("notes/a.txt", "notes/b.txt")          -- replaces an existing file at the destination
//! docs:copy("notes", "backup")                     -- a file, or a whole tree to a new path
//! docs:remove("backup", { recursive = true })
//! if app.files.errorKind(err) == "not_found" then ... end
//! ```
//!
//! A path is relative to its location, written with `/`; the rules are the use case's (`vos_application::Files`)
//! and the adapter's (`crate::files`, which says how links and races are handled). A failure returns
//! `nil, "files.<method>: <kind>: <path>: <message>"` with the kinds of `std:fs` (`errorKind` reads it); a wrong
//! argument type, an unknown option, and a write inside a `readOnly` tool or a render raise, as everywhere in
//! `app.*`. A file call holds the app's turn while it runs (`Role::Vm`, like `app.db`): the app's other calls wait
//! for it, and none of them sees it half done. It does not need `std:fs`: the host does the work.

use super::{data::reject_unknown, vm_function, Sh};
use neoc_vm::mlua::{self, Lua, LuaString, Table, Value as LuaValue};
use std::future::Future;
use vos_application::Files;
use vos_domain::{FileError, FileErrorKind, FileInfo, FileResult, Location};

/// `(value, nil)` or `(nil, err)`.
type Outcome<T> = (Option<T>, Option<String>);

pub(super) fn install(lua: &Lua, sh: &Sh, app: &Table) -> mlua::Result<()> {
    let files = lua.create_table()?;
    for at in Location::ALL {
        files.set(at.as_str(), location(lua, sh, at)?)?;
    }
    files.set("errorKind", lua.create_function(|_, err: LuaValue| Ok(error_kind(&err)))?)?;
    app.set("files", files)
}

/// The methods of one location's handle (`app.files.documents`).
fn location(lua: &Lua, sh: &Sh, at: Location) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(
        "read",
        vm_function(lua, sh, move |s, lua, (_this, path): (LuaValue, LuaString)| {
            let path = text(&path);
            async move {
                let out = call(&s, "read", false, |f| async move { f.read(at, &path?).await }).await?;
                answer(out, |bytes| lua.create_string(bytes))
            }
        })?,
    )?;
    t.set(
        "write",
        vm_function(lua, sh, move |s, _, (_this, path, data): (LuaValue, LuaString, LuaString)| {
            let (path, data) = (text(&path), data.as_bytes().to_vec());
            async move {
                let out = call(&s, "write", true, |f| async move { f.write(at, &path?, data).await }).await?;
                answer(out, |()| Ok(true))
            }
        })?,
    )?;
    t.set(
        "list",
        vm_function(lua, sh, move |s, lua, (_this, path): (LuaValue, Option<LuaString>)| {
            let path = path.as_ref().map_or_else(|| Ok(String::new()), text);
            async move {
                let out = call(&s, "list", false, |f| async move { f.list(at, &path?).await }).await?;
                answer(out, |entries| {
                    let list = lua.create_table()?;
                    for entry in &entries {
                        list.raw_push(info_table(&lua, entry)?)?;
                    }
                    Ok(list)
                })
            }
        })?,
    )?;
    t.set(
        "info",
        vm_function(lua, sh, move |s, lua, (_this, path): (LuaValue, LuaString)| {
            let path = text(&path);
            async move {
                let out = call(&s, "info", false, |f| async move { f.info(at, &path?).await }).await?;
                answer(out, |info| info_table(&lua, &info))
            }
        })?,
    )?;
    t.set(
        "createDir",
        vm_function(lua, sh, move |s, _, (_this, path): (LuaValue, LuaString)| {
            let path = text(&path);
            async move {
                let out =
                    call(&s, "createDir", true, |f| async move { f.create_dir(at, &path?).await }).await?;
                answer(out, |()| Ok(true))
            }
        })?,
    )?;
    t.set(
        "remove",
        vm_function(lua, sh, move |s, _, (_this, path, opts): (LuaValue, LuaString, Option<Table>)| {
            let (path, recursive) = (text(&path), recursive(opts.as_ref()));
            async move {
                let recursive = recursive?;
                let out = call(&s, "remove", true, |f| async move { f.remove(at, &path?, recursive).await })
                    .await?;
                answer(out, |()| Ok(true))
            }
        })?,
    )?;
    t.set(
        "move",
        vm_function(lua, sh, move |s, _, (_this, from, to): (LuaValue, LuaString, LuaString)| {
            let (from, to) = (text(&from), text(&to));
            async move {
                let out =
                    call(&s, "move", true, |f| async move { f.move_to(at, &from?, &to?).await }).await?;
                answer(out, |()| Ok(true))
            }
        })?,
    )?;
    t.set(
        "copy",
        vm_function(lua, sh, move |s, _, (_this, from, to): (LuaValue, LuaString, LuaString)| {
            let (from, to) = (text(&from), text(&to));
            async move {
                let out = call(&s, "copy", true, |f| async move { f.copy(at, &from?, &to?).await }).await?;
                answer(out, |()| Ok(true))
            }
        })?,
    )?;
    Ok(t)
}

/// One file call, holding the turn (`Role::Vm`). A write is refused first inside a `readOnly` call or a render
/// (it raises, like `app.kv:set`); the files are the read-only handle inside a `readOnly` call either way.
async fn call<T, Fut>(
    s: &Sh,
    method: &str,
    write: bool,
    op: impl FnOnce(Files) -> Fut,
) -> mlua::Result<Result<T, String>>
where
    Fut: Future<Output = FileResult<T>>,
{
    if write {
        s.require_write(&format!("app.files:{method}"))?;
    }
    let files = s.files();
    // Nothing on the first poll. mlua polls a host call once before it tries to yield, and where Luau refuses the
    // yield (a `string.gsub` callback, a `table.sort` comparator, a metamethod) it drops the call after that poll:
    // a job started now would go on although the script was told the call failed (`std:fs` does the same).
    tokio::task::yield_now().await;
    let done = tokio::select! {
        done = op(files) => done,
        () = s.life.closed() => return Err(mlua::Error::runtime("the app was unloaded")),
    };
    Ok(done.map_err(|e| format!("files.{method}: {e}")))
}

/// `(value, nil)` or `(nil, err)`, building the value only on success.
fn answer<T, V>(
    out: Result<T, String>,
    value: impl FnOnce(T) -> mlua::Result<V>,
) -> mlua::Result<Outcome<V>> {
    match out {
        Ok(v) => Ok((Some(value(v)?), None)),
        Err(e) => Ok((None, Some(e))),
    }
}

/// A path as text: it must be UTF-8 (a name is the same string on every platform).
fn text(path: &LuaString) -> FileResult<String> {
    path.to_str()
        .map(|s| s.to_owned())
        .map_err(|_| FileError::new(FileErrorKind::InvalidInput, "a path must be UTF-8 text"))
}

/// `remove`'s options: `{ recursive = true }`. An unknown option or a wrong type raises.
fn recursive(opts: Option<&Table>) -> mlua::Result<bool> {
    let Some(opts) = opts else { return Ok(false) };
    reject_unknown(opts, &["recursive"], "app.files:remove")?;
    match opts.get::<LuaValue>("recursive")? {
        LuaValue::Nil => Ok(false),
        LuaValue::Boolean(b) => Ok(b),
        other => Err(mlua::Error::runtime(format!(
            "app.files:remove: option `recursive` must be a boolean, got {}",
            other.type_name()
        ))),
    }
}

fn info_table(lua: &Lua, info: &FileInfo) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", info.name.as_str())?;
    t.set("kind", info.kind.as_str())?;
    t.set("size", info.size)?;
    t.set("modified", info.modified)?;
    Ok(t)
}

/// The kind in an error a file call returned (`files.read: not_found: ...`), or `nil` for anything else.
fn error_kind(err: &LuaValue) -> Option<&'static str> {
    let LuaValue::String(err) = err else { return None };
    let err = err.to_str().ok()?;
    let (_method, rest) = err.strip_prefix("files.")?.split_once(": ")?;
    let kind = rest.split_once(": ").map_or(rest, |(kind, _)| kind);
    FileErrorKind::parse(kind).map(FileErrorKind::as_str)
}
