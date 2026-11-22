//! Driven adapter for the `HostFiles` port: the host machine's own file system, for the person browsing it from the
//! Files page ("This Computer").
//!
//! **Nothing is confined here, on purpose.** The host is the person's own machine: any absolute path is taken,
//! symbolic links are followed as a file manager follows them, and what the host's process may read, write, create
//! and delete is what is allowed. The operating system is the only one that refuses, and its refusal is passed on as
//! it is (`permission_denied`, `not_found`, ...) about the entry that was asked for. This is not the adapter of an
//! app's managed files (`files.rs`), whose paths are relative, checked, and never follow a link; the two share
//! nothing but the error type.
//!
//! - **Listing.** A link is followed for its kind, size and time, and marked as a link; one that cannot be followed
//!   (it leads nowhere, or in a circle) is still an entry, with the reason, not a failed listing. A directory is read
//!   up to the cap it is given and no further, so a directory of a million files costs the cap, not a million.
//! - **Writing.** A file is written under a temporary name in the destination's directory (`tempfile`: exclusively
//!   created, random name) and renamed into place when it is whole and flushed, so a failure leaves nothing
//!   half-written. Without `replace` the rename refuses an existing destination itself (`persist_noclobber`), so two
//!   uploads of one name cannot both win; with `replace` a file is replaced, a directory never.
//! - **Removing.** A link is removed as the link, never what it leads to (`remove_dir_all` does not follow one
//!   either). A directory that is not empty goes only when the call says `recursive`.
//! - **Windows.** The top (`/`) lists the drives that exist; `/C:/Users` is `C:\Users`. On Unix the top is `/`.
//!
//! Every operation runs on the blocking pool.

use async_trait::async_trait;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use vos_application::{HostFiles, Listed, Listing};
use vos_domain::{files::MAX_READ_BYTES, FileError, FileErrorKind, FileInfo, FileKind, FileResult, HostPath};

/// The prefix of the temporary file an upload is written to, next to its destination.
const TEMP_PREFIX: &str = ".vos-upload-";

/// This machine's file system.
pub struct LocalFiles {
    home: Option<HostPath>,
}

impl LocalFiles {
    /// `home` is the home directory of the user running the host (the Files page's shortcut), if there is one.
    #[must_use]
    pub fn new(home: Option<&Path>) -> Self {
        Self { home: home.and_then(wire_path) }
    }
}

/// A native absolute path as a [`HostPath`] (`/Users/me`, `/C:/Users/me`); `None` for one that is not absolute or
/// not Unicode.
fn wire_path(native: &Path) -> Option<HostPath> {
    if !native.is_absolute() {
        return None;
    }
    let text = native.to_str()?.replace('\\', "/");
    HostPath::parse(&format!("/{}", text.trim_start_matches('/'))).ok()
}

/// The platform's path for `path`. The top is the root directory (on Windows it has no path: the drives are listed).
fn native(path: &HostPath) -> PathBuf {
    let mut full = PathBuf::new();
    if cfg!(windows) {
        // `C:` then the rest: `C:\Users\me`
        let mut names = path.names().iter();
        if let Some(drive) = names.next() {
            full.push(format!("{drive}\\"));
        }
        full.extend(names);
    } else {
        full.push("/");
        full.extend(path.names());
    }
    full
}

async fn blocking<T: Send + 'static>(job: impl FnOnce() -> FileResult<T> + Send + 'static) -> FileResult<T> {
    tokio::task::spawn_blocking(job)
        .await
        .unwrap_or_else(|e| Err(FileError::new(FileErrorKind::Other, format!("the file job failed: {e}"))))
}

#[async_trait]
impl HostFiles for LocalFiles {
    fn home(&self) -> Option<HostPath> {
        self.home.clone()
    }

    async fn list(&self, path: &HostPath, limit: usize) -> FileResult<Listing> {
        let path = path.clone();
        blocking(move || list(&path, limit)).await
    }

    async fn read(&self, path: &HostPath) -> FileResult<Vec<u8>> {
        let path = path.clone();
        blocking(move || read(&path)).await
    }

    async fn write(&self, path: &HostPath, data: Vec<u8>, replace: bool) -> FileResult<()> {
        let path = path.clone();
        blocking(move || write(&path, &data, replace)).await
    }

    async fn create_dir(&self, path: &HostPath) -> FileResult<()> {
        let path = path.clone();
        blocking(move || fs::create_dir(native(&path)).map_err(|e| failed(&e, &path, "make this folder")))
            .await
    }

    async fn rename(&self, from: &HostPath, to: &HostPath) -> FileResult<()> {
        let (from, to) = (from.clone(), to.clone());
        blocking(move || rename(&from, &to)).await
    }

    async fn remove(&self, path: &HostPath, recursive: bool) -> FileResult<()> {
        let path = path.clone();
        blocking(move || remove(&path, recursive)).await
    }
}

// ---- the operations (blocking) ------------------------------------------------------------------------------

fn list(path: &HostPath, limit: usize) -> FileResult<Listing> {
    if cfg!(windows) && path.is_root() {
        return Ok(drives());
    }
    let dir = native(path);
    let reading = fs::read_dir(&dir).map_err(|e| failed(&e, path, "open this folder"))?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in reading {
        // an entry that cannot be read (it went while the directory was being read) is passed over
        let Ok(entry) = entry else { continue };
        if entries.len() == limit {
            truncated = true;
            break;
        }
        entries.push(listed(&entry));
    }
    entries.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    Ok(Listing { entries, truncated })
}

/// One entry of a directory: a link is followed for what it is, and says why when it cannot be.
fn listed(entry: &fs::DirEntry) -> Listed {
    let name = entry.file_name().to_string_lossy().into_owned();
    let own = fs::symlink_metadata(entry.path());
    let link = own.as_ref().is_ok_and(|m| m.file_type().is_symlink());
    let followed = if link { fs::metadata(entry.path()) } else { own };
    match followed {
        Ok(meta) => Listed { info: info(name, &meta), link, problem: None },
        Err(e) => {
            let problem = if !link {
                describe(kind_of(&e)).to_owned()
            } else if e.kind() == io::ErrorKind::NotFound {
                "The link leads to something that is not there.".to_owned()
            } else {
                format!("The link cannot be followed: {}.", describe(kind_of(&e)))
            };
            let kind = if link { FileKind::Symlink } else { FileKind::Other };
            Listed { info: FileInfo { name, kind, size: 0, modified: None }, link, problem: Some(problem) }
        }
    }
}

fn info(name: String, meta: &fs::Metadata) -> FileInfo {
    let kind = if meta.is_dir() {
        FileKind::Dir
    } else if meta.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64());
    FileInfo { name, kind, size: meta.len(), modified }
}

/// Windows: the drives that are there, as the directories of the top.
fn drives() -> Listing {
    let entries = ('A'..='Z')
        .filter(|letter| Path::new(&format!("{letter}:\\")).is_dir())
        .map(|letter| Listed {
            info: FileInfo { name: format!("{letter}:"), kind: FileKind::Dir, size: 0, modified: None },
            link: false,
            problem: None,
        })
        .collect();
    Listing { entries, truncated: false }
}

fn read(path: &HostPath) -> FileResult<Vec<u8>> {
    let full = native(path);
    let meta = fs::metadata(&full).map_err(|e| failed(&e, path, "open this file"))?;
    if meta.is_dir() {
        return Err(error(FileErrorKind::IsDirectory, path, "it is a folder, not a file"));
    }
    if !meta.is_file() {
        // a device or a pipe would be read for ever
        return Err(error(FileErrorKind::InvalidInput, path, "it is not a regular file"));
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(too_large(path));
    }
    let file = fs::File::open(&full).map_err(|e| failed(&e, path, "open this file"))?;
    let mut data = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    // one byte more than the cap, to see a file that grew meanwhile
    file.take(MAX_READ_BYTES + 1).read_to_end(&mut data).map_err(|e| failed(&e, path, "read this file"))?;
    if data.len() as u64 > MAX_READ_BYTES {
        return Err(too_large(path));
    }
    Ok(data)
}

fn write(path: &HostPath, data: &[u8], replace: bool) -> FileResult<()> {
    const DOING: &str = "save a file here";
    let full = native(path);
    let dir = full.parent().ok_or_else(|| error(FileErrorKind::InvalidInput, path, "it has no folder"))?;
    if replace && fs::metadata(&full).is_ok_and(|seen| seen.is_dir()) {
        return Err(error(FileErrorKind::IsDirectory, path, "a folder has this name"));
    }
    let mut temp =
        tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile_in(dir).map_err(|e| failed(&e, path, DOING))?;
    // from here a failure drops `temp`, which removes the temporary file
    temp.as_file_mut().write_all(data).map_err(|e| failed(&e, path, DOING))?;
    temp.as_file_mut().sync_all().map_err(|e| failed(&e, path, DOING))?;
    let put = if replace { temp.persist(&full) } else { temp.persist_noclobber(&full) };
    put.map(drop).map_err(|e| failed(&e.error, path, DOING))
}

fn rename(from: &HostPath, to: &HostPath) -> FileResult<()> {
    let (src, dst) = (native(from), native(to));
    if from != to && fs::symlink_metadata(&dst).is_ok() {
        return Err(error(FileErrorKind::AlreadyExists, to, "something with this name is already here"));
    }
    fs::rename(src, dst).map_err(|e| failed(&e, from, "rename this"))
}

fn remove(path: &HostPath, recursive: bool) -> FileResult<()> {
    const DOING: &str = "remove this";
    let full = native(path);
    // looked at as it is: a link is removed as a link, never what it leads to
    let seen = fs::symlink_metadata(&full).map_err(|e| failed(&e, path, DOING))?;
    let removed = if !seen.is_dir() {
        fs::remove_file(&full)
    } else if recursive {
        fs::remove_dir_all(&full)
    } else {
        fs::remove_dir(&full)
    };
    removed.map_err(|e| failed(&e, path, DOING))
}

// ---- errors ---------------------------------------------------------------------------------------------------

fn error(kind: FileErrorKind, path: &HostPath, message: &str) -> FileError {
    FileError::new(kind, format!("{}: {message}", path.name().unwrap_or("/")))
}

fn too_large(path: &HostPath) -> FileError {
    error(FileErrorKind::TooLarge, path, &format!("it is larger than {MAX_READ_BYTES} bytes"))
}

/// The operating system refused: its reason as a stable kind and a sentence for the person, naming the entry by
/// the name they asked for. A refusal for want of permission says what could not be done.
fn failed(e: &io::Error, path: &HostPath, doing: &str) -> FileError {
    let kind = kind_of(e);
    let message = if kind == FileErrorKind::PermissionDenied {
        format!("You do not have permission to {doing}.")
    } else {
        describe(kind).to_owned()
    };
    error(kind, path, &message)
}

fn kind_of(e: &io::Error) -> FileErrorKind {
    use io::ErrorKind as K;
    match e.kind() {
        K::NotFound => FileErrorKind::NotFound,
        K::PermissionDenied => FileErrorKind::PermissionDenied,
        K::AlreadyExists => FileErrorKind::AlreadyExists,
        K::DirectoryNotEmpty => FileErrorKind::NotEmpty,
        K::IsADirectory => FileErrorKind::IsDirectory,
        K::NotADirectory => FileErrorKind::NotADirectory,
        K::CrossesDevices => FileErrorKind::CrossDevice,
        K::InvalidInput | K::InvalidFilename => FileErrorKind::InvalidInput,
        K::ReadOnlyFilesystem => FileErrorKind::ReadOnlyFilesystem,
        K::StorageFull | K::QuotaExceeded => FileErrorKind::StorageFull,
        K::Unsupported => FileErrorKind::Unsupported,
        K::FileTooLarge => FileErrorKind::TooLarge,
        _ => FileErrorKind::Other,
    }
}

fn describe(kind: FileErrorKind) -> &'static str {
    match kind {
        FileErrorKind::NotFound => "It is not there.",
        FileErrorKind::PermissionDenied => "You do not have permission to open this.",
        FileErrorKind::AlreadyExists => "Something with this name is already here.",
        FileErrorKind::NotEmpty => "The folder is not empty.",
        FileErrorKind::IsDirectory => "It is a folder.",
        FileErrorKind::NotADirectory => "It is not a folder.",
        FileErrorKind::CrossDevice => "It cannot be moved to another disk this way.",
        FileErrorKind::InvalidInput => "That name cannot be used here.",
        FileErrorKind::ReadOnlyFilesystem => "This disk cannot be written to.",
        FileErrorKind::StorageFull => "The disk is full.",
        FileErrorKind::Unsupported => "This is not supported here.",
        FileErrorKind::TooLarge => "It is too large.",
        FileErrorKind::Other => {
            "The operating system refused (a link that leads in a circle, or another error)."
        }
    }
}
