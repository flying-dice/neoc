//! Driven adapter for the `FileStore` / `AppFiles` ports: each app's managed files on the host's disk.
//!
//! **Where.** `<root>/<app>.files/<location>/`, where `<root>` is the directory of the apps' databases
//! (`<data>/apps`, next to `<app>.db`; an app id has no `.`, so the two never collide). A location directory is
//! made the first time the app uses it. The location directory and everything above it belong to the host: a
//! symbolic link there is the operator's choice (another disk, say) and is followed. Purging the app removes
//! `<app>.files` (a link there is removed, never what it points at).
//!
//! **The operations are neoc-vm's.** Recursive copy, move and remove are `neoc_vm::lua::std::fs::ops` (the code
//! behind `std:fs`), so they keep its rules: a recursive remove is `std::fs::remove_dir_all` (immune to
//! CVE-2022-21658, a directory swapped for a link while it runs), a tree copy recreates links instead of
//! following them, a move survives a filesystem boundary. A write, and a file copy, go to a fresh temporary file
//! created by `tempfile` (exclusively, under a random name, next to the destination) that is renamed over the
//! destination when it is whole: a reader never sees half a file, a failure leaves the old file as it was, and
//! a rename replaces the name at the destination, never following a link there. Nothing here hand-rolls a
//! filesystem routine the standard library or `std:fs` already has.
//!
//! **Scoping.** A path reaches this adapter already checked by the domain ([`FilePath::parse`]: relative, no
//! `..`, no NUL, no `:` or backslash, bounded), and each name is pushed only if the platform reads it as one plain
//! name. Then:
//!
//! - **Symbolic links inside a location are never followed.** Every existing directory on the way to the last
//!   name is looked at without following links, and a link there is refused (`invalid_input`). The last name is
//!   looked at the same way: `info` and `list` describe a link as `"symlink"`, `remove` deletes the link itself,
//!   and every other operation refuses it. A link inside a directory tree that is copied, moved or removed is
//!   copied, moved or removed as a link (see above).
//! - **Races (TOCTOU).** The checks and the system calls use paths, so the gap between them is narrowed, not
//!   closed. Closed: a write or a file copy never follows a link at the destination (rename); a read opens the
//!   file and then checks that what it opened is the file it looked at (same device and inode, on Unix), so a
//!   last name swapped for a link meanwhile is refused; a recursive remove cannot be redirected while it runs.
//!   Open: a directory on the way that is swapped for a link between the check and the system call is followed.
//!   Closing that needs handle-relative resolution (`openat2` with `RESOLVE_BENEATH`, or `cap-std`), a new
//!   dependency this change does not take. Only something that can already write the app's files wins that
//!   race (the app itself through `std:fs`, or another process of the host's user), and it can already write
//!   anywhere the host can.
//!
//! This is the convention that keeps apps out of each other's files by accident. It is **not a sandbox**: an app
//! reaches any file the host can through `std:fs`.
//!
//! **Errors** name the app's own path, never a host path. **Every operation runs on the blocking pool**; if the
//! call is dropped (the app was unloaded) a tree copy or move stops at its next entry and a write is not put in
//! place.

use async_trait::async_trait;
use neoc_vm::lua::std::fs::{
    error::{FsError, Kind},
    ops,
};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use vos_application::{AppFiles, FileStore, NoFiles};
use vos_domain::{
    files::MAX_READ_BYTES, AppId, FileError, FileErrorKind, FileInfo, FileKind, FilePath, FileResult,
    Location,
};

/// The prefix of the temporary files a write or a copy makes next to its destination (left behind only if the
/// host dies part way).
const TEMP_PREFIX: &str = ".vos-tmp-";

/// Every app's managed files under one directory (`None`: a host without a data directory keeps none, and every
/// call is `unsupported`).
pub struct DiskFiles {
    root: Option<PathBuf>,
}

impl DiskFiles {
    #[must_use]
    pub fn new(root: Option<PathBuf>) -> Self {
        Self { root }
    }

    fn app_root(&self, app: &AppId) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join(format!("{app}.files")))
    }
}

#[async_trait]
impl FileStore for DiskFiles {
    fn files(&self, app: &AppId) -> Arc<dyn AppFiles> {
        match self.app_root(app) {
            Some(root) => Arc::new(AppDisk { root }),
            None => Arc::new(NoFiles),
        }
    }

    async fn purge(&self, app: &AppId) -> FileResult<()> {
        let Some(root) = self.app_root(app) else { return Ok(()) };
        blocking(move |cancel| purge(&root, cancel)).await
    }
}

/// One app's files: `root` is `<root>/<app>.files`.
struct AppDisk {
    root: PathBuf,
}

impl AppDisk {
    /// Run `job` on the location's directory (made if missing) on the blocking pool.
    async fn at<T: Send + 'static>(
        &self,
        at: Location,
        job: impl FnOnce(&Path, &AtomicBool) -> FileResult<T> + Send + 'static,
    ) -> FileResult<T> {
        let dir = self.root.join(at.as_str());
        blocking(move |cancel| {
            fs::create_dir_all(&dir).map_err(|e| io_error(&e, "the location"))?;
            job(&dir, cancel)
        })
        .await
    }
}

#[async_trait]
impl AppFiles for AppDisk {
    async fn read(&self, at: Location, path: &FilePath) -> FileResult<Vec<u8>> {
        let path = path.clone();
        self.at(at, move |base, _| read(base, &path)).await
    }

    async fn write(&self, at: Location, path: &FilePath, data: Vec<u8>) -> FileResult<()> {
        let path = path.clone();
        self.at(at, move |base, cancel| write(base, &path, &data, cancel)).await
    }

    async fn list(&self, at: Location, path: &FilePath) -> FileResult<Vec<FileInfo>> {
        let path = path.clone();
        self.at(at, move |base, _| list(base, &path)).await
    }

    async fn info(&self, at: Location, path: &FilePath) -> FileResult<FileInfo> {
        let path = path.clone();
        self.at(at, move |base, _| info(base, at, &path)).await
    }

    async fn create_dir(&self, at: Location, path: &FilePath) -> FileResult<()> {
        let path = path.clone();
        self.at(at, move |base, _| create_dir(base, &path)).await
    }

    async fn remove(&self, at: Location, path: &FilePath, recursive: bool) -> FileResult<()> {
        let path = path.clone();
        self.at(at, move |base, cancel| remove(base, &path, recursive, cancel)).await
    }

    async fn move_to(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()> {
        let (from, to) = (from.clone(), to.clone());
        self.at(at, move |base, cancel| move_to(base, &from, &to, cancel)).await
    }

    async fn copy(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()> {
        let (from, to) = (from.clone(), to.clone());
        self.at(at, move |base, cancel| copy(base, &from, &to, cancel)).await
    }
}

// ---- the operations (blocking) --------------------------------------------------------------------------------

fn read(base: &Path, path: &FilePath) -> FileResult<Vec<u8>> {
    let full = resolve(base, path, Parents::MustExist)?;
    let seen = existing(&full, path)?;
    refuse_link(&seen, path)?;
    if seen.is_dir() {
        return Err(error(FileErrorKind::IsDirectory, path, "is a directory"));
    }
    if !seen.is_file() {
        return Err(error(FileErrorKind::InvalidInput, path, "not a regular file"));
    }
    let file = fs::File::open(&full).map_err(|e| io_error(&e, path))?;
    let opened = file.metadata().map_err(|e| io_error(&e, path))?;
    if !same_entry(&seen, &opened) {
        return Err(error(
            FileErrorKind::InvalidInput,
            path,
            "it changed while it was being opened (a symbolic link is not followed)",
        ));
    }
    if opened.len() > MAX_READ_BYTES {
        return Err(too_large(path));
    }
    // sized by what the file said, never past the cap; read one byte more than the cap to see a file that grew
    let mut data = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.take(MAX_READ_BYTES + 1).read_to_end(&mut data).map_err(|e| io_error(&e, path))?;
    if data.len() as u64 > MAX_READ_BYTES {
        return Err(too_large(path));
    }
    Ok(data)
}

fn write(base: &Path, path: &FilePath, data: &[u8], cancel: &AtomicBool) -> FileResult<()> {
    let full = resolve(base, path, Parents::MustExist)?;
    if let Some(seen) = lstat(&full, path)? {
        replaceable(&seen, path)?;
    }
    let mut temp = temp_beside(&full, path)?;
    temp.write_all(data).map_err(|e| io_error(&e, path))?;
    temp.as_file().sync_all().map_err(|e| io_error(&e, path))?;
    check_cancel(cancel)?;
    temp.persist(&full).map_err(|e| io_error(&e.error, path))?;
    Ok(())
}

fn list(base: &Path, path: &FilePath) -> FileResult<Vec<FileInfo>> {
    let full = resolve(base, path, Parents::MustExist)?;
    if !path.is_root() {
        let seen = existing(&full, path)?;
        refuse_link(&seen, path)?;
        if !seen.is_dir() {
            return Err(error(FileErrorKind::NotADirectory, path, "not a directory"));
        }
    }
    let entries = ops::read_dir(&full).map_err(|e| fs_error(&e, path))?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry.name.to_string_lossy().into_owned();
        match ops::metadata(&entry.path, false) {
            Ok(meta) => out.push(file_info(name, &meta)),
            // removed since the directory was read: it is not there any more
            Err(e) if e.kind() == Kind::NotFound => {}
            Err(e) => return Err(fs_error(&e, &format!("{}/{name}", shown(path)))),
        }
    }
    Ok(out)
}

fn info(base: &Path, at: Location, path: &FilePath) -> FileResult<FileInfo> {
    let full = resolve(base, path, Parents::MustExist)?;
    // the location itself is the host's (a link there is followed); a name inside it is described as it is
    let meta = ops::metadata(&full, path.is_root()).map_err(|e| fs_error(&e, path))?;
    Ok(file_info(path.name().unwrap_or(at.as_str()).to_owned(), &meta))
}

fn create_dir(base: &Path, path: &FilePath) -> FileResult<()> {
    let full = resolve(base, path, Parents::MayBeMissing)?;
    if path.is_root() {
        return Ok(());
    }
    match lstat(&full, path)? {
        Some(seen) if seen.file_type().is_symlink() => Err(link_refused(path)),
        Some(seen) if seen.is_dir() => Ok(()),
        Some(_) => Err(error(FileErrorKind::AlreadyExists, path, "a file is in the way")),
        None => fs::create_dir_all(&full).map_err(|e| io_error(&e, path)),
    }
}

fn remove(base: &Path, path: &FilePath, recursive: bool, cancel: &AtomicBool) -> FileResult<()> {
    let full = resolve(base, path, Parents::MustExist)?;
    let seen = existing(&full, path)?;
    let removed = if seen.is_dir() && recursive {
        ops::remove_dir_all(&full, false, cancel)
    } else if seen.is_dir() {
        ops::remove_dir(&full, false)
    } else {
        // a file, or a link (the link itself, never what it points at)
        ops::remove_file(&full, false)
    };
    removed.map(drop).map_err(|e| fs_error(&e, path))
}

fn move_to(base: &Path, from: &FilePath, to: &FilePath, cancel: &AtomicBool) -> FileResult<()> {
    let (src, dst) = (resolve(base, from, Parents::MustExist)?, resolve(base, to, Parents::MustExist)?);
    refuse_link(&existing(&src, from)?, from)?;
    if let Some(seen) = lstat(&dst, to)? {
        refuse_link(&seen, to)?;
    }
    ops::move_path(&src, &dst, cancel).map_err(|e| fs_error(&e, &pair(from, to)))
}

fn copy(base: &Path, from: &FilePath, to: &FilePath, cancel: &AtomicBool) -> FileResult<()> {
    let (src, dst) = (resolve(base, from, Parents::MustExist)?, resolve(base, to, Parents::MustExist)?);
    let source = existing(&src, from)?;
    refuse_link(&source, from)?;
    let destination = lstat(&dst, to)?;
    if let Some(seen) = &destination {
        refuse_link(seen, to)?;
    }
    if source.is_dir() {
        // the destination must be new (`already_exists` otherwise): a tree is never merged into another
        return ops::copy_dir(&src, &dst, cancel).map(drop).map_err(|e| fs_error(&e, &pair(from, to)));
    }
    if !source.is_file() {
        return Err(error(FileErrorKind::InvalidInput, from, "not a regular file or directory"));
    }
    if let Some(seen) = &destination {
        replaceable(seen, to)?;
    }
    let temp = temp_beside(&dst, to)?.into_temp_path();
    ops::copy_file(&src, &temp).map_err(|e| fs_error(&e, &pair(from, to)))?;
    check_cancel(cancel)?;
    temp.persist(&dst).map_err(|e| io_error(&e.error, to))?;
    Ok(())
}

/// Remove an app's whole file tree: a real directory with `std::fs::remove_dir_all` (through `std:fs`), a link
/// (an operator's) as the link only.
fn purge(root: &Path, cancel: &AtomicBool) -> FileResult<()> {
    const WHAT: &str = "the app's files";
    let removed = match fs::symlink_metadata(root) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_error(&e, WHAT)),
        Ok(seen) if seen.is_dir() => ops::remove_dir_all(root, true, cancel),
        Ok(_) => ops::remove_file(root, true),
    };
    removed.map(drop).map_err(|e| fs_error(&e, WHAT))
}

// ---- resolving a path ---------------------------------------------------------------------------------------

/// Whether the directories on the way to the last name must all exist.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Parents {
    MustExist,
    /// `create_dir`: the first missing one ends the checks (nothing below it exists to be a link).
    MayBeMissing,
}

/// `path` under the location directory `base`. Each existing directory on the way to the last name is looked
/// at without following links: a link is refused, a file is `not_a_directory`, a missing one `not_found`
/// (unless `parents` says it may be missing). The last name is not looked at: each operation does that.
fn resolve(base: &Path, path: &FilePath, parents: Parents) -> FileResult<PathBuf> {
    let mut full = base.to_path_buf();
    let Some((last, above)) = path.components().split_last() else { return Ok(full) };
    let mut walked: Vec<&str> = Vec::with_capacity(above.len());
    let mut checking = true;
    for name in above {
        push_name(&mut full, name)?;
        walked.push(name);
        if !checking {
            continue;
        }
        let here = walked.join("/");
        match fs::symlink_metadata(&full) {
            Ok(seen) if seen.file_type().is_symlink() => return Err(link_refused(&here)),
            Ok(seen) if seen.is_dir() => {}
            Ok(_) => return Err(error(FileErrorKind::NotADirectory, &here, "not a directory")),
            Err(e) if e.kind() == io::ErrorKind::NotFound && parents == Parents::MayBeMissing => {
                checking = false;
            }
            Err(e) => return Err(io_error(&e, &here)),
        }
    }
    push_name(&mut full, last)?;
    Ok(full)
}

/// Append one name, refusing anything the platform reads as more than a plain name (the domain has refused
/// `/`, `\`, `:` and `..` already: this holds even if that list misses a platform's own syntax).
fn push_name(full: &mut PathBuf, name: &str) -> FileResult<()> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) => {
            full.push(name);
            Ok(())
        }
        _ => Err(FileError::new(FileErrorKind::InvalidInput, "a name must be a plain name")),
    }
}

/// What is at `full`, not following a link; `None` when nothing is.
fn lstat(full: &Path, path: &FilePath) -> FileResult<Option<fs::Metadata>> {
    match fs::symlink_metadata(full) {
        Ok(seen) => Ok(Some(seen)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(&e, path)),
    }
}

fn existing(full: &Path, path: &FilePath) -> FileResult<fs::Metadata> {
    lstat(full, path)?.ok_or_else(|| error(FileErrorKind::NotFound, path, "no such file or directory"))
}

fn refuse_link(seen: &fs::Metadata, path: &FilePath) -> FileResult<()> {
    if seen.file_type().is_symlink() {
        return Err(link_refused(path));
    }
    Ok(())
}

/// What a write or a file copy may replace: a regular file.
fn replaceable(seen: &fs::Metadata, path: &FilePath) -> FileResult<()> {
    refuse_link(seen, path)?;
    if seen.is_dir() {
        return Err(error(FileErrorKind::IsDirectory, path, "is a directory"));
    }
    if !seen.is_file() {
        return Err(error(FileErrorKind::InvalidInput, path, "not a regular file"));
    }
    Ok(())
}

/// A new temporary file next to `full` (same directory, so the rename that puts it in place stays on one
/// filesystem), created exclusively under a random name by `tempfile`.
fn temp_beside(full: &Path, path: &FilePath) -> FileResult<tempfile::NamedTempFile> {
    let dir = full.parent().ok_or_else(|| error(FileErrorKind::InvalidInput, path, "has no parent"))?;
    tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile_in(dir).map_err(|e| io_error(&e, path))
}

/// The file that was opened is the one that was looked at.
#[cfg(unix)]
fn same_entry(seen: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    seen.dev() == opened.dev() && seen.ino() == opened.ino()
}

/// Windows has no stable file id in `std`: only the kind is compared (the swap check is Unix-only).
#[cfg(not(unix))]
fn same_entry(_: &fs::Metadata, opened: &fs::Metadata) -> bool {
    opened.is_file()
}

fn check_cancel(cancel: &AtomicBool) -> FileResult<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(FileError::new(FileErrorKind::Other, "cancelled: the call was stopped"));
    }
    Ok(())
}

/// Run `job` on the blocking pool; if the waiting call is dropped, the job's cancel flag is set.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce(&AtomicBool) -> FileResult<T> + Send + 'static,
) -> FileResult<T> {
    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let _guard = CancelOnDrop(cancel.clone());
    tokio::task::spawn_blocking(move || job(&cancel))
        .await
        .unwrap_or_else(|e| Err(FileError::new(FileErrorKind::Other, format!("the file job failed: {e}"))))
}

// ---- what the app sees ----------------------------------------------------------------------------------------

fn file_info(name: String, meta: &ops::Meta) -> FileInfo {
    let kind = match meta.kind {
        ops::FileKind::File => FileKind::File,
        ops::FileKind::Dir => FileKind::Dir,
        ops::FileKind::Symlink => FileKind::Symlink,
        ops::FileKind::Other => FileKind::Other,
    };
    FileInfo { name, kind, size: meta.size, modified: meta.modified }
}

/// The path as the app wrote it (`.` for the location itself): what every message names.
fn shown(path: &FilePath) -> String {
    if path.is_root() {
        ".".to_owned()
    } else {
        path.to_string()
    }
}

fn pair(from: &FilePath, to: &FilePath) -> String {
    format!("{} -> {}", shown(from), shown(to))
}

/// Something an error message can name: an app path, or a fixed description.
trait Subject {
    fn subject(&self) -> String;
}
impl Subject for FilePath {
    fn subject(&self) -> String {
        shown(self)
    }
}
impl Subject for str {
    fn subject(&self) -> String {
        self.to_owned()
    }
}
impl Subject for String {
    fn subject(&self) -> String {
        self.clone()
    }
}

fn error(kind: FileErrorKind, about: &(impl Subject + ?Sized), message: &str) -> FileError {
    FileError::new(kind, format!("{}: {message}", about.subject()))
}

fn link_refused(about: &(impl Subject + ?Sized)) -> FileError {
    error(
        FileErrorKind::InvalidInput,
        about,
        "is a symbolic link: links inside an app's files are not followed",
    )
}

fn too_large(path: &FilePath) -> FileError {
    error(
        FileErrorKind::TooLarge,
        path,
        &format!("larger than {MAX_READ_BYTES} bytes, the most read returns"),
    )
}

/// An I/O error, classified with `std:fs`'s kinds; its message is the platform's (no path in it).
fn io_error(e: &io::Error, about: &(impl Subject + ?Sized)) -> FileError {
    error(kind_of(Kind::from(e.kind())), about, &e.to_string())
}

/// A `std:fs` operation's error: its kind, its message without the host path it carries.
fn fs_error(e: &FsError, about: &(impl Subject + ?Sized)) -> FileError {
    error(kind_of(e.kind()), about, e.message())
}

fn kind_of(kind: Kind) -> FileErrorKind {
    FileErrorKind::parse(kind.as_str()).unwrap_or(FileErrorKind::Other)
}
