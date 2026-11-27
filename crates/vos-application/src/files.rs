//! The file ports and the use case over them: one app's managed files.
//!
//! An app never names a host path. It names a [`Location`] and a path inside it, which [`Files`] checks
//! ([`FilePath::parse`]) before anything reaches the port, and the adapter behind [`AppFiles`] decides where on
//! disk that is. Scoping is by application only: one shared root per app, not per user or device. It is a
//! convention that keeps apps out of each other's files by accident, not a sandbox (an app can reach any file
//! through `std:fs`).

use async_trait::async_trait;
use std::sync::Arc;
use vos_domain::{
    limits::{MAX_LIST_ENTRIES, MAX_UPLOAD_BYTES},
    AppId, FileError, FileErrorKind, FileInfo, FileKind, FilePath, FileResult, HostPath, Location,
};

/// One app's managed files, on validated paths. Errors carry a stable kind and never name a host path.
#[async_trait]
pub trait AppFiles: Send + Sync {
    /// The whole file. A directory is `is_directory`; a file over `MAX_READ_BYTES` is `too_large`.
    async fn read(&self, at: Location, path: &FilePath) -> FileResult<Vec<u8>>;
    /// Replace (or create) the file in one step. The parent directory must exist.
    async fn write(&self, at: Location, path: &FilePath, data: Vec<u8>) -> FileResult<()>;
    /// The entries of a directory, sorted by name.
    async fn list(&self, at: Location, path: &FilePath) -> FileResult<Vec<FileInfo>>;
    /// What `path` is (a symbolic link is described, not followed).
    async fn info(&self, at: Location, path: &FilePath) -> FileResult<FileInfo>;
    /// Create a directory and the missing ones above it. Already there is not an error.
    async fn create_dir(&self, at: Location, path: &FilePath) -> FileResult<()>;
    /// Remove a file, or a directory (`recursive`: with everything in it; otherwise it must be empty).
    async fn remove(&self, at: Location, path: &FilePath, recursive: bool) -> FileResult<()>;
    /// Rename or move a file or directory inside the location, replacing an existing destination file.
    async fn move_to(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()>;
    /// Copy a file (replacing an existing destination file) or a directory tree (to a new path) inside the
    /// location.
    async fn copy(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()>;
}

/// Finds an app's files and removes them when the person asks for the app's data to go.
#[async_trait]
pub trait FileStore: Send + Sync {
    /// The app's files (nothing is created until the app uses them).
    fn files(&self, app: &AppId) -> Arc<dyn AppFiles>;
    /// Delete every file of the app (uninstall with purge, delete data). Nothing there is not an error.
    async fn purge(&self, app: &AppId) -> FileResult<()>;
}

/// The use case behind `app.files`: the rules every app's file call keeps, whatever the adapter.
///
/// - A path is checked before the port sees it (relative, no `..`, no NUL, bounded: [`FilePath::parse`]).
/// - The location itself cannot be written, removed, moved or copied, nor be a destination.
/// - A directory cannot be moved or copied into itself, and a copy onto its own source is refused.
/// - A read-only handle ([`Files::read_only`]) refuses every write with `permission_denied` (a read-only call
///   runs through one).
#[derive(Clone)]
pub struct Files {
    port: Arc<dyn AppFiles>,
    read_only: bool,
}

impl Files {
    #[must_use]
    pub fn new(port: Arc<dyn AppFiles>) -> Self {
        Self { port, read_only: false }
    }

    /// The same files, with every write refused.
    #[must_use]
    pub fn read_only(&self) -> Self {
        Self { port: self.port.clone(), read_only: true }
    }

    pub async fn read(&self, at: Location, path: &str) -> FileResult<Vec<u8>> {
        let path = FilePath::parse(path)?;
        self.port.read(at, &path).await
    }

    pub async fn write(&self, at: Location, path: &str, data: Vec<u8>) -> FileResult<()> {
        let path = self.target(path, "written")?;
        self.port.write(at, &path, data).await
    }

    pub async fn list(&self, at: Location, path: &str) -> FileResult<Vec<FileInfo>> {
        let path = FilePath::parse(path)?;
        self.port.list(at, &path).await
    }

    pub async fn info(&self, at: Location, path: &str) -> FileResult<FileInfo> {
        let path = FilePath::parse(path)?;
        self.port.info(at, &path).await
    }

    pub async fn create_dir(&self, at: Location, path: &str) -> FileResult<()> {
        self.writable()?;
        let path = FilePath::parse(path)?;
        self.port.create_dir(at, &path).await
    }

    pub async fn remove(&self, at: Location, path: &str, recursive: bool) -> FileResult<()> {
        let path = self.target(path, "removed")?;
        self.port.remove(at, &path, recursive).await
    }

    pub async fn move_to(&self, at: Location, from: &str, to: &str) -> FileResult<()> {
        let (from, to) = self.pair(from, to, "moved")?;
        if to != from && to.is_within(&from) {
            return Err(invalid("a directory cannot be moved into itself"));
        }
        self.port.move_to(at, &from, &to).await
    }

    pub async fn copy(&self, at: Location, from: &str, to: &str) -> FileResult<()> {
        let (from, to) = self.pair(from, to, "copied")?;
        if to.is_within(&from) {
            return Err(invalid("a copy cannot be made onto its source or inside it"));
        }
        self.port.copy(at, &from, &to).await
    }

    fn writable(&self) -> FileResult<()> {
        if self.read_only {
            return Err(FileError::new(
                FileErrorKind::PermissionDenied,
                "a read-only call cannot change files",
            ));
        }
        Ok(())
    }

    /// A path a write names: not the location itself.
    fn target(&self, raw: &str, what: &str) -> FileResult<FilePath> {
        self.writable()?;
        let path = FilePath::parse(raw)?;
        if path.is_root() {
            return Err(invalid(&format!("the location itself cannot be {what}: name a path inside it")));
        }
        Ok(path)
    }

    fn pair(&self, from: &str, to: &str, what: &str) -> FileResult<(FilePath, FilePath)> {
        Ok((self.target(from, what)?, self.target(to, what)?))
    }
}

/// What a listing knows of one entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// On the computer a link is followed: the kind, size and time are those of what it leads to. A link that cannot
    /// be followed (and any link in an app's Documents) keeps the kind [`vos_domain::FileKind::Symlink`].
    pub info: FileInfo,
    /// The entry is a symbolic link.
    pub link: bool,
    /// Why it cannot be opened, when the listing already knows (a link that leads nowhere or in a circle).
    pub problem: Option<String>,
}

/// The entries of one directory, sorted by name, at most [`MAX_LIST_ENTRIES`] of them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Listing {
    pub entries: Vec<Listed>,
    /// The directory holds more than the cap: `entries` are some of them.
    pub truncated: bool,
}

/// The host's own file system, for the person browsing their machine ("This Computer"). Unlike [`AppFiles`] it is
/// confined to nothing: a [`HostPath`] is any absolute path, a symbolic link is followed as a file manager follows
/// one, and what the host's process may read, write, create and delete is what is allowed. The operating system's
/// refusals come back as they are (`permission_denied`, `not_found`, ...), about the one entry that was asked for.
#[async_trait]
pub trait HostFiles: Send + Sync {
    /// The home directory of the user running the host, where there is one.
    fn home(&self) -> Option<HostPath>;
    /// The entries of a directory, sorted by name, cut off at `limit` (the answer says when it was). The top lists
    /// the root directory, or on Windows the drives. A link among them that cannot be followed is an entry with a
    /// `problem`, not a failed listing.
    async fn list(&self, path: &HostPath, limit: usize) -> FileResult<Listing>;
    /// A whole regular file. A directory is `is_directory`; a file over `MAX_READ_BYTES` is `too_large`.
    async fn read(&self, path: &HostPath) -> FileResult<Vec<u8>>;
    /// Put a file in place, whole: written under a temporary name in the same directory and renamed, so a failure
    /// leaves nothing half-written. Without `replace` an existing entry is `already_exists` and is left as it was
    /// (decided by the rename itself, not by a look beforehand). The parent directory must exist.
    async fn write(&self, path: &HostPath, data: Vec<u8>, replace: bool) -> FileResult<()>;
    /// Make one directory. One that is there is `already_exists`; a missing parent is `not_found`.
    async fn create_dir(&self, path: &HostPath) -> FileResult<()>;
    /// Rename or move an entry. A destination that exists is `already_exists`: nothing is replaced.
    async fn rename(&self, from: &HostPath, to: &HostPath) -> FileResult<()>;
    /// Remove a file, a link (the link itself, never what it leads to) or a directory: an empty one, or with
    /// `recursive` everything in it. A directory that is not empty without `recursive` is `not_empty`.
    async fn remove(&self, path: &HostPath, recursive: bool) -> FileResult<()>;
}

/// A place the person browses from the host's own Files page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The host's own file system, all of it ("This Computer"). Paths are [`HostPath`]s.
    Computer,
    /// The Documents of an installed app. Paths are [`FilePath`]s inside it, with their rules.
    App(AppId),
}

/// What the person may do with the files of one [`Place`]: browse it, take a copy of a file, put a file in, make a
/// folder, rename and remove.
///
/// The two kinds of place keep different rules, which is why a path is parsed here and not before. An app's place is
/// its [`Location::Documents`] and nothing else, through [`Files`]: a path is relative to it, `..` and absolute
/// paths never reach the port, and no symbolic link is followed. The computer is the person's own machine: any
/// absolute path, links followed, and only the operating system says no.
#[derive(Clone)]
pub struct UserFiles(Kind);

#[derive(Clone)]
enum Kind {
    App(Files),
    Computer(Arc<dyn HostFiles>),
}

impl UserFiles {
    /// The Documents of an app, behind its file port.
    #[must_use]
    pub fn of_app(port: Arc<dyn AppFiles>) -> Self {
        Self(Kind::App(Files::new(port)))
    }

    /// The host's own file system.
    #[must_use]
    pub fn of_computer(port: Arc<dyn HostFiles>) -> Self {
        Self(Kind::Computer(port))
    }

    /// The entries of a directory (`""` is the top), sorted by name, at most [`MAX_LIST_ENTRIES`].
    pub async fn list(&self, path: &str) -> FileResult<Listing> {
        match &self.0 {
            Kind::Computer(host) => host.list(&HostPath::parse(path)?, MAX_LIST_ENTRIES).await,
            Kind::App(files) => {
                let mut all = files.list(Location::Documents, path).await?;
                let truncated = all.len() > MAX_LIST_ENTRIES;
                all.truncate(MAX_LIST_ENTRIES);
                let entries = all
                    .into_iter()
                    .map(|info| Listed { link: info.kind == FileKind::Symlink, info, problem: None })
                    .collect();
                Ok(Listing { entries, truncated })
            }
        }
    }

    /// A whole file.
    pub async fn read(&self, path: &str) -> FileResult<Vec<u8>> {
        match &self.0 {
            Kind::Computer(host) => host.read(&HostPath::parse(path)?).await,
            Kind::App(files) => files.read(Location::Documents, path).await,
        }
    }

    /// Remove a file, or a directory: an empty one, or with `recursive` everything in it (a directory that is not
    /// empty is `not_empty` otherwise, so that taking a whole tree is always asked for by name). The top of a place
    /// cannot be removed.
    pub async fn remove(&self, path: &str, recursive: bool) -> FileResult<()> {
        match &self.0 {
            Kind::Computer(host) => host.remove(&named(path, "removed")?, recursive).await,
            Kind::App(files) => files.remove(Location::Documents, path, recursive).await,
        }
    }

    /// Make a folder. One that is already there is `already_exists`: the person asked for a new folder, and is
    /// told the name is taken.
    pub async fn create_dir(&self, path: &str) -> FileResult<()> {
        match &self.0 {
            Kind::Computer(host) => host.create_dir(&named(path, "made")?).await,
            Kind::App(files) => {
                if FilePath::parse(path)?.is_root() {
                    return Err(invalid("a folder needs a name"));
                }
                if exists(files, path).await? {
                    return Err(taken(path));
                }
                files.create_dir(Location::Documents, path).await
            }
        }
    }

    /// Rename or move an entry inside the place. Nothing is replaced: a destination that exists is `already_exists`.
    pub async fn rename(&self, from: &str, to: &str) -> FileResult<()> {
        match &self.0 {
            Kind::Computer(host) => host.rename(&named(from, "renamed")?, &named(to, "renamed")?).await,
            Kind::App(files) => {
                if FilePath::parse(from)? != FilePath::parse(to)? && exists(files, to).await? {
                    return Err(taken(to));
                }
                files.move_to(Location::Documents, from, to).await
            }
        }
    }

    /// Put an uploaded file in place, whole (the adapter writes it under a temporary name beside the destination and
    /// renames it, so a failed upload leaves nothing half-written). At most [`MAX_UPLOAD_BYTES`]. An existing file is
    /// replaced only when `replace` says so: otherwise it is `already_exists` and nothing is written.
    pub async fn upload(&self, path: &str, data: Vec<u8>, replace: bool) -> FileResult<()> {
        if data.len() > MAX_UPLOAD_BYTES {
            return Err(FileError::new(
                FileErrorKind::TooLarge,
                format!("an upload may be at most {MAX_UPLOAD_BYTES} bytes"),
            ));
        }
        match &self.0 {
            Kind::Computer(host) => host.write(&named(path, "written")?, data, replace).await,
            Kind::App(files) => {
                if !replace && exists(files, path).await? {
                    return Err(taken(path));
                }
                files.write(Location::Documents, path, data).await
            }
        }
    }
}

/// A host path that names an entry: not the top of the file system.
fn named(raw: &str, what: &str) -> FileResult<HostPath> {
    let path = HostPath::parse(raw)?;
    if path.is_root() {
        return Err(invalid(&format!("the top of the file system cannot be {what}: name a path inside it")));
    }
    Ok(path)
}

async fn exists(files: &Files, path: &str) -> FileResult<bool> {
    match files.info(Location::Documents, path).await {
        Ok(_) => Ok(true),
        Err(e) if e.kind == FileErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn taken(path: &str) -> FileError {
    let name = path.rsplit('/').find(|part| !part.is_empty()).unwrap_or(path);
    FileError::new(FileErrorKind::AlreadyExists, format!("{name}: something with this name is already here"))
}

/// The use case behind the Files page (`host.files.*` and `/files/...`): finds the files of a [`Place`].
///
/// Security: the computer is deliberately unrestricted. The host is the person's own machine, and a paired device
/// can already install apps (arbitrary code as the user running the host, with `std:fs`) and run containers on it, so
/// reading and writing any file the host's process can gives it nothing it did not have. What is kept is that only a
/// paired device gets here: an unpaired client reaches none of the routes this is behind. An operator who wants no
/// file browsing of the machine turns it off (`host_files = false`), and only the apps' Documents are offered.
pub struct FilesService {
    store: Arc<dyn FileStore>,
    /// Whether an app is installed (its Documents are browsable whatever state it is in).
    installed: Box<dyn Fn(&AppId) -> bool + Send + Sync>,
    /// The host's own file system; `None` when browsing it is turned off.
    computer: Option<Arc<dyn HostFiles>>,
}

impl FilesService {
    #[must_use]
    pub fn new(
        store: Arc<dyn FileStore>,
        installed: impl Fn(&AppId) -> bool + Send + Sync + 'static,
        computer: Option<Arc<dyn HostFiles>>,
    ) -> Self {
        Self { store, installed: Box::new(installed), computer }
    }

    /// Whether the Files page may browse this host's own file system.
    #[must_use]
    pub fn shares_computer(&self) -> bool {
        self.computer.is_some()
    }

    /// The home directory of the user running the host, when the computer is browsable and there is one.
    #[must_use]
    pub fn home(&self) -> Option<HostPath> {
        self.computer.as_ref().and_then(|host| host.home())
    }

    /// The files of `place`: `not_found` for an app that is not installed, `unsupported` for the computer on a host
    /// where browsing it is turned off.
    pub fn open(&self, place: &Place) -> FileResult<UserFiles> {
        match place {
            Place::Computer => self.computer.clone().map(UserFiles::of_computer).ok_or_else(|| {
                FileError::new(
                    FileErrorKind::Unsupported,
                    "browsing this computer's files is turned off on this host (`host_files = false` in its settings)",
                )
            }),
            Place::App(app) if (self.installed)(app) => Ok(UserFiles::of_app(self.store.files(app))),
            Place::App(app) => {
                Err(FileError::new(FileErrorKind::NotFound, format!("app `{app}` is not installed")))
            }
        }
    }
}

fn invalid(message: &str) -> FileError {
    FileError::new(FileErrorKind::InvalidInput, message)
}

/// No files: what an app sees when the host keeps no data directory (and in tests of other things). Every call
/// is `unsupported`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFiles;

fn unavailable<T>() -> FileResult<T> {
    Err(FileError::new(
        FileErrorKind::Unsupported,
        "this host keeps no files for apps (it has no data directory)",
    ))
}

#[async_trait]
impl AppFiles for NoFiles {
    async fn read(&self, _: Location, _: &FilePath) -> FileResult<Vec<u8>> {
        unavailable()
    }
    async fn write(&self, _: Location, _: &FilePath, _: Vec<u8>) -> FileResult<()> {
        unavailable()
    }
    async fn list(&self, _: Location, _: &FilePath) -> FileResult<Vec<FileInfo>> {
        unavailable()
    }
    async fn info(&self, _: Location, _: &FilePath) -> FileResult<FileInfo> {
        unavailable()
    }
    async fn create_dir(&self, _: Location, _: &FilePath) -> FileResult<()> {
        unavailable()
    }
    async fn remove(&self, _: Location, _: &FilePath, _: bool) -> FileResult<()> {
        unavailable()
    }
    async fn move_to(&self, _: Location, _: &FilePath, _: &FilePath) -> FileResult<()> {
        unavailable()
    }
    async fn copy(&self, _: Location, _: &FilePath, _: &FilePath) -> FileResult<()> {
        unavailable()
    }
}

#[async_trait]
impl FileStore for NoFiles {
    fn files(&self, _: &AppId) -> Arc<dyn AppFiles> {
        Arc::new(NoFiles)
    }
    async fn purge(&self, _: &AppId) -> FileResult<()> {
        Ok(())
    }
}
