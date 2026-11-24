//! Filesystem access: `std:fs` (mirrors `std::fs`).
//!
//! The surface follows `std::fs`: `read`, `write`, `open`, `create`, `create_dir`, `create_dir_all`, `metadata`,
//! `symlink_metadata`, `read_dir`, `copy`, `rename`, `remove_file`, `remove_dir`, `remove_dir_all`, `canonicalize`,
//! `read_link`, `symlink` and `exists`; plus what Rust leaves to crates or spells differently: `append` (a file
//! opened to append), `set_mode` and `set_readonly` (the two things `set_permissions` does), `walk` (a recursive
//! listing), `copy_dir` (a recursive copy), `move` (a rename that survives a filesystem boundary) and `watch`
//! (the changes inside a directory, as the operating system reports them). `std:path` has the text-only helpers.
//!
//! Conventions, the same for every function:
//!
//! - **`(value, err)`.** A failure returns `nil, err`; `err` is a typed [`error::FsError`] with a stable `kind`,
//!   the `path` it is about (and `to` for a failure about two paths) and the operating system's `reason`. A wrong
//!   argument (a type, an option that does not exist) is raised with its place, as everywhere in the VM.
//! - **Rust's behaviour where Rust has the function.** `copy` and `rename` replace an existing destination file
//!   (and `write` replaces its content). `copy_dir` refuses an existing destination instead of merging.
//!   Symlinks found inside a tree are not followed while walking, copying or removing it (the path a script names
//!   is: `walk` and `copy_dir` resolve a symlink given as their root, `remove_dir_all` refuses one); see [`ops`].
//! - **Calls that can run for long wait; the rest run in place.** `copy`, `copy_dir`, `move`, `read_dir`,
//!   `remove_dir_all`, `walk` and `watch` do their work on the blocking pool: the call waits, and other calls run
//!   meanwhile. Luau refuses a yield inside a metamethod, a `table.sort` comparator or a `string.gsub` callback, so
//!   these seven fail with the yield error when called from one of them, and do nothing: the job starts only once
//!   the call has really suspended. `Watcher:receive` waits for the operating system the same way. Every other
//!   function is one system call and runs in place.
//! - **Not a sandbox.** Any path the host process can reach is reachable.
//!
//! Platform differences, the ones a script can meet:
//!
//! - **Paths.** Unix paths are bytes and Windows paths are UTF-16: a name that is not UTF-8 works on Unix (and comes
//!   back from `read_dir` unchanged) and is refused as an argument on Windows. A NUL byte is `invalid_input`
//!   everywhere. Case sensitivity is the filesystem's, and no function folds case.
//! - **Symlinks.** Creating one on Windows needs a privilege (`permission_denied` without it) and the kind of link,
//!   file or directory, is chosen from what the target is at that moment. `exists`, `metadata` and `canonicalize`
//!   follow links; `symlink_metadata`, `read_dir`, and what `walk`, `copy_dir` and the removals meet inside a tree,
//!   do not.
//! - **Permissions.** `Metadata.mode` and `set_mode` are Unix bits and are absent / `unsupported` on Windows, where
//!   `readonly` is the read-only attribute.
//! - **Rename.** Atomic on one filesystem and `cross_device` across two (`move` copies then). Windows cannot
//!   rename a file that is open and will not put a directory over a directory.
//!
//! [`ops`] (the operations, free of the Lua state) and [`error`] (their classified errors) are public, so an
//! embedder that offers files its own way builds on these operations instead of writing its own copy or remove.
//!
//! The API's rules:
//! - An error is a typed `FsError` (`err.kind`, `err.path`, `err.to`, `err.reason`).
//! - `open(path)` opens for reading; `create(path)` and `append(path)` open for writing.
//! - `Metadata` and `DirEntry` are plain records (`meta.len`, `entry.file_name`); what an entry is is its `kind` (`kind
//!   == "file"`).
//! - Permissions are set with `set_mode(path, mode)` and `set_readonly(path, readonly)`.
//! - A move is `move_path` (see the TODO below).
//! - `Watcher:receive` takes a `std:time` `Duration` as its timeout.
//! - A wrong option (an unknown name, a value of the wrong type) is raised as a wrong argument, with its place;
//!   `max_depth` is a whole number up to 4294967295.
pub mod error;
pub mod ops;
mod watch;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use error::{FsError, FsResult, Kind};

use neoc_vm_core::osstr::OsPath;

/// A script's path as a path: a NUL byte is refused here, on every platform, with a message that says so (the
/// operating system would call it an invalid argument).
fn path_of(path: OsPath) -> FsResult<PathBuf> {
    if path.0.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(FsError::new(Kind::InvalidInput, "a path cannot contain a NUL byte"));
    }
    Ok(path.0)
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
    joined.unwrap_or_else(|e| Err(FsError::new(Kind::Other, format!("the filesystem job failed: {e}"))))
}

/// Files and directories. A failure is `nil` and an `FsError` whose `kind` means the same on every platform.
/// `copy`, `copy_dir`, `move`, `read_dir`, `remove_dir_all`, `walk`, `watch` and `Watcher:receive` wait
/// without blocking other calls; where Luau cannot yield (a metamethod, a `table.sort` comparator, a `string.gsub`
/// callback) they fail with the yield error and do nothing.
#[mlua_ir::module("std:fs")]
pub mod fs {
    use std::io::{BufReader, BufWriter};
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{Bytes, Object, Options, Shape};
    use serde::{Deserialize, Serialize};

    use super::error::FsError;
    use super::ops::{self, Entry, FileKind, Metadata, Permission};
    use super::watch::{self, Event, Watch};
    use super::{blocking, path_of};
    use crate::io::io::{Reader, Writer};
    use crate::time::time::Duration;
    use neoc_vm_core::osstr::OsPath;

    /// One entry of a `read_dir` or `walk` listing.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct DirEntry {
        /// The entry's own name, as the filesystem has it (bytes on Unix, so it may not be UTF-8).
        pub file_name: OsPath,
        /// The directory given to `read_dir` or `walk` joined with the entry's path under it.
        pub path: OsPath,
        /// What the entry is; a symlink is `"symlink"`, not what it points at.
        pub kind: FileKind,
        /// How far below the listed directory the entry is: `1` for a direct child (always, for `read_dir`).
        pub depth: u32,
    }

    fn listing(entries: Vec<Entry>) -> Vec<DirEntry> {
        entries
            .into_iter()
            .map(|entry| DirEntry {
                file_name: OsPath::of(entry.name),
                path: OsPath(entry.path),
                kind: entry.kind,
                depth: u32::try_from(entry.depth).unwrap_or(u32::MAX),
            })
            .collect()
    }

    /// How `walk` walks.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct WalkOptions {
        /// How far down to go: `1` lists the directory's own entries, as `read_dir` does. Unlimited when not given.
        pub max_depth: Option<u32>,
    }

    /// What a removal does about a path that is not there.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct RemoveOptions {
        /// A missing path is not an error: the call returns `false`.
        pub missing_ok: bool,
    }

    /// What `watch` watches.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct WatchOptions {
        /// Watch the whole tree under the directory, not only its direct children.
        pub recursive: bool,
    }

    /// Read the entire contents of `path`, as bytes. Mirrors `std::fs::read`.
    ///
    /// ```luau
    /// local data, err = fs.read("no/such/file")
    /// assert(data == nil and err ~= nil and err.kind == "not_found")
    /// ```
    pub fn read(path: OsPath) -> Result<Bytes, FsError> {
        let path = path_of(path)?;
        std::fs::read(&path).map(Bytes).map_err(|e| FsError::io(&e, &path))
    }

    /// Write `data` to `path`, replacing any existing content. Mirrors `std::fs::write`.
    pub fn write(path: OsPath, data: Bytes) -> Result<(), FsError> {
        let path = path_of(path)?;
        std::fs::write(&path, &data.0).map_err(|e| FsError::io(&e, &path))
    }

    /// Open the file `path` for reading. Mirrors `std::fs::File::open`.
    pub fn open(path: OsPath) -> Result<Reader, FsError> {
        let path = path_of(path)?;
        let file = std::fs::File::open(&path).map_err(|e| FsError::io(&e, &path))?;
        Ok(Reader::of(BufReader::new(file)))
    }

    /// Open the file `path` for writing: it is created, or emptied when it exists. What is written reaches the file
    /// at `flush`, or when the writer is collected. Mirrors `std::fs::File::create`.
    pub fn create(path: OsPath) -> Result<Writer, FsError> {
        let path = path_of(path)?;
        let file = std::fs::File::create(&path).map_err(|e| FsError::io(&e, &path))?;
        Ok(Writer::of(BufWriter::new(file)))
    }

    /// Open the file `path` for writing at its end, creating it when it does not exist. What is written reaches
    /// the file at `flush`, or when the writer is collected.
    pub fn append(path: OsPath) -> Result<Writer, FsError> {
        let path = path_of(path)?;
        let file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| FsError::io(&e, &path))?;
        Ok(Writer::of(BufWriter::new(file)))
    }

    /// Whether `path` exists, following symlinks: `false` for a missing path and for a dangling symlink. Any other
    /// failure to check is an error with its kind, not `false` (`permission_denied`, say). Use `symlink_metadata`
    /// to see a dangling link itself. Mirrors `std::fs::exists`.
    pub fn exists(path: OsPath) -> Result<bool, FsError> {
        ops::exists(&path_of(path)?)
    }

    /// Metadata of `path`, following symlinks (a dangling symlink is `not_found`). Mirrors `std::fs::metadata`.
    ///
    /// ```luau
    /// local meta = assert(fs.metadata("."))
    /// assert(meta.kind == "dir")
    /// ```
    pub fn metadata(path: OsPath) -> Result<Metadata, FsError> {
        ops::metadata(&path_of(path)?, true)
    }

    /// Like `metadata`, but about the symlink itself when `path` is one (its `kind` is then `"symlink"`). Mirrors
    /// `std::fs::symlink_metadata`.
    pub fn symlink_metadata(path: OsPath) -> Result<Metadata, FsError> {
        ops::metadata(&path_of(path)?, false)
    }

    /// Set the Unix permission bits of `path` (through a symlink): `0o755` is `493`, at most `4095`
    /// (`invalid_input` past that). `unsupported` on Windows.
    pub fn set_mode(path: OsPath, mode: u32) -> Result<(), FsError> {
        ops::set_permissions(&path_of(path)?, Permission::Mode(mode))
    }

    /// Forbid or allow writing to `path` (through a symlink). `true` clears every write bit on Unix and sets the
    /// read-only attribute on Windows; `false` gives the owner (not group or others) write access on Unix.
    pub fn set_readonly(path: OsPath, readonly: bool) -> Result<(), FsError> {
        ops::set_permissions(&path_of(path)?, Permission::Readonly(readonly))
    }

    /// Create a single directory. Mirrors `std::fs::create_dir`: `already_exists` when the path exists,
    /// `not_found` when the parent is missing.
    pub fn create_dir(path: OsPath) -> Result<(), FsError> {
        let path = path_of(path)?;
        std::fs::create_dir(&path).map_err(|e| FsError::io(&e, &path))
    }

    /// Create a directory and every missing ancestor. Mirrors `std::fs::create_dir_all`; succeeds if the directory
    /// already exists.
    pub fn create_dir_all(path: OsPath) -> Result<(), FsError> {
        let path = path_of(path)?;
        std::fs::create_dir_all(&path).map_err(|e| FsError::io(&e, &path))
    }

    /// List the directory `path`: its entries sorted by name (Rust leaves the order unspecified), without `.` and
    /// `..`. A symlink is listed as `"symlink"`, not as what it points at. It is `not_found` for a missing path
    /// and `not_a_directory` for a file. The whole listing is read at once, and at most 1048576 entries
    /// (`too_large` past that). `walk` lists a whole tree; `lib:glob` matches patterns. Waits without blocking
    /// other calls.
    pub async fn read_dir(path: OsPath) -> Result<Vec<DirEntry>, FsError> {
        let path = path_of(path)?;
        blocking(move |_| ops::read_dir(&path)).await.map(listing)
    }

    /// List everything under the directory `path`, recursively (the directory itself is not listed): depth first,
    /// the children of each directory sorted by name, so the order is the same on every platform. A symlink inside
    /// is listed and never entered, so a link loop cannot make a walk run for ever; `path` itself may be a symlink
    /// to a directory and is followed. It is `not_found` for a missing path and `not_a_directory` for a file; an
    /// unreadable subdirectory fails the whole walk with `permission_denied` and its path. At most 1048576 entries
    /// (`too_large` past that: walk the subdirectories separately). Waits without blocking other calls.
    pub async fn walk(path: OsPath, opts: WalkOptions) -> Result<Vec<DirEntry>, FsError> {
        let path = path_of(path)?;
        let depth = opts.max_depth.map(|depth| usize::try_from(depth).unwrap_or(usize::MAX));
        blocking(move |cancel| ops::walk(&path, depth, cancel)).await.map(listing)
    }

    /// Copy the file `from` to the path `to` and return the number of bytes written. Mirrors `std::fs::copy`: `to`
    /// is created or replaced and takes the permission bits of `from`; a symlink given as `from` is resolved, and
    /// so is one at `to`. `to` is the new path of the copy, not a directory to copy into (`is_directory`); `from`
    /// must be a file (`is_directory` for a directory: use `copy_dir`); copying a file onto itself is
    /// `invalid_input` rather than emptying it. The parent of `to` must exist (`not_found`). Waits without blocking
    /// other calls.
    pub async fn copy(from: OsPath, to: OsPath) -> Result<u64, FsError> {
        let (from, to) = (path_of(from)?, path_of(to)?);
        blocking(move |_| ops::copy_file(&from, &to)).await
    }

    /// Copy the directory `from` and everything under it to the new path `to`, and return the bytes of file data
    /// copied. `to` must not exist (`already_exists`; nothing is merged) and its parent must; a destination inside
    /// the source is `invalid_input`. Files keep their permission bits, and symlinks are recreated as symlinks
    /// with the target as stored, never followed. A failure part way removes the partial `to` again (best effort).
    /// A symlink given as `from` is resolved. Waits without blocking other calls.
    pub async fn copy_dir(from: OsPath, to: OsPath) -> Result<u64, FsError> {
        let (from, to) = (path_of(from)?, path_of(to)?);
        blocking(move |cancel| ops::copy_dir(&from, &to, cancel)).await
    }

    /// Rename or move `from` to `to` on the same filesystem, in one atomic step, as `std::fs::rename` does. An
    /// existing file or symlink at `to` is replaced; a directory replaces an empty directory only (`not_empty`
    /// otherwise); a file over a directory is `is_directory`, a directory over a file `not_a_directory`; a move
    /// across filesystems is `cross_device` (use `move_path`). A trailing separator or `.` on either path is
    /// dropped first, so a symlink is renamed as a link. Windows refuses a directory over an existing directory
    /// and a file over a directory (`permission_denied`) and cannot rename a file that is open.
    pub fn rename(from: OsPath, to: OsPath) -> Result<(), FsError> {
        ops::rename(&path_of(from)?, &path_of(to)?)
    }

    /// Move `from` to `to`, with the rules of `rename`. When the two are on different filesystems it copies
    /// instead (a file, a symlink as a symlink, or a whole directory tree, never following a link) and removes
    /// `from` only after the copy completed. That case is not atomic: a failed copy leaves the source whole, and a
    /// file or symlink at `to` is replaced by renaming the finished copy over it. Waits without blocking other
    /// calls.
    pub async fn r#move(from: OsPath, to: OsPath) -> Result<(), FsError> {
        let (from, to) = (path_of(from)?, path_of(to)?);
        blocking(move |cancel| ops::move_path(&from, &to, cancel)).await
    }

    /// Remove a file or a symlink (the link, never its target); `true` once removed. A directory is
    /// `is_directory`. Mirrors `std::fs::remove_file`; on Windows a read-only file or one that is open may be
    /// `permission_denied`.
    ///
    /// ```luau
    /// assert(fs.remove_file("no/such/file", { missing_ok = true }) == false)
    /// ```
    pub fn remove_file(path: OsPath, opts: RemoveOptions) -> Result<bool, FsError> {
        ops::remove_file(&path_of(path)?, opts.missing_ok)
    }

    /// Remove an empty directory (`not_empty` otherwise; `remove_dir_all` removes a tree); `true` once removed. A
    /// symlink to a directory is `not_a_directory` (use `remove_file`), also when written `link/`; so is a file.
    pub fn remove_dir(path: OsPath, opts: RemoveOptions) -> Result<bool, FsError> {
        ops::remove_dir(&path_of(path)?, opts.missing_ok)
    }

    /// Remove a directory and everything under it, children before parents; `true` once removed. Symlinks inside
    /// are deleted without being followed, so nothing outside the tree is touched. It refuses `/`, `.`, `..` and
    /// the empty path (`invalid_input`, a guard against a typo, not a security boundary) and a symlink to a
    /// directory (`not_a_directory`: use `remove_file`). A failure part way leaves what was not yet removed; once
    /// started it cannot be interrupted. Waits without blocking other calls.
    pub async fn remove_dir_all(path: OsPath, opts: RemoveOptions) -> Result<bool, FsError> {
        let path = path_of(path)?;
        blocking(move |cancel| ops::remove_dir_all(&path, opts.missing_ok, cancel)).await
    }

    /// The absolute path of `path` with every symlink and `.`/`..` resolved; the path must exist (`not_found`). On
    /// Windows the result has the extended-length `\\?\` prefix. Mirrors `std::fs::canonicalize`; `path.absolute`
    /// makes a path absolute without touching the disk.
    pub fn canonicalize(path: OsPath) -> Result<OsPath, FsError> {
        ops::canonicalize(&path_of(path)?).map(OsPath)
    }

    /// The path the symlink `path` points to, as stored (possibly relative, possibly dangling). A path that is not
    /// a symlink is `invalid_input`. Mirrors `std::fs::read_link`.
    pub fn read_link(path: OsPath) -> Result<OsPath, FsError> {
        ops::read_link(&path_of(path)?).map(OsPath)
    }

    /// Create the symlink `link` pointing at `target` (stored as given; it need not exist). `already_exists` when
    /// `link` exists. On Windows creating a symlink needs the privilege to do so (`permission_denied` otherwise),
    /// and the kind of link (file or directory) follows what `target` is when it is created.
    pub fn symlink(target: OsPath, link: OsPath) -> Result<(), FsError> {
        ops::symlink(&path_of(target)?, &path_of(link)?)
    }

    /// A watch on a directory: `receive` gives its changes one by one.
    #[derive(Clone, Object)]
    pub struct Watcher {
        watch: Arc<Watch>,
    }

    #[mlua_ir::object]
    impl Watcher {
        /// Wait for the next change. Once the watcher is closed it returns `nil` (and no error), also to a call
        /// already waiting. A `timeout` ends the wait with `timed_out` (a zero one only takes what is already
        /// there); without one the call waits for as long as it takes. An error the watch reports is returned and
        /// the watch goes on. Other calls run while it waits.
        ///
        /// ```luau
        /// local time = require("std:time")
        /// local watcher = assert(fs.watch("."))
        /// local event, err = watcher:receive(time.Duration.zero())
        /// assert(event ~= nil or (err ~= nil and err.kind == "timed_out"))
        /// watcher:close()
        /// ```
        // `Mixed`, not `Wait`: a `Wait` body takes the event when its wait ends and only then queues for the turn,
        // and a call dropped there would lose it. `receive` waits through `idle` and takes the event with the
        // turn held.
        #[host("Mixed")]
        pub async fn receive(
            &self,
            timeout: Option<Duration>,
        ) -> mlua::Result<Result<Option<Event>, FsError>> {
            self.watch.receive(timeout.map(Into::into)).await
        }

        /// Stop watching: the operating system's watch is released, events not yet received are dropped, and every
        /// `receive`, waiting or later, returns `nil`. Closing again does nothing.
        pub fn close(&self) {
            self.watch.close();
        }
    }

    /// Watch the directory `path` for changes; read them with `watcher:receive()`. It is `not_found` for a missing
    /// path and `not_a_directory` for a file. The watch is the operating system's (inotify, FSEvents,
    /// ReadDirectoryChangesW), so the directory is not rescanned. Events are not deduplicated or debounced, and one
    /// change can be reported as several; a rename is one event per path. The watch stops at `watcher:close()`,
    /// when the watcher is collected, and when the engine shuts down. Starting a recursive watch on Linux adds one
    /// watch per directory of the tree. Waits without blocking other calls.
    pub async fn watch(lua: Lua, path: OsPath, opts: WatchOptions) -> Result<Watcher, FsError> {
        let path = path_of(path)?;
        let started = blocking(move |_| watch::start(path, opts.recursive)).await?;
        Ok(Watcher { watch: watch::register(started, &lua) })
    }
}

#[cfg(test)]
mod tests;
