//! Filesystem access: `std:fs` (mirrors `std::fs`).
//!
//! The surface follows `std::fs`: `read`, `write`, `open`, `create_dir`, `create_dir_all`, `metadata`,
//! `symlink_metadata`, `read_dir`, `copy`, `rename`, `remove_file`, `remove_dir`, `remove_dir_all`,
//! `canonicalize`, `read_link`, `symlink`, `set_permissions`, and `exists`; plus what Rust leaves to crates:
//! `walk` (a recursive listing), `copy_dir` (a recursive copy), `move` (a rename that survives a filesystem
//! boundary), `watch` (the changes inside a directory, as the operating system reports them), `error_kind` and
//! `error_message`. Paths are Lua strings; on Unix they are bytes (a name `read_dir` returns can always be passed
//! back), elsewhere they must be UTF-8 (and a name that is not valid Unicode comes back lossily; Windows was never
//! built or run). `std:path` has the text-only path helpers.
//!
//! Conventions, the same for every function:
//!
//! - **`(value, err)`.** A failure returns `nil, err`; `err` is one string, `fs.<function>: <kind>: <path>:
//!   <message>`, with a stable `kind` (see [`error::Kind`], read it with `error_kind`, and what follows it with
//!   `error_message`). An error of a call on two paths may name both, `<from> -> <to>`, in place of `<path>`; one
//!   about no path (a bad option) leaves the path part out, `fs.<function>: <kind>: <message>`. A wrong argument
//!   type is a Lua error, as everywhere in the VM (`open` is the exception for its options table: a missing or
//!   mistyped `path` or `mode` is `invalid_input`).
//! - **Options are tables of booleans or counts** (`missing_ok`, `max_depth`, `recursive`); an unknown name, or a value of the
//!   wrong type, is `invalid_input` rather than being ignored, so a typo cannot silently change what a call does.
//! - **Rust's behaviour where Rust has the function.** `copy` and `rename` replace an existing destination file
//!   (and `write` replaces its content). `copy_dir` refuses an existing destination instead of merging.
//!   Symlinks found inside a tree are not followed while walking, copying or removing it (the path a script names
//!   is: `walk` and `copy_dir` resolve a symlink given as their root, `remove_dir_all` refuses one); see [`ops`].
//! - **Calls that can run for long wait; the rest run in place.** `copy`, `copy_dir`, `move`, `read_dir`,
//!   `remove_dir_all`, `walk` and `watch` do their work on the blocking pool (a `Role::Wait`, like a network call):
//!   the call waits, and an embedder can run other calls meanwhile. Luau refuses a yield inside a metamethod, a
//!   `table.sort` comparator or a `string.gsub` callback, so these seven fail with the yield error when called from
//!   one of them, and do nothing: the job starts only once the call has really suspended
//!   (`tests::the_long_calls_fail_where_luau_refuses_a_yield`). `Watcher:receive` waits for the operating system
//!   the same way (and fails there too when it would have to wait). Every other function is one system call and
//!   runs in place, as `read` and `write` always did.
//! - **Not a sandbox.** Any path the host process can reach is reachable.
//!
//! Platform differences, the ones a script can meet (each function's doc says which it is subject to):
//!
//! - **Paths.** Unix paths are bytes and Windows paths are UTF-16: a name that is not UTF-8 works on Unix (and comes
//!   back from `read_dir` unchanged) and is `invalid_input` on Windows. A NUL byte is `invalid_input` everywhere.
//!   macOS (APFS) refuses to create a name that is not UTF-8 itself. Case sensitivity is the filesystem's: macOS and
//!   Windows are usually insensitive, Linux is not, and no function folds case.
//! - **Symlinks.** Creating one on Windows needs a privilege (`permission_denied` without it) and the kind of link,
//!   file or directory, is chosen from what the target is at that moment. `exists`, `metadata` and `canonicalize`
//!   follow links, `symlink_metadata`, `read_dir`, and what `walk`, `copy_dir` and the removals meet inside a tree, do not.
//! - **Permissions.** `mode()` and `set_permissions { mode }` are Unix bits and are `nil` / `unsupported` on Windows,
//!   where `readonly` is the read-only attribute. A removal of a read-only or open file can be `permission_denied`
//!   on Windows.
//! - **Times.** `created` is `nil` on filesystems with no birth time (some Linux ones), `accessed` is often not kept
//!   up to date.
//! - **Rename.** Atomic on one filesystem and `cross_device` across two (`move` copies then). Windows cannot rename
//!   a file that is open and will not put a directory over a directory.
//!
//! `open` returns canonical `ReaderHandle` / `WriterHandle` wrappers so the result plugs directly into `std:io`
//! operations and any future consumer that accepts a stream handle.
//!
//! [`ops`] (the operations, free of Lua) and [`error`] (their classified errors) are public, so an embedder that
//! offers files its own way builds on these operations instead of writing its own copy, move or remove.

pub mod error;
pub mod ops;
mod watch;

use std::ffi::OsStr;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::{IntoLua, Lua, Table, UserData, UserDataMethods, Value};

use crate::lua::error_text;
use crate::lua::facade::{DeclType, Module};
use crate::lua::std::io::{ReaderHandle, WriterHandle};
use crate::lua::std::osstr;

use error::{FsError, FsResult, Kind};
use ops::{Entry, Meta, Permission};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// What an option table may say, per function.
const MISSING_OK: &[&str] = &["missing_ok"];
const WALK_OPTIONS: &[&str] = &["max_depth"];
const PERMISSION_OPTIONS: &[&str] = &["mode", "readonly"];
const WATCH_OPTIONS: &[&str] = &["recursive"];

/// What every call that waits on the blocking pool says about it, in place of a pointer to these module notes, which
/// an editor's hover does not show.
const WAITS: &str = "It waits like `task.wait`: other tasks, and an embedder's other calls, run while it does. Where Luau cannot yield (a metamethod, a `table.sort` comparator, a `string.gsub` callback) it fails with the yield error and does nothing.";

/// A function's doc followed by [`WAITS`].
fn waits(doc: &str) -> String {
    format!("{doc} {WAITS}")
}

/// Declares the `std:fs` surface through the facade: every function is a fallible one (except `error_kind` and
/// `error_message`), so each is registered (with its `fs.<name>` tracing span and the `(nil, err)` shape) and
/// recorded for the definitions in one call.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:fs")?;

    declare_files(&mut m)?;
    declare_directories(&mut m)?;
    declare_tree_operations(&mut m)?;
    declare_links_and_errors(&mut m)?;
    declare_watch(&mut m)?;
    declare_types(&mut m);

    Ok(m)
}

/// A path argument as the script passed it.
type Path1 = mlua::LuaString;

#[neoc_macros::lua_module]
fn declare_files(m: &mut Module<'_>) -> mlua::Result<()> {
    /// Open a file given `{ path, mode }`. Mode `"r"` returns a reader handle; `"w"` (create or truncate) and `"a"` (create or append) return a writer handle; any other mode is `invalid_input`.
    #[lua]
    m.try_func("open", |_, opts: Table| open(&opts))?;
    /// Read the entire contents of `path` into a binary-safe string. Mirrors `std::fs::read`.
    #[lua(types(path = "string"))]
    m.try_func("read", |lua, path: Path1| {
        let path = path_of(&path)?;
        let bytes = std::fs::read(&path).map_err(|e| FsError::io(&e, &path))?;
        lua.create_string(&bytes).map_err(lua_failure)
    })?;
    /// Write `data` to `path`, replacing any existing content. Mirrors `std::fs::write`; binary-safe.
    #[lua(types(path = "string"))]
    m.try_func("write", |_, (path, data): (Path1, mlua::LuaString)| {
        let path = path_of(&path)?;
        std::fs::write(&path, data.as_bytes())
            .map(|()| true)
            .map_err(|e| FsError::io(&e, &path))
    })?;
    /// Whether `path` exists, following symlinks: `false` for a missing path and for a dangling symlink. Any other failure to check is an error with its kind, not `false`: `permission_denied`, `not_a_directory` when a parent is a file, `other` for a symlink loop. Use `symlink_metadata` to see a dangling link itself. Mirrors `std::fs::exists`.
    #[lua(types(path = "string"))]
    m.try_func("exists", |_, path: Path1| ops::exists(&path_of(&path)?))?;
    /// Metadata of `path`, following symlinks (a dangling symlink is `not_found`). See `Metadata` for what it says; timestamps are seconds since the Unix epoch, and `nil` where the platform or filesystem does not record them (`created` on some Linux filesystems). Mirrors `std::fs::metadata`.
    #[lua(types(path = "string"))]
    m.try_func("metadata", |_, path: Path1| {
        ops::metadata(&path_of(&path)?, true).map(MetadataHandle)
    })?;
    /// Like `metadata`, but about the symlink itself when `path` is one (`kind()` is then `"symlink"`). Mirrors `std::fs::symlink_metadata`.
    #[lua(types(path = "string"))]
    m.try_func("symlink_metadata", |_, path: Path1| {
        ops::metadata(&path_of(&path)?, false).map(MetadataHandle)
    })?;
    /// Change the permissions of `path` (through a symlink). Give exactly one of `mode`, the Unix permission bits (`0o755` is `493`, at most `4095`; `unsupported` on Windows), or `readonly`. `readonly = true` clears every write bit on Unix and sets the read-only attribute on Windows; `readonly = false` gives the owner (not group or others) write access on Unix. Mirrors `std::fs::set_permissions`.
    #[lua(types(path = "string", opts = "{ mode: number?, readonly: bool? }"))]
    m.try_func("set_permissions", |_, (path, opts): (Path1, Table)| {
        let path = path_of(&path)?;
        let options = read_options(Some(&opts), PERMISSION_OPTIONS)?;
        let mode = options.count("mode")?;
        let readonly = options.flag("readonly")?;
        let change = match (mode, readonly) {
            (Some(mode), None) => {
                Permission::Mode(u32::try_from(mode).map_err(|_| too_big("mode", mode))?)
            }
            (None, Some(on)) => Permission::Readonly(on),
            _ => {
                return Err(FsError::new(
                    Kind::InvalidInput,
                    "give exactly one of mode and readonly",
                ));
            }
        };
        ops::set_permissions(&path, change).map(|()| true)
    })?;
    Ok(())
}

#[neoc_macros::lua_module]
fn declare_directories(m: &mut Module<'_>) -> mlua::Result<()> {
    /// Create a single directory. Mirrors `std::fs::create_dir`: `already_exists` when the path exists, `not_found` when the parent is missing.
    #[lua(types(path = "string"))]
    m.try_func("create_dir", |_, path: Path1| {
        let path = path_of(&path)?;
        std::fs::create_dir(&path)
            .map(|()| true)
            .map_err(|e| FsError::io(&e, &path))
    })?;
    /// Create a directory and every missing ancestor. Mirrors `std::fs::create_dir_all`; succeeds if the directory already exists, and is `not_a_directory` when a part of the path is a file.
    #[lua(types(path = "string"))]
    m.try_func("create_dir_all", |_, path: Path1| {
        let path = path_of(&path)?;
        std::fs::create_dir_all(&path)
            .map(|()| true)
            .map_err(|e| FsError::io(&e, &path))
    })?;
    /// List the directory `path`: an array of `DirEntry`, sorted by name (Rust leaves the order unspecified), without `.` and `..`. A symlink is listed as `"symlink"`, not as what it points at, and is not entered. It is `not_found` for a missing path and `not_a_directory` for a file. The whole listing is read at once, and at most 1048576 entries (`too_large` past that). On Unix a name is the file's own bytes and can be passed back to any `std:fs` function unchanged; on Windows (never built or run) a name that is not valid Unicode comes back with replacement characters and may not name the same file. `walk` lists a whole tree; `lib:glob` matches patterns.
    #[lua(types(path = "string"), doc_with = waits)]
    m.try_async_func("read_dir", |_, path: Path1| {
        let path = path_of(&path);
        async move {
            let path = path?;
            blocking(move |_| ops::read_dir(&path))
                .await
                .map(DirEntries)
        }
    })?;
    /// List everything under the directory `path`, recursively: an array of `DirEntry` (the directory itself is not in it), depth first, the children of each directory sorted by name, so the order is the same on every platform. A symlink inside is listed and never entered, so a link loop cannot make a walk run for ever; `path` itself may be a symlink to a directory and is followed. Option `{ max_depth }` (a non-negative whole number) limits how far down it goes: `1` is `read_dir`. It is `not_found` for a missing path and `not_a_directory` for a file; an unreadable subdirectory fails the whole walk with `permission_denied` and its path. At most 1048576 entries (`too_large` past that: walk the subdirectories separately).
    #[lua(types(path = "string", opts = "{ max_depth: number? }?"), doc_with = waits)]
    m.try_async_func("walk", |_, (path, opts): (Path1, Option<Table>)| {
        let parsed = path_of(&path).and_then(|path| {
            let options = read_options(opts.as_ref(), WALK_OPTIONS)?;
            let depth = options.count("max_depth")?;
            Ok((
                path,
                depth.map(|d| usize::try_from(d).unwrap_or(usize::MAX)),
            ))
        });
        async move {
            let (path, depth) = parsed?;
            blocking(move |cancel| ops::walk(&path, depth, cancel))
                .await
                .map(DirEntries)
        }
    })?;
    Ok(())
}

#[neoc_macros::lua_module]
fn declare_tree_operations(m: &mut Module<'_>) -> mlua::Result<()> {
    /// Copy the file `from` to the path `to` and return the number of bytes written. Mirrors `std::fs::copy`: `to` is created or replaced and takes the permission bits of `from`; a symlink given as `from` is resolved, and so is one at `to`: the file it points at is written (the link stays a link), and a dangling one creates its target. `to` is the new path of the copy, not a directory to copy into (`is_directory`), `from` must be a file (`is_directory` for a directory: use `copy_dir`; `invalid_input` for a device or FIFO), as must an existing `to` (`invalid_input` for a FIFO or device: opening one for writing could block for ever), and copying a file onto itself is `invalid_input` rather than emptying it. Its parent must exist (`not_found`).
    #[lua(types(from = "string", to = "string"), doc_with = waits)]
    m.try_async_func("copy", |_, (from, to): (Path1, Path1)| {
        let paths = path_of(&from).and_then(|from| Ok((from, path_of(&to)?)));
        async move {
            let (from, to) = paths?;
            blocking(move |_| ops::copy_file(&from, &to)).await
        }
    })?;
    /// Copy the directory `from` and everything under it to the new path `to`, and return the bytes of file data copied. `to` must not exist (`already_exists`; nothing is merged) and its parent must; a destination inside the source is `invalid_input`. Files keep their permission bits, directories get the default ones, and symlinks are recreated as symlinks with the target as stored (a relative target stays relative), never followed, so a link loop cannot make it run for ever. A source that holds something that is not a file, directory or symlink (a device, FIFO, socket) is `invalid_input`. A failure part way removes the partial `to` again (best effort: if that removal fails too, the partial copy stays). A symlink given as `from` is resolved.
    #[lua(types(from = "string", to = "string"), doc_with = waits)]
    m.try_async_func("copy_dir", |_, (from, to): (Path1, Path1)| {
        let paths = path_of(&from).and_then(|from| Ok((from, path_of(&to)?)));
        async move {
            let (from, to) = paths?;
            blocking(move |cancel| ops::copy_dir(&from, &to, cancel)).await
        }
    })?;
    /// Rename or move `from` to `to` on the same filesystem, in one atomic step: the operating system's rename, as `std::fs::rename` calls it. An existing file or symlink at `to` is replaced; a directory replaces an empty directory only (`not_empty` otherwise); a file over a directory is `is_directory`, a directory over a file `not_a_directory`; a move across filesystems is `cross_device` (use `move`). A trailing separator or `.` on either path is dropped first, so a symlink is renamed as a link on every platform (macOS would otherwise follow `link/`). Platform differences: Windows refuses a directory over an existing directory (`permission_denied`) and cannot rename a file that is open.
    #[lua(types(from = "string", to = "string"))]
    m.try_func("rename", |_, (from, to): (Path1, Path1)| {
        ops::rename(&path_of(&from)?, &path_of(&to)?).map(|()| true)
    })?;
    /// Move `from` to `to`, with the rules of `rename`. When the two are on different filesystems it copies instead (a file, a symlink as a symlink, or a whole directory tree, never following a link) and removes `from` only after the copy completed. That case is not atomic: a failed copy leaves the source whole, a failure while removing a source tree leaves it partly removed and the destination complete, an existing empty directory at `to` is removed before the copy starts, and a file or symlink at `to` is replaced by renaming the finished copy over it (so a copy that fails leaves the old `to` as it was). A trailing separator on either path is dropped, so `link/` moves the link, not its target.
    #[lua(types(from = "string", to = "string"), doc_with = waits)]
    m.try_async_func("move", |_, (from, to): (Path1, Path1)| {
        let paths = path_of(&from).and_then(|from| Ok((from, path_of(&to)?)));
        async move {
            let (from, to) = paths?;
            blocking(move |cancel| ops::move_path(&from, &to, cancel))
                .await
                .map(|()| true)
        }
    })?;
    /// Remove a file or a symlink (the link, never its target; on Windows also a link to a directory). A directory is `is_directory`. Option `{ missing_ok }`: a missing path is then not an error and the result is `false` (otherwise `not_found`); a removal returns `true`. Mirrors `std::fs::remove_file`; on Windows a read-only file or one that is open may be `permission_denied`.
    #[lua(types(path = "string", opts = "{ missing_ok: bool? }?"))]
    m.try_func("remove_file", |_, (path, opts): (Path1, Option<Table>)| {
        let path = path_of(&path)?;
        let missing_ok = read_options(opts.as_ref(), MISSING_OK)?
            .flag("missing_ok")?
            .unwrap_or(false);
        ops::remove_file(&path, missing_ok)
    })?;
    /// Remove an empty directory (`not_empty` otherwise; `remove_dir_all` removes a tree). A symlink to a directory is `not_a_directory` (use `remove_file`), also when written `link/`; so is a file. Option `{ missing_ok }` as for `remove_file`. Like `std::fs::remove_dir`, but a symlink is `not_a_directory` on every platform.
    #[lua(types(path = "string", opts = "{ missing_ok: bool? }?"))]
    m.try_func("remove_dir", |_, (path, opts): (Path1, Option<Table>)| {
        let path = path_of(&path)?;
        let missing_ok = read_options(opts.as_ref(), MISSING_OK)?
            .flag("missing_ok")?
            .unwrap_or(false);
        ops::remove_dir(&path, missing_ok)
    })?;
    /// Remove a directory and everything under it, children before parents. Symlinks inside are deleted without being followed, so nothing outside the tree is touched. It refuses `/`, `.`, `..` and the empty path (`invalid_input`, a guard against a typo, not a security boundary) and a symlink to a directory (`not_a_directory`, also when written `link/`: use `remove_file`). Option `{ missing_ok }` as for `remove_file`. A failure part way (a permission, an open file on Windows) leaves what was not yet removed. Delegates to `std::fs::remove_dir_all` (which is safe against a directory swapped for a symlink while it runs, and not limited by path length) after refusing a symlink root and the paths above; once started it cannot be interrupted.
    #[lua(types(path = "string", opts = "{ missing_ok: bool? }?"), doc_with = waits)]
    m.try_async_func(
        "remove_dir_all",
        |_, (path, opts): (Path1, Option<Table>)| {
            let parsed = path_of(&path).and_then(|path| {
                let missing_ok = read_options(opts.as_ref(), MISSING_OK)?
                    .flag("missing_ok")?
                    .unwrap_or(false);
                Ok((path, missing_ok))
            });
            async move {
                let (path, missing_ok) = parsed?;
                blocking(move |cancel| ops::remove_dir_all(&path, missing_ok, cancel)).await
            }
        },
    )?;
    Ok(())
}

#[neoc_macros::lua_module]
fn declare_links_and_errors(m: &mut Module<'_>) -> mlua::Result<()> {
    /// The absolute path of `path` with every symlink and `.`/`..` resolved; the path must exist (`not_found`). On Windows the result has the extended-length `\\?\` prefix. Mirrors `std::fs::canonicalize`; `path.absolute` makes a path absolute without touching the disk.
    #[lua(types(path = "string"))]
    m.try_func("canonicalize", |lua, path: Path1| {
        let resolved = ops::canonicalize(&path_of(&path)?)?;
        os_to_lua(lua, resolved.as_os_str()).map_err(lua_failure)
    })?;
    /// The path the symlink `path` points to, as stored (possibly relative, possibly dangling). A path that is not a symlink is `invalid_input`. Mirrors `std::fs::read_link`.
    #[lua(types(path = "string"))]
    m.try_func("read_link", |lua, path: Path1| {
        let target = ops::read_link(&path_of(&path)?)?;
        os_to_lua(lua, target.as_os_str()).map_err(lua_failure)
    })?;
    /// Create the symlink `link` pointing at `target` (stored as given; it need not exist). `already_exists` when `link` exists. On Windows creating a symlink needs the privilege to do so (`permission_denied` otherwise), and the kind of link (file or directory) follows what `target` is when it is created.
    #[lua(types(target = "string", link = "string"))]
    m.try_func("symlink", |_, (target, link): (Path1, Path1)| {
        ops::symlink(&path_of(&target)?, &path_of(&link)?).map(|()| true)
    })?;
    /// The stable kind inside an error string returned by a `std:fs` function, or `nil` when the string is not one of theirs (or is `nil`, so `fs.error_kind(err)` takes the `err` of any call). Such a string is `fs.<function>: <kind>: <path>: <message>`; for a call on two paths (`copy`, `rename`, ...) the path part may be `<from> -> <to>`, and an error about no one path (a bad option, say) has none: `fs.<function>: <kind>: <message>`. Kinds: `not_found`, `permission_denied`, `already_exists`, `not_empty`, `is_directory`, `not_a_directory`, `cross_device`, `invalid_input`, `read_only_filesystem`, `storage_full`, `unsupported`, `too_large`, `timed_out`, `other`. They mean the same on every platform; the rest of the message is the operating system's wording and differs.
    #[lua(takes("err: string?"))]
    m.func("error_kind", error_text::reader(error::kind_of))?;
    /// The message inside an error string returned by a `std:fs` function: everything after the kind in `fs.<function>: <kind>: <message>`, or `nil` when the string is not one of theirs (or is `nil`, so `fs.error_message(err)` takes the `err` of any call), the same rule as `error_kind`. It is the path the error is about (when it is about one) and then the operating system's wording: `/x: No such file or directory (os error 2)`. It is for showing or logging as it is: branch on `error_kind`, not on these words, which differ between platforms and versions. Read it here instead of taking the error string apart.
    #[lua(takes("err: string?"))]
    m.func("error_message", error_text::reader(error::message_of))?;
    Ok(())
}

#[neoc_macros::lua_module]
fn declare_watch(m: &mut Module<'_>) -> mlua::Result<()> {
    /// Watch the directory `path` for changes and return a `Watcher`; read them with `watcher:receive()`. Option `{ recursive }` watches the whole tree under it (default: only its direct children). It is `not_found` for a missing path and `not_a_directory` for a file. The watch is the operating system's (inotify, FSEvents, ReadDirectoryChangesW), so the directory is not rescanned. Each event is `{ kind, path }`, `path` under the `path` given here. Kinds: `create`, `modify`, `remove`, `rename` (one event per path, the old name and the new name each; they are not paired, and macOS cannot tell which is which: check with `exists`), and `overflow`: changes were lost (more than 4096 waiting, or the system's own buffer overflowed), so rescan; its `path` is the watched directory. Events are not deduplicated or debounced, and one change can be reported as several (FSEvents coalesces: a new file may also show as `modify`). On macOS a change made just before the watch started may also be reported. The watch stops at `watcher:close()`, when the handle is collected, and when the engine shuts down. Starting a recursive watch on Linux adds one watch per directory of the tree.
    #[lua(types(path = "string", opts = "{ recursive: bool? }?"), doc_with = waits)]
    m.try_async_func("watch", |lua, (path, opts): (Path1, Option<Table>)| {
        let parsed = path_of(&path).and_then(|path| {
            let recursive = read_options(opts.as_ref(), WATCH_OPTIONS)?
                .flag("recursive")?
                .unwrap_or(false);
            Ok((path, recursive))
        });
        async move {
            let (path, recursive) = parsed?;
            let started = blocking(move |_| watch::start(path, recursive)).await?;
            Ok::<_, FsError>(started.register(&lua))
        }
    })?;
    Ok(())
}

/// Records the result types (the runtime registration is their `impl UserData`).
fn declare_types(m: &mut Module<'_>) {
    m.userdata("Metadata", |t| {
        MetadataHandle::lua(t);
    });
    m.userdata("Watcher", |t| {
        watch::WatchHandle::lua(t);
    });
    m.userdata("DirEntry", |t| {
        DirEntryHandle::lua(t);
    });
}

/// The parsed option table of one call: each name a script gave, with its value, checked against the names the
/// function takes.
struct Options(Vec<(String, Value)>);

impl Options {
    fn value(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// A boolean option, `None` when absent.
    fn flag(&self, name: &str) -> FsResult<Option<bool>> {
        match self.value(name) {
            None => Ok(None),
            Some(Value::Boolean(on)) => Ok(Some(*on)),
            Some(_) => Err(FsError::new(
                Kind::InvalidInput,
                format!("option \"{name}\" must be a boolean"),
            )),
        }
    }

    /// A count option (a non-negative whole number), `None` when absent. A huge count is fine, it is a bound.
    fn count(&self, name: &str) -> FsResult<Option<u64>> {
        let invalid = || {
            FsError::new(
                Kind::InvalidInput,
                format!("option \"{name}\" must be a non-negative whole number"),
            )
        };
        match self.value(name) {
            None => Ok(None),
            Some(Value::Integer(n)) => u64::try_from(*n).map(Some).map_err(|_| invalid()),
            Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => {
                // `as` saturates: a count past u64 is "as far as it goes"
                Ok(Some(*n as u64))
            }
            Some(_) => Err(invalid()),
        }
    }
}

/// Reads an options table: each key must be one of `allowed`. `None` is "no options".
fn read_options(opts: Option<&Table>, allowed: &[&str]) -> FsResult<Options> {
    let Some(opts) = opts else {
        return Ok(Options(Vec::new()));
    };
    let mut read = Vec::new();
    for pair in opts.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(lua_failure)?;
        let name = match &key {
            Value::String(s) => s.to_string_lossy(),
            _ => {
                return Err(FsError::new(
                    Kind::InvalidInput,
                    "option names must be strings",
                ));
            }
        };
        if !allowed.contains(&&*name) {
            return Err(FsError::new(
                Kind::InvalidInput,
                format!(
                    "unknown option \"{name}\" (this function takes: {})",
                    allowed.join(", ")
                ),
            ));
        }
        read.push((name.to_string(), value));
    }
    Ok(Options(read))
}

fn too_big(name: &str, n: u64) -> FsError {
    FsError::new(
        Kind::InvalidInput,
        format!("option \"{name}\" is out of range: {n}"),
    )
}

/// A script's path string as a path: a NUL byte is refused here, on every platform, with a message that says so
/// (the operating system would call it an invalid argument), and so is a string that is not UTF-8 where a path must be.
fn path_of(s: &mlua::LuaString) -> FsResult<PathBuf> {
    if s.as_bytes().contains(&0) {
        return Err(FsError::new(
            Kind::InvalidInput,
            "a path cannot contain a NUL byte",
        ));
    }
    osstr::to_path(s).ok_or_else(|| {
        FsError::new(
            Kind::InvalidInput,
            "a path must be valid UTF-8 on this platform",
        )
    })
}

fn os_to_lua(lua: &Lua, s: &OsStr) -> mlua::Result<mlua::LuaString> {
    osstr::to_lua(lua, s)
}

/// A failure of the Lua side (an allocation, a hostile `__pairs`) as an `other` error.
fn lua_failure(e: mlua::Error) -> FsError {
    FsError::new(Kind::Other, e.to_string())
}

/// Runs `job` on the blocking pool and waits for it. If the call is dropped while the job runs (it was abandoned, or
/// its script shut down) the job's cancel flag is set and a loop over a tree stops at its next entry.
async fn blocking<T, F>(job: F) -> FsResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AtomicBool) -> FsResult<T> + Send + 'static,
{
    struct CancelOnDrop(Option<Arc<AtomicBool>>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            if let Some(flag) = &self.0 {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
    // Do nothing on the first poll. mlua polls a host call once before it tries to yield, and where Luau refuses
    // the yield (a `gsub` callback, a `table.sort` comparator, a metamethod) it drops the call after that poll: a
    // job started now would run to the end although the script was told the call failed. Returning `Pending` first
    // means a call that cannot suspend never starts its job.
    tokio::task::yield_now().await;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut guard = CancelOnDrop(Some(Arc::clone(&cancel)));
    let joined = tokio::task::spawn_blocking(move || job(&cancel)).await;
    guard.0 = None;
    joined.unwrap_or_else(|e| {
        Err(FsError::new(
            Kind::Other,
            format!("the filesystem job failed: {e}"),
        ))
    })
}

/// The handle `open` hands back: a reader for `"r"`, a writer for `"w"`/`"a"`.
enum OpenHandle {
    Reader(ReaderHandle),
    Writer(WriterHandle),
}

impl OpenHandle {
    fn reader(f: std::fs::File) -> Self {
        OpenHandle::Reader(ReaderHandle::new(BufReader::new(f)))
    }
    fn writer(f: std::fs::File) -> Self {
        OpenHandle::Writer(WriterHandle::new(BufWriter::new(f)))
    }
}

impl IntoLua for OpenHandle {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        match self {
            OpenHandle::Reader(r) => r.into_lua(lua),
            OpenHandle::Writer(w) => w.into_lua(lua),
        }
    }
}

impl DeclType for OpenHandle {
    fn decl_type() -> String {
        "LuaValue".to_owned()
    }
}

fn open(opts: &Table) -> FsResult<OpenHandle> {
    let text = |key: &str| {
        opts.get::<mlua::LuaString>(key)
            .map_err(|_| FsError::new(Kind::InvalidInput, format!("opts.{key} must be a string")))
    };
    let path = path_of(&text("path")?)?;
    let mode = text("mode")?.to_string_lossy();
    let opened = match &*mode {
        "r" => std::fs::File::open(&path).map(OpenHandle::reader),
        "w" => std::fs::File::create(&path).map(OpenHandle::writer),
        "a" => std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map(OpenHandle::writer),
        other => {
            return Err(FsError::new(
                Kind::InvalidInput,
                format!("unknown mode \"{other}\" (use \"r\", \"w\" or \"a\")"),
            ));
        }
    };
    opened.map_err(|e| FsError::io(&e, &path))
}

/// The result of `metadata` / `symlink_metadata`.
struct MetadataHandle(Meta);

#[neoc_macros::lua_userdata("std:fs", "Metadata")]
impl UserData for MetadataHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// `"file"`, `"dir"`, `"symlink"` (only from `symlink_metadata`) or `"other"`.
        methods.add_method("kind", |_, this, ()| Ok(this.0.kind.as_str()));
        /// `kind() == "file"`.
        methods.add_method("is_file", |_, this, ()| {
            Ok(this.0.kind == ops::FileKind::File)
        });
        /// `kind() == "dir"`.
        #[lua("bool")]
        methods.add_method(
            "is_dir",
            |_, this, ()| Ok(this.0.kind == ops::FileKind::Dir),
        );
        /// `kind() == "symlink"`.
        methods.add_method("is_symlink", |_, this, ()| {
            Ok(this.0.kind == ops::FileKind::Symlink)
        });
        /// Size in bytes (for a directory, whatever the platform reports).
        methods.add_method("len", |_, this, ()| Ok(this.0.size));
        /// Whether the permissions forbid writing: on Unix, no write bit at all; on Windows, the read-only attribute.
        methods.add_method("readonly", |_, this, ()| Ok(this.0.readonly));
        /// Last modification, seconds since the Unix epoch; `nil` if unavailable.
        methods.add_method("modified", |_, this, ()| Ok(this.0.modified));
        /// Creation (birth) time, seconds since the Unix epoch; `nil` where the platform or filesystem does not record it.
        methods.add_method("created", |_, this, ()| Ok(this.0.created));
        /// Last access, seconds since the Unix epoch; `nil` if unavailable (and unreliable: many filesystems do not update it).
        methods.add_method("accessed", |_, this, ()| Ok(this.0.accessed));
        /// The Unix permission bits (`0o644` is `420`); `nil` on Windows.
        methods.add_method("mode", |_, this, ()| Ok(this.0.mode));
    }
}

/// One entry of a `read_dir` or `walk` listing.
struct DirEntryHandle(Entry);

#[neoc_macros::lua_userdata("std:fs", "DirEntry")]
impl UserData for DirEntryHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// The entry's own name, as the filesystem has it (bytes on Unix, so it may not be UTF-8).
        methods.add_method("file_name", |lua, this, ()| os_to_lua(lua, &this.0.name));
        /// The directory given to `read_dir` or `walk` joined with the entry's path under it.
        methods.add_method("path", |lua, this, ()| {
            os_to_lua(lua, this.0.path.as_os_str())
        });
        /// `"file"`, `"dir"`, `"symlink"` or `"other"`; a symlink is not resolved.
        methods.add_method("kind", |_, this, ()| Ok(this.0.kind.as_str()));
        /// `kind() == "file"`.
        methods.add_method("is_file", |_, this, ()| {
            Ok(this.0.kind == ops::FileKind::File)
        });
        /// `kind() == "dir"`.
        #[lua("bool")]
        methods.add_method(
            "is_dir",
            |_, this, ()| Ok(this.0.kind == ops::FileKind::Dir),
        );
        /// `kind() == "symlink"`.
        methods.add_method("is_symlink", |_, this, ()| {
            Ok(this.0.kind == ops::FileKind::Symlink)
        });
        /// How far below the listed directory the entry is: `1` for a direct child (always, for `read_dir`).
        methods.add_method("depth", |_, this, ()| Ok(this.0.depth));
    }
}

/// A listing: a Lua array of entries.
struct DirEntries(Vec<Entry>);

impl IntoLua for DirEntries {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let array = lua.create_table_with_capacity(self.0.len(), 0)?;
        for (i, entry) in self.0.into_iter().enumerate() {
            array.raw_set(i + 1, DirEntryHandle(entry))?;
        }
        Ok(Value::Table(array))
    }
}

impl DeclType for DirEntries {
    fn decl_type() -> String {
        "{ DirEntry }".to_owned()
    }
}

#[cfg(test)]
mod tests;
