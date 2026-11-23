//! The filesystem operations behind `std:fs`, free of the Lua state so they can be tested and reasoned about alone.
//! Ported from neoc-vm's `std/fs/ops.rs` unchanged in what they do; [`FileKind`] and [`Metadata`] are also what a
//! script is given.
//!
//! The rules every operation here keeps:
//!
//! - **Rust's semantics where Rust has the function.** `copy` and `rename` replace an existing destination file
//!   as `std::fs::copy` and `std::fs::rename` do; the operations Rust lacks (`copy_dir`, `move`, `walk`) take the
//!   strictest reading: `copy_dir` refuses an existing destination instead of merging into it.
//! - **Symlinks are never followed while walking.** A recursive copy recreates the links it meets, `walk` lists
//!   them as links and does not enter them, a recursive remove deletes them and never what they point at. Only
//!   the top-level source of a copy or a walk is resolved.
//! - **Walks are bounded by the tree, not by a script.** `walk` and `copy_dir` are iterative (`walkdir`), none
//!   sizes an allocation from a number a script passed, and a listing is capped at [`MAX_ENTRIES`] entries.
//! - **Failures are classified** ([`Kind`]), so a script can tell `not_found` from `permission_denied` on any
//!   platform.
//! - **A job that loops over a tree takes a cancel flag** (`walk` and `copy_dir`, and so the copy half of a
//!   cross-device `move`) and stops at the next entry once it is set (the call that started it was abandoned or
//!   its script shut down). The recursive remove delegates to `std::fs::remove_dir_all` (see [`remove_dir_all`]),
//!   which cannot be interrupted: the flag is checked once before it starts. A single file copy is one
//!   `std::fs::copy` and cannot be interrupted either.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mlua_ir::Shape;
use serde::Serialize;
use walkdir::WalkDir;

use super::error::{FsError, FsResult, Kind};

/// The most entries one `read_dir` or `walk` returns: a listing is built in memory and handed to the script as
/// one table, so it has a bound a script cannot raise. A tree with more is walked in pieces, directory by
/// directory.
pub const MAX_ENTRIES: usize = 1 << 20;

/// What a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "std:fs")]
pub enum FileKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link: only where links are not followed (`symlink_metadata`, a listing).
    Symlink,
    /// Anything else: a device, a FIFO, a socket.
    Other,
}

impl From<fs::FileType> for FileKind {
    fn from(t: fs::FileType) -> Self {
        if t.is_symlink() {
            FileKind::Symlink
        } else if t.is_dir() {
            FileKind::Dir
        } else if t.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        }
    }
}

/// What the filesystem says of one path.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "std:fs")]
pub struct Metadata {
    /// What the path is.
    pub kind: FileKind,
    /// Size in bytes (for a directory, whatever the platform reports).
    pub len: u64,
    /// Whether the permissions forbid writing: on Unix, no write bit at all; on Windows, the read-only attribute.
    pub readonly: bool,
    /// Last modification, in seconds since the Unix epoch; absent if unavailable.
    pub modified: Option<f64>,
    /// Creation (birth) time, in seconds since the Unix epoch; absent where the platform or the filesystem does
    /// not record it (some Linux filesystems).
    pub created: Option<f64>,
    /// Last access, in seconds since the Unix epoch; absent if unavailable, and unreliable: many filesystems do
    /// not keep it up to date.
    pub accessed: Option<f64>,
    /// The Unix permission bits (`0o644` is `420`); absent on Windows.
    pub mode: Option<u32>,
}

impl From<&fs::Metadata> for Metadata {
    fn from(m: &fs::Metadata) -> Self {
        Self {
            kind: m.file_type().into(),
            len: m.len(),
            readonly: m.permissions().readonly(),
            modified: m.modified().ok().map(epoch_seconds),
            created: m.created().ok().map(epoch_seconds),
            accessed: m.accessed().ok().map(epoch_seconds),
            mode: mode_bits(m),
        }
    }
}

#[cfg(unix)]
fn mode_bits(m: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_bits(_: &fs::Metadata) -> Option<u32> {
    None
}

/// A time as seconds since the epoch (negative before it). A Lua number is a double, so this is a double.
fn epoch_seconds(t: SystemTime) -> f64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs_f64(),
        Err(before) => -before.duration().as_secs_f64(),
    }
}

/// One entry of a directory listing or a walk.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: OsString,
    /// The listed directory joined with the entry's path under it.
    pub path: PathBuf,
    /// The entry's own kind: a symlink is `Symlink`, not what it points at.
    pub kind: FileKind,
    /// 1 for a direct child of the listed directory.
    pub depth: usize,
}

/// The metadata of `path`: through a symlink when `follow`, of the link itself otherwise.
pub fn metadata(path: &Path, follow: bool) -> FsResult<Metadata> {
    let m = if follow { fs::metadata(path) } else { fs::symlink_metadata(path) };
    m.map(|m| Metadata::from(&m)).map_err(|e| FsError::io(&e, path))
}

/// Whether `path` exists, following symlinks (a dangling link does not exist). A path that cannot be checked
/// (permission denied on a parent) is an error, not `false`.
pub fn exists(path: &Path) -> FsResult<bool> {
    path.try_exists().map_err(|e| FsError::io(&e, path))
}

/// The entries of the directory `path`, sorted by name (no `.` or `..`).
pub fn read_dir(path: &Path) -> FsResult<Vec<Entry>> {
    read_dir_up_to(path, MAX_ENTRIES)
}

/// [`read_dir`] with its own entry cap, so a test can reach the cap without a million files.
fn read_dir_up_to(path: &Path, cap: usize) -> FsResult<Vec<Entry>> {
    let listing = fs::read_dir(path).map_err(|e| FsError::io(&e, path))?;
    let mut entries = Vec::new();
    for item in listing {
        let item = item.map_err(|e| FsError::io(&e, path))?;
        if entries.len() >= cap {
            return Err(too_many(path, cap, "this directory is too big to list at once"));
        }
        let entry_path = item.path();
        let file_type = item.file_type().map_err(|e| FsError::io(&e, &entry_path))?;
        entries.push(Entry { name: item.file_name(), path: entry_path, kind: file_type.into(), depth: 1 });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn too_many(path: &Path, cap: usize, hint: &str) -> FsError {
    FsError::at(Kind::TooLarge, path, format!("more than {cap} entries; {hint}"))
}

/// Every entry under the directory `root`, depth first, each directory's children sorted by name (no `root`
/// itself). A symlink is listed and not entered; `max_depth` bounds how far down it goes (`Some(1)` is `read_dir`).
pub fn walk(root: &Path, max_depth: Option<usize>, cancel: &AtomicBool) -> FsResult<Vec<Entry>> {
    walk_up_to(root, max_depth, cancel, MAX_ENTRIES)
}

/// [`walk`] with its own entry cap (see [`read_dir_up_to`]).
fn walk_up_to(
    root: &Path,
    max_depth: Option<usize>,
    cancel: &AtomicBool,
    cap: usize,
) -> FsResult<Vec<Entry>> {
    let meta = fs::metadata(root).map_err(|e| FsError::io(&e, root))?;
    if !meta.is_dir() {
        return Err(FsError::at(Kind::NotADirectory, root, "not a directory"));
    }
    if max_depth == Some(0) {
        return Ok(Vec::new());
    }
    let mut walker = WalkDir::new(root).follow_links(false).min_depth(1).sort_by_file_name();
    if let Some(depth) = max_depth {
        walker = walker.max_depth(depth);
    }
    let mut entries = Vec::new();
    for item in walker {
        check_cancel(cancel)?;
        let item = item.map_err(|e| walk_error(&e, root))?;
        if entries.len() >= cap {
            return Err(too_many(root, cap, "walk the subdirectories one at a time, or lower max_depth"));
        }
        entries.push(Entry {
            name: item.file_name().to_owned(),
            path: item.path().to_owned(),
            kind: item.file_type().into(),
            depth: item.depth(),
        });
    }
    Ok(entries)
}

fn check_cancel(cancel: &AtomicBool) -> FsResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(FsError::new(Kind::Other, "cancelled: the call that started it is gone"))
    } else {
        Ok(())
    }
}

fn walk_error(e: &walkdir::Error, fallback: &Path) -> FsError {
    let path = e.path().unwrap_or(fallback);
    match e.io_error() {
        Some(io) => FsError::io(io, path),
        None => FsError::at(Kind::Other, path, e.to_string()),
    }
}

/// Copies the file `from` to `to` and returns the bytes written, as `std::fs::copy` does: `to` is created or
/// replaced (through a symlink there, as the standard library does) and takes the source's permission bits.
/// A directory source, a directory destination and copying a file onto itself (which would truncate it) are
/// refused up front.
pub fn copy_file(from: &Path, to: &Path) -> FsResult<u64> {
    let meta = fs::metadata(from).map_err(|e| FsError::io(&e, from))?;
    if meta.is_dir() {
        return Err(FsError::at(Kind::IsDirectory, from, "is a directory; use copy_dir"));
    }
    if !meta.is_file() {
        // a device or a FIFO may never end: copying one could fill the disk
        return Err(FsError::at(Kind::InvalidInput, from, "not a regular file, directory or symlink"));
    }
    match fs::metadata(to) {
        Ok(existing) if existing.is_dir() => {
            return Err(FsError::at(Kind::IsDirectory, to, "the destination is a directory"));
        }
        Ok(existing) if !existing.is_file() => {
            // opening a FIFO for writing blocks until someone reads it; a device is not a file to replace
            return Err(FsError::at(
                Kind::InvalidInput,
                to,
                "the destination exists and is not a regular file",
            ));
        }
        Ok(_) => match same_file(from, to) {
            Ok(false) => {}
            Ok(true) => {
                return Err(FsError::at(
                    Kind::InvalidInput,
                    to,
                    "the source and the destination are the same file",
                ));
            }
            Err(e) => return Err(FsError::io(&e, to)),
        },
        // a missing destination is made by the copy; anything else it reports itself
        Err(_) => {}
    }
    fs::copy(from, to).map_err(|e| FsError::io2(&e, from, to))
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (fs::metadata(a)?, fs::metadata(b)?);
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    Ok(fs::canonicalize(a)? == fs::canonicalize(b)?)
}

/// Copies the directory tree `from` to the new path `to` (which must not exist; its parent must), iteratively and
/// without following a symlink inside it, and returns the bytes of file data copied. A failure part way removes
/// the partial `to` again (this call created it; the removal is best effort, so if it fails too the partial copy stays).
pub fn copy_dir(from: &Path, to: &Path, cancel: &AtomicBool) -> FsResult<u64> {
    let meta = fs::metadata(from).map_err(|e| FsError::io(&e, from))?;
    if !meta.is_dir() {
        return Err(FsError::at(Kind::NotADirectory, from, "not a directory; use copy"));
    }
    let source = fs::canonicalize(from).map_err(|e| FsError::io(&e, from))?;
    let destination = resolve_for_creation(to).map_err(|e| FsError::io(&e, to))?;
    if destination.starts_with(&source) {
        // it would walk the copy it is making, for ever
        return Err(FsError::at(Kind::InvalidInput, to, "the destination is inside the source"));
    }
    match fs::symlink_metadata(to) {
        Ok(_) => {
            return Err(FsError::at(Kind::AlreadyExists, to, "the destination exists"));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(FsError::io(&e, to)),
    }
    fs::create_dir(to).map_err(|e| FsError::io2(&e, from, to))?;
    let copied = copy_tree(from, to, cancel);
    if copied.is_err() {
        // best effort, and not cancellable (the cancel flag may be why the copy stopped): the failure being
        // reported is the copy's, not the cleanup's
        let _ = remove_tree(to, &AtomicBool::new(false));
    }
    copied
}

/// Copies what is under `from` into the existing empty directory `to`.
fn copy_tree(from: &Path, to: &Path, cancel: &AtomicBool) -> FsResult<u64> {
    let mut copied = 0_u64;
    let walk = WalkDir::new(from).follow_links(false).min_depth(1).sort_by_file_name();
    for item in walk {
        check_cancel(cancel)?;
        let item = item.map_err(|e| walk_error(&e, from))?;
        let relative = item
            .path()
            .strip_prefix(from)
            .map_err(|_| FsError::at(Kind::Other, item.path(), "walked outside the source"))?;
        let target = to.join(relative);
        let file_type = item.file_type();
        if file_type.is_dir() {
            fs::create_dir(&target).map_err(|e| FsError::io2(&e, item.path(), &target))?;
        } else if file_type.is_symlink() {
            copy_symlink(item.path(), &target)?;
        } else if file_type.is_file() {
            let bytes = fs::copy(item.path(), &target).map_err(|e| FsError::io2(&e, item.path(), &target))?;
            copied = copied.saturating_add(bytes);
        } else {
            return Err(FsError::at(
                Kind::InvalidInput,
                item.path(),
                "not a regular file, directory or symlink",
            ));
        }
    }
    Ok(copied)
}

/// Recreates the symlink `from` at the new path `to` with the same (unresolved) target.
fn copy_symlink(from: &Path, to: &Path) -> FsResult<()> {
    let target = fs::read_link(from).map_err(|e| FsError::io(&e, from))?;
    make_symlink(&target, to).map_err(|e| FsError::io2(&e, from, to))
}

/// `path` made absolute and symlink-free as far as it exists, with the missing tail appended as given.
fn resolve_for_creation(path: &Path) -> io::Result<PathBuf> {
    let mut tail = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        match fs::canonicalize(&current) {
            Ok(mut resolved) => {
                resolved.extend(tail.iter().rev());
                return Ok(resolved);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let (Some(name), Some(parent)) =
                    (current.file_name().map(ToOwned::to_owned), current.parent())
                else {
                    return Err(e);
                };
                tail.push(name);
                current =
                    if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent.to_path_buf() };
            }
            Err(e) => return Err(e),
        }
    }
}

/// Removes the file or symlink `path` (never what a symlink points at). `false` when it is missing and
/// `missing_ok`.
pub fn remove_file(path: &Path, missing_ok: bool) -> FsResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Err(FsError::at(Kind::IsDirectory, path, "is a directory; use remove_dir")),
        Ok(m) => remove_leaf(path, m.file_type()).map(|()| true).map_err(|e| FsError::io(&e, path)),
        Err(e) if missing_ok && e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(FsError::io(&e, path)),
    }
}

/// Removes the empty directory `path`. A symlink to a directory is not one: it is refused, also when spelled
/// `link/`.
pub fn remove_dir(path: &Path, missing_ok: bool) -> FsResult<bool> {
    let path = without_trailing_dots(path);
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_dir() => Err(not_a_directory(&path)),
        Ok(_) => fs::remove_dir(&path).map(|()| true).map_err(|e| FsError::io(&e, &path)),
        Err(e) if missing_ok && e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(FsError::io(&e, &path)),
    }
}

/// Removes the directory `path` and everything under it (`std::fs::remove_dir_all`: symlinks inside are removed,
/// never followed; not interruptible once started). It refuses a path that names no directory of its own (`""`, `/`, `.`, `..`) and a symlink to a
/// directory, also when spelled `link/` or `link/.`: the operating system follows a link spelled like that.
pub fn remove_dir_all(path: &Path, missing_ok: bool, cancel: &AtomicBool) -> FsResult<bool> {
    if path.as_os_str().is_empty() {
        return Err(FsError::new(Kind::InvalidInput, "the path is empty"));
    }
    let path = without_trailing_dots(path);
    // a typo guard, not a security boundary: `/`, `.` and `..` name no directory of their own
    if path.file_name().is_none() {
        return Err(FsError::at(
            Kind::InvalidInput,
            &path,
            "refusing to remove it recursively: it names no directory",
        ));
    }
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_dir() => Err(not_a_directory(&path)),
        Ok(_) => remove_tree(&path, cancel).map(|()| true),
        Err(e) if missing_ok && e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(FsError::io(&e, &path)),
    }
}

/// Deletes the tree under `root` (a real directory, not a link) with `std::fs::remove_dir_all`, which walks with
/// `openat` and `unlinkat` and so is not fooled by a directory swapped for a symlink while it runs (CVE-2022-21658),
/// and is not limited by the length of a path. The price: once it has started it cannot be interrupted, so the cancel
/// flag is checked only before it.
fn remove_tree(root: &Path, cancel: &AtomicBool) -> FsResult<()> {
    check_cancel(cancel)?;
    fs::remove_dir_all(root).map_err(|e| FsError::io(&e, root))
}

fn not_a_directory(path: &Path) -> FsError {
    FsError::at(Kind::NotADirectory, path, "not a directory (a symlink is not followed; use remove_file)")
}

/// `path` without a trailing separator or `.` (`link/` and `link/.` become `link`), so that a symlink spelled that
/// way is still seen as a symlink: the operating system follows one when the path ends like that.
fn without_trailing_dots(path: &Path) -> PathBuf {
    match path.file_name() {
        Some(name) => path.parent().map_or_else(|| PathBuf::from(name), |parent| parent.join(name)),
        None => path.to_path_buf(),
    }
}

/// Removes a file or a symlink. On Windows a symlink to a directory is removed as a directory.
#[cfg_attr(not(windows), allow(unused_variables))]
fn remove_leaf(path: &Path, file_type: fs::FileType) -> io::Result<()> {
    let removed = fs::remove_file(path);
    #[cfg(windows)]
    if removed.is_err() && file_type.is_symlink() {
        return fs::remove_dir(path).or(removed);
    }
    removed
}

/// Renames `from` to `to` on one filesystem, as `std::fs::rename` does (the operating system's rename: atomic, and
/// replacing an existing file or symlink at `to`). A trailing separator or `.` on either path is dropped first.
pub fn rename(from: &Path, to: &Path) -> FsResult<()> {
    // `link/` would make some systems (macOS) rename what the link points at: a trailing separator is dropped, so a
    // symlink is renamed as the link it is on every platform
    let (from, to) = (without_trailing_dots(from), without_trailing_dots(to));
    fs::rename(&from, &to).map_err(|e| FsError::io2(&e, &from, &to))
}

/// Moves `from` to `to`: a rename, and when that crosses a filesystem boundary, a copy followed by removing the
/// source (see [`move_by_copy`]). A trailing separator or `.` on either path is dropped once, here, so that both
/// halves treat a symlink as the link it is.
pub fn move_path(from: &Path, to: &Path, cancel: &AtomicBool) -> FsResult<()> {
    move_path_with(from, to, cancel, rename, copy_file)
}

/// [`move_path`] with its two system-facing steps passed in, so a test can make the rename cross a device and the
/// copy fail part way.
fn move_path_with(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    rename_step: impl Fn(&Path, &Path) -> FsResult<()>,
    copy_step: impl Fn(&Path, &Path) -> FsResult<u64>,
) -> FsResult<()> {
    let (from, to) = (without_trailing_dots(from), without_trailing_dots(to));
    match rename_step(&from, &to) {
        Err(e) if e.kind() == Kind::CrossDevice => move_by_copy_with(&from, &to, cancel, copy_step),
        other => other,
    }
}

/// The cross-device half of a move: copy `from` (a file, a symlink as a symlink, a directory tree) to `to`, and only
/// when that completed remove `from`. The destination rules are the ones of `rename`: a file or symlink replaces a
/// file or symlink (the copy is made under a temporary name next to `to` and renamed over it, so a copy that fails
/// leaves the old `to` untouched and a hard link to it is not rewritten), a directory replaces an empty directory
/// only (that empty directory is removed before the copy starts). A failure part way leaves the source whole,
/// except that removing a source tree is the last step and can fail part way through it.
fn move_by_copy_with(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    copy_step: impl Fn(&Path, &Path) -> FsResult<u64>,
) -> FsResult<()> {
    let (from, to) = (without_trailing_dots(from), without_trailing_dots(to));
    let (from, to) = (from.as_path(), to.as_path());
    let meta = fs::symlink_metadata(from).map_err(|e| FsError::io(&e, from))?;
    let existing = match fs::symlink_metadata(to) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(FsError::io(&e, to)),
    };
    match (meta.is_dir(), existing) {
        (true, Some(m)) if m.is_dir() => {
            let empty = fs::read_dir(to).map_err(|e| FsError::io(&e, to))?.next().is_none();
            if !empty {
                return Err(FsError::at(Kind::NotEmpty, to, "the destination directory is not empty"));
            }
            fs::remove_dir(to).map_err(|e| FsError::io(&e, to))?;
        }
        (true, Some(_)) => {
            return Err(FsError::at(Kind::NotADirectory, to, "the destination is not a directory"));
        }
        (false, Some(m)) if m.is_dir() => {
            return Err(FsError::at(Kind::IsDirectory, to, "the destination is a directory"));
        }
        // a file or symlink at `to` is replaced by the rename of the finished copy below
        (false, Some(_)) | (_, None) => {}
    }
    if meta.is_dir() {
        copy_dir(from, to, cancel)?;
        remove_tree(from, cancel)
    } else {
        place_beside(from, to, &meta, copy_step)?;
        remove_leaf(from, meta.file_type()).map_err(|e| FsError::io(&e, from))
    }
}

/// Puts a copy of the file or symlink `from` at `to` in one step: the copy is made under a name that did not exist
/// next to `to` (created exclusively, with a random name, so a link planted there beforehand is never followed or
/// removed and nothing already there is overwritten), and renamed over `to` only when it is whole. Whatever fails,
/// only what this call created is removed.
fn place_beside(
    from: &Path,
    to: &Path,
    meta: &fs::Metadata,
    copy_step: impl Fn(&Path, &Path) -> FsResult<u64>,
) -> FsResult<()> {
    let dir = match to.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    if meta.file_type().is_symlink() {
        let staged = stage_symlink(from, dir)?;
        fs::rename(&staged, to).map_err(|e| {
            // this call made `staged`, so it may remove it
            let _ = remove_leaf(&staged, meta.file_type());
            FsError::io2(&e, &staged, to)
        })
    } else {
        let staged = tempfile::Builder::new()
            .prefix(".neoc-move-")
            .tempfile_in(dir)
            .map_err(|e| FsError::io(&e, dir))?;
        // dropping `staged` on any early return deletes the file this call created, and only that one
        copy_step(from, staged.path())?;
        staged.persist(to).map(|_| ()).map_err(|e| FsError::io2(&e.error, from, to))
    }
}

/// Recreates the symlink `from` next to the destination under a fresh random name and returns that name. `symlink(2)`
/// refuses an existing name, so a taken name is retried with another and nothing existing is ever replaced.
fn stage_symlink(from: &Path, dir: &Path) -> FsResult<PathBuf> {
    let target = fs::read_link(from).map_err(|e| FsError::io(&e, from))?;
    let mut last = None;
    for _ in 0..16 {
        let staged = dir.join(format!(".neoc-move-{}", uuid::Uuid::new_v4().simple()));
        match make_symlink(&target, &staged) {
            Ok(()) => return Ok(staged),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(FsError::io2(&e, from, &staged)),
        }
    }
    let error = last.unwrap_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists));
    Err(FsError::io2(&error, from, dir))
}

/// The path a symlink at `path` points to, as stored.
pub fn read_link(path: &Path) -> FsResult<PathBuf> {
    fs::read_link(path).map_err(|e| FsError::io(&e, path))
}

/// The absolute path of `path` with every symlink resolved; it must exist. (On Windows the result has the
/// extended-length `\\?\` prefix.)
pub fn canonicalize(path: &Path) -> FsResult<PathBuf> {
    fs::canonicalize(path).map_err(|e| FsError::io(&e, path))
}

/// Creates the symlink `link` pointing at `target` (stored as given; it need not exist).
pub fn symlink(target: &Path, link: &Path) -> FsResult<()> {
    make_symlink(target, link).map_err(|e| FsError::io2(&e, target, link))
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    let resolved = link.parent().map_or_else(|| target.to_path_buf(), |parent| parent.join(target));
    if fs::metadata(resolved).is_ok_and(|m| m.is_dir()) {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
fn make_symlink(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// What `set_permissions` changes: the Unix permission bits, or the read-only state (not both).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Exactly these permission bits (`0o755`), at most `0o7777`. Unix only.
    Mode(u32),
    /// Whether the file may be written. On Unix it clears (`true`) or sets the owner's write bit (`false`) and leaves
    /// every other bit as it is; on Windows it is the read-only attribute.
    Readonly(bool),
}

/// Changes the permissions of `path` (through a symlink, as `chmod` does).
pub fn set_permissions(path: &Path, change: Permission) -> FsResult<()> {
    let current = fs::metadata(path).map_err(|e| FsError::io(&e, path))?;
    let permissions = apply(current.permissions(), change)
        .map_err(|message| FsError::at(permission_error_kind(change), path, message))?;
    fs::set_permissions(path, permissions).map_err(|e| FsError::io(&e, path))
}

fn permission_error_kind(change: Permission) -> Kind {
    match change {
        Permission::Mode(mode) if mode > 0o7777 => Kind::InvalidInput,
        _ => Kind::Unsupported,
    }
}

#[cfg(unix)]
fn apply(mut permissions: fs::Permissions, change: Permission) -> Result<fs::Permissions, String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match change {
        Permission::Mode(mode) if mode > 0o7777 => {
            return Err(format!("mode {mode:#o} is above 0o7777"));
        }
        Permission::Mode(mode) => mode,
        // `Permissions::set_readonly(false)` would make the file writable by everyone: change the owner's bit only
        Permission::Readonly(true) => permissions.mode() & !0o222,
        Permission::Readonly(false) => permissions.mode() | 0o200,
    };
    permissions.set_mode(mode);
    Ok(permissions)
}

#[cfg(not(unix))]
fn apply(mut permissions: fs::Permissions, change: Permission) -> Result<fs::Permissions, String> {
    match change {
        Permission::Mode(_) => {
            Err("Unix permission bits do not exist on this platform; use readonly".to_owned())
        }
        Permission::Readonly(on) => {
            permissions.set_readonly(on);
            Ok(permissions)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn kind<T: std::fmt::Debug>(r: FsResult<T>) -> Kind {
        r.unwrap_err().kind()
    }

    fn move_by_copy(from: &Path, to: &Path, cancel: &AtomicBool) -> FsResult<()> {
        move_by_copy_with(from, to, cancel, copy_file)
    }

    fn live() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// `root/src/{a.txt, sub/b.txt, sub/deeper/c.txt}`.
    fn tree(root: &Path) -> PathBuf {
        let src = root.join("src");
        fs::create_dir_all(src.join("sub/deeper")).unwrap();
        write(&src.join("a.txt"), "a");
        write(&src.join("sub/b.txt"), "bb");
        write(&src.join("sub/deeper/c.txt"), "ccc");
        src
    }

    #[test]
    fn metadata_describes_files_directories_and_missing_paths() {
        let d = dir();
        let file = d.path().join("f");
        write(&file, "hello");
        let m = metadata(&file, true).unwrap();
        assert_eq!((m.kind, m.len), (FileKind::File, 5));
        assert!(m.modified.is_some_and(|t| t > 1.0e9));
        assert_eq!(metadata(d.path(), true).unwrap().kind, FileKind::Dir);
        assert_eq!(kind(metadata(&d.path().join("nope"), true)), Kind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn metadata_reports_unix_permission_bits_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir();
        let file = d.path().join("f");
        write(&file, "x");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(metadata(&file, true).unwrap().mode, Some(0o640));
        let link = d.path().join("l");
        symlink(&file, &link).unwrap();
        assert_eq!(metadata(&link, false).unwrap().kind, FileKind::Symlink);
        assert_eq!(metadata(&link, true).unwrap().kind, FileKind::File);
        let dangling = d.path().join("dangling");
        symlink(&d.path().join("gone"), &dangling).unwrap();
        assert_eq!(metadata(&dangling, false).unwrap().kind, FileKind::Symlink);
        assert_eq!(kind(metadata(&dangling, true)), Kind::NotFound);
        assert!(!exists(&dangling).unwrap());
    }

    #[test]
    fn exists_is_false_for_missing_and_true_for_present() {
        let d = dir();
        assert!(exists(d.path()).unwrap());
        assert!(!exists(&d.path().join("nope")).unwrap());
    }

    #[test]
    fn read_dir_lists_sorted_entries_with_their_kinds() {
        let d = dir();
        write(&d.path().join("b"), "");
        write(&d.path().join("a"), "");
        fs::create_dir(d.path().join("c")).unwrap();
        let listing = read_dir(d.path()).unwrap();
        let seen: Vec<_> = listing.iter().map(|e| (e.name.to_str().unwrap().to_owned(), e.kind)).collect();
        assert_eq!(
            seen,
            vec![
                ("a".to_owned(), FileKind::File),
                ("b".to_owned(), FileKind::File),
                ("c".to_owned(), FileKind::Dir)
            ]
        );
        assert_eq!(listing[0].path, d.path().join("a"));
        assert_eq!(listing[0].depth, 1);
    }

    #[test]
    fn read_dir_errors_for_a_missing_directory_and_a_file() {
        let d = dir();
        assert_eq!(kind(read_dir(&d.path().join("nope"))), Kind::NotFound);
        let file = d.path().join("f");
        write(&file, "");
        assert_eq!(kind(read_dir(&file)), Kind::NotADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn read_dir_reports_a_symlink_as_a_symlink() {
        let d = dir();
        fs::create_dir(d.path().join("real")).unwrap();
        symlink(&d.path().join("real"), &d.path().join("link")).unwrap();
        let kinds: Vec<_> = read_dir(d.path()).unwrap().into_iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![FileKind::Symlink, FileKind::Dir]);
    }

    #[test]
    fn walk_lists_the_whole_tree_depth_first_with_depths() {
        let d = dir();
        let src = tree(d.path());
        let entries = walk(&src, None, &live()).unwrap();
        let seen: Vec<_> = entries
            .iter()
            .map(|e| (e.path.strip_prefix(&src).unwrap().to_str().unwrap().replace('\\', "/"), e.depth))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("a.txt".to_owned(), 1),
                ("sub".to_owned(), 1),
                ("sub/b.txt".to_owned(), 2),
                ("sub/deeper".to_owned(), 2),
                ("sub/deeper/c.txt".to_owned(), 3),
            ]
        );
        assert_eq!(walk(&src, Some(1), &live()).unwrap().len(), 2);
        assert_eq!(walk(&src, Some(0), &live()).unwrap().len(), 0);
        assert_eq!(walk(&src, Some(usize::MAX), &live()).unwrap().len(), 5);
    }

    #[test]
    fn walk_needs_an_existing_directory() {
        let d = dir();
        assert_eq!(kind(walk(&d.path().join("nope"), None, &live())), Kind::NotFound);
        let file = d.path().join("f");
        write(&file, "");
        assert_eq!(kind(walk(&file, None, &live())), Kind::NotADirectory);
    }

    #[test]
    fn walk_stops_once_cancelled() {
        let d = dir();
        let src = tree(d.path());
        let cancelled = AtomicBool::new(true);
        assert_eq!(kind(walk(&src, None, &cancelled)), Kind::Other);
        assert_eq!(kind(copy_dir(&src, &d.path().join("dst"), &cancelled)), Kind::Other);
        assert!(!d.path().join("dst").exists(), "a cancelled copy leaves nothing behind");
    }

    #[cfg(unix)]
    #[test]
    fn walk_lists_symlinks_and_never_enters_them() {
        let d = dir();
        let src = tree(d.path());
        symlink(d.path(), &src.join("loop")).unwrap();
        symlink(Path::new("a.txt"), &src.join("zlink")).unwrap();
        let entries = walk(&src, None, &live()).unwrap();
        assert_eq!(entries.len(), 7);
        let kinds: Vec<_> = entries.iter().filter(|e| e.kind == FileKind::Symlink).map(|e| &e.name).collect();
        assert_eq!(kinds.len(), 2);
        // the root itself may be a link: it is followed
        let via_link = d.path().join("via");
        symlink(&src, &via_link).unwrap();
        assert_eq!(walk(&via_link, None, &live()).unwrap().len(), 7);
    }

    #[test]
    fn copy_file_copies_data_and_replaces_like_std() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "one");
        assert_eq!(copy_file(&a, &b).unwrap(), 3);
        assert_eq!(read(&b), "one");
        write(&a, "two!");
        assert_eq!(copy_file(&a, &b).unwrap(), 4);
        assert_eq!(read(&b), "two!");
    }

    #[test]
    fn copy_file_reports_missing_source_and_missing_parent() {
        let d = dir();
        let a = d.path().join("a");
        assert_eq!(kind(copy_file(&d.path().join("nope"), &a)), Kind::NotFound);
        write(&a, "x");
        assert_eq!(kind(copy_file(&a, &d.path().join("no/such/b"))), Kind::NotFound);
    }

    #[test]
    fn copy_file_refuses_directories_and_itself() {
        let d = dir();
        let a = d.path().join("a");
        write(&a, "x");
        fs::create_dir(d.path().join("dir")).unwrap();
        assert_eq!(kind(copy_file(&a, &d.path().join("dir"))), Kind::IsDirectory);
        assert_eq!(kind(copy_file(d.path(), &a)), Kind::IsDirectory);
        assert_eq!(kind(copy_file(&a, &a)), Kind::InvalidInput);
        assert_eq!(read(&a), "x", "copying a file onto itself does not truncate it");
    }

    #[cfg(unix)]
    #[test]
    fn copy_file_refuses_a_hard_link_to_itself_and_a_fifo_source() {
        let d = dir();
        let a = d.path().join("a");
        write(&a, "x");
        let hard = d.path().join("hard");
        fs::hard_link(&a, &hard).unwrap();
        assert_eq!(kind(copy_file(&a, &hard)), Kind::InvalidInput);
        assert_eq!(read(&a), "x");
        let status = std::process::Command::new("mkfifo").arg(d.path().join("fifo")).status();
        if status.is_ok_and(|s| s.success()) {
            assert_eq!(kind(copy_file(&d.path().join("fifo"), &d.path().join("out"))), Kind::InvalidInput);
        }
    }

    #[cfg(unix)]
    #[test]
    fn copy_file_keeps_permission_bits() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "x");
        fs::set_permissions(&a, fs::Permissions::from_mode(0o751)).unwrap();
        copy_file(&a, &b).unwrap();
        assert_eq!(metadata(&b, true).unwrap().mode, Some(0o751));
    }

    #[test]
    fn copy_dir_copies_everything_and_counts_bytes() {
        let d = dir();
        let src = tree(d.path());
        let dst = d.path().join("dst");
        assert_eq!(copy_dir(&src, &dst, &live()).unwrap(), 6);
        assert_eq!(read(&dst.join("a.txt")), "a");
        assert_eq!(read(&dst.join("sub/b.txt")), "bb");
        assert_eq!(read(&dst.join("sub/deeper/c.txt")), "ccc");
        assert!(src.join("sub/deeper/c.txt").exists(), "the source is untouched");
    }

    #[test]
    fn copy_dir_copies_empty_directories_too() {
        let d = dir();
        let src = d.path().join("src");
        fs::create_dir_all(src.join("empty/inner")).unwrap();
        copy_dir(&src, &d.path().join("dst"), &live()).unwrap();
        assert!(d.path().join("dst/empty/inner").is_dir());
    }

    #[test]
    fn copy_dir_refuses_an_existing_destination_and_a_file_source() {
        let d = dir();
        let src = tree(d.path());
        let dst = d.path().join("dst");
        fs::create_dir(&dst).unwrap();
        write(&dst.join("keep"), "k");
        assert_eq!(kind(copy_dir(&src, &dst, &live())), Kind::AlreadyExists);
        assert_eq!(read(&dst.join("keep")), "k");
        let file = d.path().join("file");
        write(&file, "f");
        assert_eq!(kind(copy_dir(&src, &file, &live())), Kind::AlreadyExists);
        assert_eq!(kind(copy_dir(&file, &d.path().join("x"), &live())), Kind::NotADirectory);
        assert_eq!(kind(copy_dir(&d.path().join("nope"), &d.path().join("x"), &live())), Kind::NotFound);
    }

    #[test]
    fn copy_dir_into_itself_is_refused_before_anything_is_created() {
        let d = dir();
        let src = tree(d.path());
        assert_eq!(kind(copy_dir(&src, &src.join("sub/copy"), &live())), Kind::InvalidInput);
        assert!(!src.join("sub/copy").exists());
        assert_eq!(kind(copy_dir(&src, &src, &live())), Kind::InvalidInput);
        // a destination that is outside, but whose name merely starts like the source's, is fine
        assert!(copy_dir(&src, &d.path().join("src2"), &live()).is_ok());
    }

    #[test]
    fn copy_dir_needs_an_existing_parent() {
        let d = dir();
        let src = tree(d.path());
        assert_eq!(kind(copy_dir(&src, &d.path().join("no/such/dst"), &live())), Kind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recreates_symlinks_and_never_follows_them() {
        let d = dir();
        let src = tree(d.path());
        let outside = d.path().join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside.join("secret"), "s");
        symlink(&outside, &src.join("to_dir")).unwrap();
        symlink(Path::new("a.txt"), &src.join("to_file")).unwrap();
        // a link back to the source's own parent: following it would never end
        symlink(d.path(), &src.join("loop")).unwrap();
        let dst = d.path().join("dst");
        copy_dir(&src, &dst, &live()).unwrap();
        assert_eq!(metadata(&dst.join("to_dir"), false).unwrap().kind, FileKind::Symlink);
        assert_eq!(read_link(&dst.join("to_dir")).unwrap(), outside);
        assert_eq!(read_link(&dst.join("to_file")).unwrap(), Path::new("a.txt"));
        assert_eq!(read(&dst.join("to_file")), "a");
        assert_eq!(metadata(&dst.join("loop"), false).unwrap().kind, FileKind::Symlink);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_of_a_symlink_to_a_directory_copies_the_directory() {
        let d = dir();
        let src = tree(d.path());
        let link = d.path().join("link");
        symlink(&src, &link).unwrap();
        let dst = d.path().join("dst");
        copy_dir(&link, &dst, &live()).unwrap();
        assert_eq!(read(&dst.join("sub/b.txt")), "bb");
        assert_eq!(metadata(&dst, false).unwrap().kind, FileKind::Dir);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_removes_its_partial_destination_when_it_fails() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir();
        let src = tree(d.path());
        let locked = src.join("sub/b.txt");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = fs::read(&locked).is_ok(); // running as root ignores the mode
        let dst = d.path().join("dst");
        let outcome = copy_dir(&src, &dst, &live());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        if !readable {
            assert_eq!(kind(outcome), Kind::PermissionDenied);
            assert!(!dst.exists(), "a failed copy leaves nothing behind");
        }
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_refuses_a_fifo_inside_the_source_without_opening_it_and_leaves_nothing() {
        let d = dir();
        let src = tree(d.path());
        // after `sub/b.txt` in name order, so part of the tree is already copied when the FIFO is met
        let made = std::process::Command::new("mkfifo").arg(src.join("sub/pipe")).status();
        if made.is_ok_and(|s| s.success()) {
            let dst = d.path().join("dst");
            // opening the FIFO would block for ever: the copy must refuse it by its type instead
            assert_eq!(kind(copy_dir(&src, &dst, &live())), Kind::InvalidInput);
            assert!(!dst.exists(), "the partial copy is removed again");
            assert!(src.join("sub/pipe").exists(), "the source is left as it was");
        }
    }

    #[cfg(unix)]
    #[test]
    fn copy_file_writes_through_a_symlink_at_the_destination_and_a_dangling_one_creates_its_target() {
        let d = dir();
        let (a, target, link) = (d.path().join("a"), d.path().join("target"), d.path().join("link"));
        write(&a, "new");
        write(&target, "old");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(copy_file(&a, &link).unwrap(), 3);
        assert_eq!(read(&target), "new", "the file the link points at is written");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays a link");

        let (missing, dangling) = (d.path().join("missing"), d.path().join("dangling"));
        std::os::unix::fs::symlink(&missing, &dangling).unwrap();
        assert_eq!(copy_file(&a, &dangling).unwrap(), 3);
        assert_eq!(read(&missing), "new", "a dangling link's target is created");
        assert!(fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
    }

    #[test]
    fn remove_file_removes_a_file_and_reports_missing_and_directories() {
        let d = dir();
        let f = d.path().join("f");
        write(&f, "x");
        assert!(remove_file(&f, false).unwrap());
        assert!(!f.exists());
        assert_eq!(kind(remove_file(&f, false)), Kind::NotFound);
        assert!(!remove_file(&f, true).unwrap());
        assert_eq!(kind(remove_file(d.path(), true)), Kind::IsDirectory);
        assert!(d.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_file_removes_a_symlink_and_not_its_target() {
        let d = dir();
        let (f, l) = (d.path().join("f"), d.path().join("l"));
        write(&f, "x");
        symlink(&f, &l).unwrap();
        assert!(remove_file(&l, false).unwrap());
        assert!(f.exists());
        let dir_link = d.path().join("dl");
        symlink(d.path(), &dir_link).unwrap();
        assert!(remove_file(&dir_link, false).unwrap(), "a link to a directory is removed by remove_file");
        assert!(d.path().exists());
    }

    #[test]
    fn remove_dir_removes_only_an_empty_directory() {
        let d = dir();
        let src = tree(d.path());
        assert_eq!(kind(remove_dir(&src, false)), Kind::NotEmpty);
        assert!(src.join("a.txt").exists());
        let empty = d.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(remove_dir(&empty, false).unwrap());
        assert!(!remove_dir(&empty, true).unwrap());
        assert_eq!(kind(remove_dir(&empty, false)), Kind::NotFound);
        let f = d.path().join("f");
        write(&f, "x");
        assert_eq!(kind(remove_dir(&f, true)), Kind::NotADirectory);
    }

    #[test]
    fn remove_dir_all_removes_a_tree_and_honours_missing_ok() {
        let d = dir();
        let src = tree(d.path());
        assert!(remove_dir_all(&src, false, &live()).unwrap());
        assert!(!src.exists());
        assert_eq!(kind(remove_dir_all(&src, false, &live())), Kind::NotFound);
        assert!(!remove_dir_all(&src, true, &live()).unwrap());
        let f = d.path().join("f");
        write(&f, "x");
        assert_eq!(kind(remove_dir_all(&f, true, &live())), Kind::NotADirectory);
        assert!(f.exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_dir_all_deletes_links_and_never_what_they_point_at() {
        let d = dir();
        let src = tree(d.path());
        let outside = d.path().join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside.join("keep"), "k");
        symlink(&outside, &src.join("to_dir")).unwrap();
        symlink(d.path(), &src.join("loop")).unwrap();
        assert!(remove_dir_all(&src, false, &live()).unwrap());
        assert!(!src.exists());
        assert_eq!(read(&outside.join("keep")), "k");
    }

    #[cfg(unix)]
    #[test]
    fn remove_dir_and_remove_dir_all_refuse_a_symlink_to_a_directory() {
        let d = dir();
        let real = tree(d.path());
        let link = d.path().join("link");
        symlink(&real, &link).unwrap();
        assert_eq!(kind(remove_dir_all(&link, false, &live())), Kind::NotADirectory);
        assert_eq!(kind(remove_dir(&link, false)), Kind::NotADirectory);
        assert!(real.join("a.txt").exists());
        // spelled `link/` or `link/.`, the OS would follow the link: it is still refused
        assert_eq!(kind(remove_dir_all(&link.join("."), false, &live())), Kind::NotADirectory);
        assert_eq!(
            kind(remove_dir_all(Path::new(&format!("{}/", link.display())), false, &live())),
            Kind::NotADirectory
        );
        assert_eq!(kind(remove_dir(Path::new(&format!("{}/", link.display())), false)), Kind::NotADirectory);
        assert!(real.join("a.txt").exists());
    }

    #[test]
    fn remove_dir_all_refuses_nameless_paths() {
        assert_eq!(kind(remove_dir_all(Path::new("/"), false, &live())), Kind::InvalidInput);
        assert_eq!(kind(remove_dir_all(Path::new("."), false, &live())), Kind::InvalidInput);
        assert_eq!(kind(remove_dir_all(Path::new(".."), false, &live())), Kind::InvalidInput);
        assert_eq!(kind(remove_dir_all(Path::new(""), false, &live())), Kind::InvalidInput);
    }

    #[test]
    fn remove_dir_all_handles_a_very_deep_tree_without_recursion() {
        let d = dir();
        let mut deep = d.path().join("deep");
        fs::create_dir(&deep).unwrap();
        let root = deep.clone();
        // as deep as a path can go in one step: past this the OS refuses, which is an error, never a crash
        for _ in 0..200 {
            deep = deep.join("d");
            if fs::create_dir(&deep).is_err() {
                break;
            }
        }
        assert!(remove_dir_all(&root, false, &live()).unwrap());
        assert!(!root.exists());
        let again = tree(d.path());
        assert!(walk(&again, None, &live()).is_ok());
    }

    #[test]
    fn rename_moves_and_replaces_a_file_like_std() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "a");
        rename(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(read(&b), "a");
        write(&a, "new");
        rename(&a, &b).unwrap();
        assert_eq!(read(&b), "new");
        assert_eq!(kind(rename(&a, &d.path().join("c"))), Kind::NotFound);
    }

    #[test]
    fn rename_moves_a_directory_with_its_contents() {
        let d = dir();
        let src = tree(d.path());
        let dst = d.path().join("moved");
        rename(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(read(&dst.join("sub/deeper/c.txt")), "ccc");
    }

    #[cfg(unix)]
    #[test]
    fn rename_replaces_an_empty_directory_but_not_a_full_one_and_not_across_kinds() {
        let d = dir();
        let src = tree(d.path());
        let empty = d.path().join("empty");
        fs::create_dir(&empty).unwrap();
        rename(&src, &empty).unwrap();
        assert_eq!(read(&empty.join("a.txt")), "a");
        let src2 = tree(d.path());
        assert_eq!(kind(rename(&src2, &empty)), Kind::NotEmpty);
        let file = d.path().join("file");
        write(&file, "f");
        assert_eq!(kind(rename(&file, &empty)), Kind::IsDirectory);
        assert_eq!(kind(rename(&src2, &file)), Kind::NotADirectory);
    }

    #[test]
    fn move_path_renames_when_it_can() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "a");
        move_path(&a, &b, &live()).unwrap();
        assert_eq!(read(&b), "a");
        assert_eq!(kind(move_path(&a, &d.path().join("c"), &live())), Kind::NotFound);
    }

    #[test]
    fn move_by_copy_moves_files_and_trees_and_removes_the_source_last() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "a");
        move_by_copy(&a, &b, &live()).unwrap();
        assert!(!a.exists());
        assert_eq!(read(&b), "a");
        let src = tree(d.path());
        let dst = d.path().join("moved");
        move_by_copy(&src, &dst, &live()).unwrap();
        assert!(!src.exists());
        assert_eq!(read(&dst.join("sub/deeper/c.txt")), "ccc");
    }

    #[test]
    fn move_by_copy_follows_the_destination_rules_of_rename() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "a");
        write(&b, "b");
        move_by_copy(&a, &b, &live()).unwrap();
        assert_eq!((a.exists(), read(&b)), (false, "a".to_owned()));
        let src = tree(d.path());
        let full = d.path().join("full");
        fs::create_dir(&full).unwrap();
        write(&full.join("x"), "x");
        assert_eq!(kind(move_by_copy(&src, &full, &live())), Kind::NotEmpty);
        assert!(src.join("a.txt").exists(), "a refused move leaves the source whole");
        assert_eq!(kind(move_by_copy(&src, &b, &live())), Kind::NotADirectory);
        assert_eq!(kind(move_by_copy(&b, &full, &live())), Kind::IsDirectory);
        let empty = d.path().join("empty");
        fs::create_dir(&empty).unwrap();
        move_by_copy(&src, &empty, &live()).unwrap();
        assert_eq!(read(&empty.join("a.txt")), "a");
    }

    #[cfg(unix)]
    #[test]
    fn move_by_copy_moves_a_symlink_as_a_symlink() {
        let d = dir();
        let (target, link, moved) = (d.path().join("t"), d.path().join("l"), d.path().join("m"));
        write(&target, "t");
        symlink(&target, &link).unwrap();
        move_by_copy(&link, &moved, &live()).unwrap();
        assert!(!link.exists() && link.symlink_metadata().is_err());
        assert_eq!(read_link(&moved).unwrap(), target);
        assert_eq!(read(&target), "t");
    }

    #[test]
    fn move_by_copy_of_a_missing_source_is_not_found() {
        let d = dir();
        assert_eq!(kind(move_by_copy(&d.path().join("nope"), &d.path().join("x"), &live())), Kind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_read_link_and_canonicalize_agree() {
        let d = dir();
        let (f, l) = (d.path().join("f"), d.path().join("l"));
        write(&f, "x");
        symlink(Path::new("f"), &l).unwrap();
        assert_eq!(read_link(&l).unwrap(), Path::new("f"));
        assert_eq!(canonicalize(&l).unwrap(), canonicalize(&f).unwrap());
        assert_eq!(kind(symlink(Path::new("f"), &l)), Kind::AlreadyExists);
        assert_eq!(kind(read_link(&f)), Kind::InvalidInput);
        assert_eq!(kind(canonicalize(&d.path().join("nope"))), Kind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn set_permissions_sets_bits_and_toggles_readonly_without_widening_access() {
        let d = dir();
        let f = d.path().join("f");
        write(&f, "x");
        set_permissions(&f, Permission::Mode(0o750)).unwrap();
        assert_eq!(metadata(&f, true).unwrap().mode, Some(0o750));
        set_permissions(&f, Permission::Readonly(true)).unwrap();
        let m = metadata(&f, true).unwrap();
        assert_eq!((m.mode, m.readonly), (Some(0o550), true));
        set_permissions(&f, Permission::Readonly(false)).unwrap();
        assert_eq!(
            metadata(&f, true).unwrap().mode,
            Some(0o750 | 0o200),
            "only the owner gains write access"
        );
        assert_eq!(kind(set_permissions(&f, Permission::Mode(0o10000))), Kind::InvalidInput);
        assert_eq!(kind(set_permissions(&d.path().join("nope"), Permission::Mode(0o600))), Kind::NotFound);
    }

    #[test]
    fn a_listing_past_the_cap_is_too_large() {
        let d = dir();
        for name in ["a", "b", "c", "d"] {
            write(&d.path().join(name), "");
        }
        fs::create_dir(d.path().join("sub")).unwrap();
        write(&d.path().join("sub/inner"), "");
        assert_eq!(read_dir_up_to(d.path(), 5).unwrap().len(), 5);
        assert_eq!(kind(read_dir_up_to(d.path(), 4)), Kind::TooLarge);
        assert_eq!(walk_up_to(d.path(), None, &live(), 6).unwrap().len(), 6);
        let e = walk_up_to(d.path(), None, &live(), 5).unwrap_err();
        assert_eq!(e.kind(), Kind::TooLarge);
        assert!(e.to_string().contains("more than 5 entries"), "{e}");
    }

    #[test]
    fn the_recursive_remove_stops_once_cancelled_and_leaves_the_rest() {
        let d = dir();
        let src = tree(d.path());
        let cancelled = AtomicBool::new(true);
        assert_eq!(kind(remove_dir_all(&src, false, &cancelled)), Kind::Other);
        assert!(src.join("sub/deeper/c.txt").exists());
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        fs::create_dir(&a).unwrap();
        write(&a.join("f"), "f");
        assert_eq!(kind(move_by_copy(&a, &b, &cancelled)), Kind::Other);
        assert!(a.join("f").exists(), "a cancelled move leaves the source whole");
        assert!(!b.exists());
    }

    #[test]
    fn a_move_cancelled_after_its_copy_keeps_the_source_and_the_complete_destination() {
        // the copy checks the flag at each entry it meets, and an empty tree has none: the first check this cancelled
        // move reaches is the one before the source is removed
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        fs::create_dir(&a).unwrap();
        let cancelled = AtomicBool::new(true);
        assert_eq!(kind(move_by_copy(&a, &b, &cancelled)), Kind::Other);
        assert!(a.is_dir(), "the source is not removed once the call is gone");
        assert!(b.is_dir(), "the finished copy stays");
    }

    #[cfg(unix)]
    #[test]
    fn copy_file_refuses_a_fifo_destination_instead_of_blocking_on_it() {
        let d = dir();
        let a = d.path().join("a");
        write(&a, "x");
        let fifo = d.path().join("fifo");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        if made.is_ok_and(|s| s.success()) {
            assert_eq!(kind(copy_file(&a, &fifo)), Kind::InvalidInput);
        }
    }

    #[cfg(unix)]
    #[test]
    fn rename_drops_a_trailing_separator_so_a_link_is_renamed_as_a_link() {
        let d = dir();
        let real = tree(d.path());
        let link = d.path().join("link");
        symlink(&real, &link).unwrap();
        rename(Path::new(&format!("{}/", link.display())), &d.path().join("moved")).unwrap();
        assert!(real.join("a.txt").exists(), "what the link pointed at stays where it was");
        assert_eq!(metadata(&d.path().join("moved"), false).unwrap().kind, FileKind::Symlink);
        assert!(!link.exists() && link.symlink_metadata().is_err());
    }

    /// Everything in `dir` that is not one of `expected`: a staging file left behind shows up here.
    fn strays(dir: &Path, expected: &[&str]) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !expected.contains(&n.as_str()))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_cross_device_move_over_a_file_survives_a_copy_that_fails_after_the_destination_was_opened() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "new data");
        write(&b, "old precious");
        // a copy that has written part of the data into whatever path it was given, then fails (a full disk)
        let failing = |_: &Path, into: &Path| -> FsResult<u64> {
            fs::write(into, "par").unwrap();
            Err(FsError::at(Kind::StorageFull, into, "no space left on device"))
        };
        assert_eq!(kind(move_by_copy_with(&a, &b, &live(), failing)), Kind::StorageFull);
        assert_eq!(read(&b), "old precious", "the old destination is untouched");
        assert_eq!(read(&a), "new data", "the source is whole");
        assert_eq!(strays(d.path(), &["a", "b"]), Vec::<String>::new(), "no staging file is left");
        // and when the copy works, the destination is replaced by a rename: a hard link to it keeps the old data
        #[cfg(unix)]
        {
            let hard = d.path().join("hard");
            fs::hard_link(&b, &hard).unwrap();
            move_by_copy(&a, &b, &live()).unwrap();
            assert_eq!(read(&b), "new data");
            assert_eq!(read(&hard), "old precious", "the old inode was replaced, not rewritten");
            assert!(!a.exists());
        }
    }

    #[test]
    fn a_missing_source_leaves_the_destination_alone() {
        let d = dir();
        let b = d.path().join("b");
        write(&b, "old");
        assert_eq!(kind(move_by_copy(&d.path().join("nope"), &b, &live())), Kind::NotFound);
        assert_eq!(read(&b), "old");
    }

    #[cfg(unix)]
    #[test]
    fn a_cross_device_move_of_a_symlink_replaces_a_file_and_a_symlink_like_rename() {
        let d = dir();
        let (target, link) = (d.path().join("t"), d.path().join("l"));
        write(&target, "t");
        symlink(&target, &link).unwrap();
        let existing_file = d.path().join("file");
        write(&existing_file, "old");
        move_by_copy(&link, &existing_file, &live()).unwrap();
        assert_eq!(read_link(&existing_file).unwrap(), target);
        assert!(link.symlink_metadata().is_err(), "the source link is gone");
        assert_eq!(strays(d.path(), &["t", "file"]), Vec::<String>::new());
        let (link2, existing_link) = (d.path().join("l2"), d.path().join("el"));
        symlink(&target, &link2).unwrap();
        symlink(&d.path().join("elsewhere"), &existing_link).unwrap();
        move_by_copy(&link2, &existing_link, &live()).unwrap();
        assert_eq!(read_link(&existing_link).unwrap(), target);
        assert_eq!(read(&target), "t");
    }

    #[cfg(unix)]
    #[test]
    fn a_cross_device_move_of_link_slash_moves_the_link_and_leaves_its_target_alone() {
        let d = dir();
        let real = tree(d.path());
        let link = d.path().join("link");
        symlink(&real, &link).unwrap();
        let moved = d.path().join("moved");
        let crossing = |_: &Path, _: &Path| -> FsResult<()> { Err(FsError::new(Kind::CrossDevice, "x")) };
        let from = PathBuf::from(format!("{}/", link.display()));
        move_path_with(&from, &moved, &live(), crossing, copy_file).unwrap();
        assert_eq!(read_link(&moved).unwrap(), real, "the link was moved as a link");
        assert!(link.symlink_metadata().is_err());
        assert_eq!(read(&real.join("sub/b.txt")), "bb", "its target is whole");
        assert!(real.join("sub/deeper/c.txt").exists());
    }

    #[test]
    fn a_move_that_does_not_cross_a_device_never_reaches_the_copy() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "a");
        let never = |_: &Path, _: &Path| -> FsResult<u64> { panic!("copied") };
        move_path_with(&a, &b, &live(), rename, never).unwrap();
        assert_eq!(read(&b), "a");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_planted_at_a_staging_style_name_is_neither_followed_nor_removed() {
        let d = dir();
        let victim = d.path().join("victim");
        write(&victim, "precious");
        // what an attacker would plant: the names a counter-based staging file would have used
        let planted: Vec<PathBuf> = (0..64)
            .map(|n| d.path().join(format!(".neoc-move-{}-{n}", std::process::id())))
            .chain([d.path().join(".neoc-move-0"), d.path().join(".neoc-move-")])
            .collect();
        for link in &planted {
            symlink(&victim, link).unwrap();
        }
        let (a, to) = (d.path().join("a"), d.path().join("to"));
        write(&a, "new data");
        move_by_copy(&a, &to, &live()).unwrap();
        assert_eq!(read(&to), "new data");
        assert!(!a.exists());
        assert_eq!(read(&victim), "precious", "nothing was written through a planted link");
        // a symlink move too
        let (link, moved) = (d.path().join("l"), d.path().join("moved"));
        symlink(&victim, &link).unwrap();
        move_by_copy(&link, &moved, &live()).unwrap();
        assert_eq!(read_link(&moved).unwrap(), victim);
        assert_eq!(read(&victim), "precious");
        for link in &planted {
            assert_eq!(read_link(link).unwrap(), victim, "{link:?} was removed or replaced");
        }
        assert!(!to.symlink_metadata().unwrap().file_type().is_symlink(), "`to` is the copy, not a link");
        // nothing but what was there, the two results and the planted links
        let mut expected: Vec<String> =
            planted.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        expected.extend(["victim", "to", "moved"].map(String::from));
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(strays(d.path(), &expected), Vec::<String>::new());
    }

    #[test]
    fn a_failed_cross_device_move_leaves_no_staged_file_and_the_destination_intact() {
        let d = dir();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        write(&a, "new");
        write(&b, "old");
        let failing = |_: &Path, into: &Path| -> FsResult<u64> {
            fs::write(into, "partial").unwrap();
            Err(FsError::at(Kind::Other, into, "boom"))
        };
        assert!(move_by_copy_with(&a, &b, &live(), failing).is_err());
        assert_eq!((read(&a), read(&b)), ("new".to_owned(), "old".to_owned()));
        assert_eq!(strays(d.path(), &["a", "b"]), Vec::<String>::new());
    }

    #[test]
    fn resolve_for_creation_appends_the_missing_tail() {
        let d = dir();
        let real = fs::canonicalize(d.path()).unwrap();
        assert_eq!(resolve_for_creation(&d.path().join("a/b")).unwrap(), real.join("a/b"));
        assert_eq!(resolve_for_creation(d.path()).unwrap(), real);
    }
}
