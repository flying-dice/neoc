//! The vocabulary of an app's managed files: where they live, how a path inside them is written, what an
//! entry looks like and what can go wrong. Pure: the lexical checks that keep an app's paths inside its own
//! location (relative, no `..`, no NUL, no `:` or backslash, bounded) are here; resolving them on disk, and the
//! symlink checks that needs, is the adapter's business. Scoping by app is a convention that keeps apps from
//! stepping on each other's files by accident, not a sandbox: an app that wants another app's files can reach them
//! through `std:fs`.

use std::fmt;

/// The longest path an app may name (bytes, `/` included).
pub const MAX_PATH_BYTES: usize = 4096;
/// The longest single name in a path (bytes).
pub const MAX_NAME_BYTES: usize = 255;
/// The most bytes `read` returns: the whole file becomes one string in the app's VM.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

/// A managed location: one fixed directory per app, found from the app's identity (never from a host path).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Location {
    /// The app's own files: what the person would be upset to lose.
    Documents,
    /// Files the app can rebuild.
    Cache,
    /// Scratch files.
    Temp,
}

impl Location {
    pub const ALL: [Location; 3] = [Location::Documents, Location::Cache, Location::Temp];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Location::Documents => "documents",
            Location::Cache => "cache",
            Location::Temp => "temp",
        }
    }
}

/// A path inside a [`Location`]: relative, written with `/`, and never leaving it. The empty path (`""`, `"."`)
/// is the location itself. Empty and `.` components are dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePath(Vec<String>);

impl FilePath {
    /// Check `raw`. An absolute path, a backslash, a `:` (a drive or a stream on Windows, refused on every
    /// platform so a path means the same everywhere), a NUL, a `..` component (even one that would stay inside),
    /// a name over [`MAX_NAME_BYTES`] and a path over [`MAX_PATH_BYTES`] are `invalid_input`. The message never
    /// repeats the path.
    pub fn parse(raw: &str) -> Result<Self, FileError> {
        let refuse = |why: &str| Err(FileError::new(FileErrorKind::InvalidInput, why));
        if raw.len() > MAX_PATH_BYTES {
            return refuse(&format!("a path may be at most {MAX_PATH_BYTES} bytes"));
        }
        if raw.contains('\0') {
            return refuse("a path cannot contain a NUL byte");
        }
        if raw.starts_with('/') {
            return refuse("absolute paths are refused: a path is relative to its location");
        }
        if raw.contains('\\') {
            return refuse("write a path with `/`: a backslash is refused");
        }
        if raw.contains(':') {
            return refuse("`:` is refused in a path (on Windows it names a drive or a stream)");
        }
        let mut parts = Vec::new();
        for part in raw.split('/') {
            match part {
                "" | "." => {}
                ".." => return refuse("`..` is refused: a path cannot leave its location"),
                name if name.len() > MAX_NAME_BYTES => {
                    return refuse(&format!("a name may be at most {MAX_NAME_BYTES} bytes"))
                }
                name => parts.push(name.to_owned()),
            }
        }
        Ok(Self(parts))
    }

    /// The components, each a plain name.
    #[must_use]
    pub fn components(&self) -> &[String] {
        &self.0
    }

    /// The location itself.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The last component, or `None` for the location itself.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }

    /// `self` is `other` or somewhere under it (`a/b` is within `a`, `ab` is not).
    #[must_use]
    pub fn is_within(&self, other: &FilePath) -> bool {
        self.0.starts_with(&other.0)
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("/"))
    }
}

/// The longest host path taken, in bytes.
pub const MAX_HOST_PATH_BYTES: usize = 4096;

/// A path on the host's own file system, for the person browsing their machine ("This Computer"). It is a type of
/// its own, with rules of its own: unlike a [`FilePath`] it is absolute and is confined to nothing.
///
/// On the wire a host path always starts with `/` and uses `/` between names, on every platform: `/` is the top
/// (the root directory on Unix, the list of drives on Windows), `/Users/me` a directory, `/C:/Users/me` one on a
/// Windows drive. It is tidied when parsed: empty names and `.` are dropped and `..` steps up (never above the top),
/// so what is kept is the list of names from the top. The adapter turns that into the platform's own path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostPath(Vec<String>);

impl HostPath {
    /// The top: `/`.
    #[must_use]
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Parse a wire path. `""` is the top, like `/`.
    ///
    /// # Errors
    /// `invalid_input` for a path that does not start with `/`, holds a NUL byte, or is too long.
    pub fn parse(raw: &str) -> Result<Self, FileError> {
        let refuse = |why: &str| Err(FileError::new(FileErrorKind::InvalidInput, why));
        if raw.len() > MAX_HOST_PATH_BYTES {
            return refuse(&format!("a path may be at most {MAX_HOST_PATH_BYTES} bytes"));
        }
        if raw.contains('\0') {
            return refuse("a path cannot contain a NUL byte");
        }
        if !raw.is_empty() && !raw.starts_with('/') {
            return refuse("a path on this computer starts with `/`");
        }
        let mut names: Vec<String> = Vec::new();
        for name in raw.split('/') {
            match name {
                "" | "." => {}
                ".." => {
                    names.pop();
                }
                name if name.len() > MAX_NAME_BYTES => {
                    return refuse(&format!("a name may be at most {MAX_NAME_BYTES} bytes"));
                }
                name => names.push(name.to_owned()),
            }
        }
        Ok(Self(names))
    }

    /// The names from the top.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.0
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The last name; `None` for the top.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }
}

impl std::fmt::Display for HostPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "/{}", self.0.join("/"))
    }
}

/// What an entry is, without following a symbolic link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Other,
}

impl FileKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::File => "file",
            FileKind::Dir => "dir",
            FileKind::Symlink => "symlink",
            FileKind::Other => "other",
        }
    }
}

/// What the app may know of an entry. No host path.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    /// The last component (the location's own name for the location itself).
    pub name: String,
    pub kind: FileKind,
    /// Bytes (whatever the platform reports for a directory).
    pub size: u64,
    /// Seconds since the Unix epoch; `None` where the filesystem does not record it.
    pub modified: Option<f64>,
}

/// Why an operation failed. The names are stable and are the ones `std:fs` uses, so an app matches on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileErrorKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    NotEmpty,
    IsDirectory,
    NotADirectory,
    CrossDevice,
    InvalidInput,
    ReadOnlyFilesystem,
    StorageFull,
    Unsupported,
    TooLarge,
    Other,
}

impl FileErrorKind {
    pub const ALL: [FileErrorKind; 13] = [
        FileErrorKind::NotFound,
        FileErrorKind::PermissionDenied,
        FileErrorKind::AlreadyExists,
        FileErrorKind::NotEmpty,
        FileErrorKind::IsDirectory,
        FileErrorKind::NotADirectory,
        FileErrorKind::CrossDevice,
        FileErrorKind::InvalidInput,
        FileErrorKind::ReadOnlyFilesystem,
        FileErrorKind::StorageFull,
        FileErrorKind::Unsupported,
        FileErrorKind::TooLarge,
        FileErrorKind::Other,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FileErrorKind::NotFound => "not_found",
            FileErrorKind::PermissionDenied => "permission_denied",
            FileErrorKind::AlreadyExists => "already_exists",
            FileErrorKind::NotEmpty => "not_empty",
            FileErrorKind::IsDirectory => "is_directory",
            FileErrorKind::NotADirectory => "not_a_directory",
            FileErrorKind::CrossDevice => "cross_device",
            FileErrorKind::InvalidInput => "invalid_input",
            FileErrorKind::ReadOnlyFilesystem => "read_only_filesystem",
            FileErrorKind::StorageFull => "storage_full",
            FileErrorKind::Unsupported => "unsupported",
            FileErrorKind::TooLarge => "too_large",
            FileErrorKind::Other => "other",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A failed operation: a stable kind and a message that names no host path.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}: {message}", kind.as_str())]
pub struct FileError {
    pub kind: FileErrorKind,
    pub message: String,
}

impl FileError {
    pub fn new(kind: FileErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

pub type FileResult<T> = Result<T, FileError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(raw: &str) -> Vec<String> {
        FilePath::parse(raw).unwrap().components().to_vec()
    }

    #[test]
    fn a_relative_path_is_split_and_tidied() {
        assert_eq!(parts("notes/2026/a.txt"), ["notes", "2026", "a.txt"]);
        assert_eq!(parts("./notes//a.txt/"), ["notes", "a.txt"]);
        assert!(FilePath::parse("").unwrap().is_root());
        assert!(FilePath::parse(".").unwrap().is_root());
        assert_eq!(FilePath::parse("a/./b").unwrap().to_string(), "a/b");
    }

    #[test]
    fn hostile_paths_are_invalid_input() {
        let long_name = "a".repeat(MAX_NAME_BYTES + 1);
        let long_path = "a/".repeat(MAX_PATH_BYTES);
        for raw in [
            "/etc/passwd",
            "//server/share",
            "C:/Windows",
            "c:",
            "a/C:b",
            "file.txt:stream",
            "a\\b",
            "..\\x",
            "..",
            "a/../b",
            "../escape",
            "a/..",
            "a\0b",
            long_name.as_str(),
            long_path.as_str(),
        ] {
            let e = FilePath::parse(raw).unwrap_err();
            assert_eq!(e.kind, FileErrorKind::InvalidInput, "{raw:?}");
            assert!(!e.message.contains("passwd"), "the message does not echo the path: {e}");
        }
        assert!(FilePath::parse(&"a".repeat(MAX_NAME_BYTES)).is_ok());
    }

    #[test]
    fn within_compares_whole_components() {
        let p = |raw: &str| FilePath::parse(raw).unwrap();
        assert!(p("a/b").is_within(&p("a")));
        assert!(p("a").is_within(&p("a")));
        assert!(p("a").is_within(&p("")));
        assert!(!p("ab").is_within(&p("a")));
        assert!(!p("a").is_within(&p("a/b")));
        assert_eq!(p("a/b.txt").name(), Some("b.txt"));
        assert_eq!(p("").name(), None);
    }

    #[test]
    fn error_kind_names_round_trip() {
        for kind in FileErrorKind::ALL {
            assert_eq!(FileErrorKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(FileErrorKind::parse("nonsense"), None);
    }

    #[test]
    fn a_host_path_is_absolute_and_tidied_and_confined_to_nothing() {
        let names = |raw: &str| HostPath::parse(raw).unwrap().names().to_vec();
        assert_eq!(names("/Users/me/Documents"), ["Users", "me", "Documents"]);
        assert_eq!(names("/etc/passwd"), ["etc", "passwd"], "anywhere on the machine");
        assert!(HostPath::parse("/").unwrap().is_root() && HostPath::parse("").unwrap().is_root());
        assert_eq!(names("//Users/./me//"), ["Users", "me"]);
        assert_eq!(names("/Users/me/../you"), ["Users", "you"], "`..` steps up");
        assert_eq!(names("/../../etc"), ["etc"], "and never above the top");
        assert_eq!(names("/C:/Users/me"), ["C:", "Users", "me"], "a Windows drive is the first name");
        assert_eq!(names("/a b/été \"1\""), ["a b", "été \"1\""]);
        assert_eq!(HostPath::parse("/Users/me/").unwrap().to_string(), "/Users/me");
        assert_eq!(HostPath::root().to_string(), "/");
        assert_eq!(HostPath::parse("/a/b").unwrap().name(), Some("b"));
        for bad in ["relative/path", "~/x", "C:/Users", "/a\0b"] {
            assert_eq!(HostPath::parse(bad).unwrap_err().kind, FileErrorKind::InvalidInput, "{bad:?}");
        }
        assert!(HostPath::parse(&format!("/{}", "a".repeat(MAX_NAME_BYTES + 1))).is_err());
        assert!(HostPath::parse(&format!("/{}", "a/".repeat(MAX_HOST_PATH_BYTES))).is_err());
    }
}
