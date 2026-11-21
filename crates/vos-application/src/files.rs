//! The file ports and the use case over them: one app's managed files.
//!
//! An app never names a host path. It names a [`Location`] and a path inside it, which [`Files`] checks
//! ([`FilePath::parse`]) before anything reaches the port, and the adapter behind [`AppFiles`] decides where on
//! disk that is. Scoping is by application only: one shared root per app, not per user or device. It is a
//! convention that keeps apps out of each other's files by accident, not a sandbox (an app can reach any file
//! through `std:fs`).

use async_trait::async_trait;
use std::sync::Arc;
use vos_domain::{AppId, FileError, FileErrorKind, FileInfo, FilePath, FileResult, Location};

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
/// - A read-only handle ([`Files::read_only`]) refuses every write with `permission_denied` (a `readOnly` tool
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
                "a readOnly tool cannot change files",
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
