//! The error contract of `std:fs`: a stable `kind` plus the platform's own message and the failing path.
//!
//! Every fallible `std:fs` function returns `(nil, err)` where `err` is one string,
//! `fs.<function>: <kind>: <path>: <message>` (the path part is left out when no path is involved). The `kind` is
//! one of [`Kind`]'s names and means the same on every platform, so a script can branch on it
//! (`fs.error_kind(err) == "not_found"`); the message is the operating system's wording and differs between
//! platforms, so a script must not parse it.

use std::fmt;
use std::io;
use std::path::Path;

/// What went wrong, stable across platforms. The names are the wire contract (`fs.error_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
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

impl Kind {
    /// Every kind, for the docs and the tests that hold them to each other.
    pub const ALL: [Kind; 13] = [
        Kind::NotFound,
        Kind::PermissionDenied,
        Kind::AlreadyExists,
        Kind::NotEmpty,
        Kind::IsDirectory,
        Kind::NotADirectory,
        Kind::CrossDevice,
        Kind::InvalidInput,
        Kind::ReadOnlyFilesystem,
        Kind::StorageFull,
        Kind::Unsupported,
        Kind::TooLarge,
        Kind::Other,
    ];

    /// The stable name a script sees.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::NotFound => "not_found",
            Kind::PermissionDenied => "permission_denied",
            Kind::AlreadyExists => "already_exists",
            Kind::NotEmpty => "not_empty",
            Kind::IsDirectory => "is_directory",
            Kind::NotADirectory => "not_a_directory",
            Kind::CrossDevice => "cross_device",
            Kind::InvalidInput => "invalid_input",
            Kind::ReadOnlyFilesystem => "read_only_filesystem",
            Kind::StorageFull => "storage_full",
            Kind::Unsupported => "unsupported",
            Kind::TooLarge => "too_large",
            Kind::Other => "other",
        }
    }

    fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

impl From<io::ErrorKind> for Kind {
    fn from(kind: io::ErrorKind) -> Self {
        match kind {
            io::ErrorKind::NotFound => Kind::NotFound,
            io::ErrorKind::PermissionDenied => Kind::PermissionDenied,
            io::ErrorKind::AlreadyExists => Kind::AlreadyExists,
            io::ErrorKind::DirectoryNotEmpty => Kind::NotEmpty,
            io::ErrorKind::IsADirectory => Kind::IsDirectory,
            io::ErrorKind::NotADirectory => Kind::NotADirectory,
            io::ErrorKind::CrossesDevices => Kind::CrossDevice,
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidFilename => Kind::InvalidInput,
            io::ErrorKind::ReadOnlyFilesystem => Kind::ReadOnlyFilesystem,
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Kind::StorageFull,
            io::ErrorKind::Unsupported => Kind::Unsupported,
            io::ErrorKind::FileTooLarge => Kind::TooLarge,
            // `io::ErrorKind` is non-exhaustive: anything else is reported, never hidden, as `other`.
            _ => Kind::Other,
        }
    }
}

/// A failed filesystem operation: its [`Kind`], what it was about and the platform's message.
#[derive(Debug)]
pub struct FsError {
    kind: Kind,
    subject: Option<String>,
    message: String,
}

/// The result of an operation of this module.
pub type FsResult<T> = Result<T, FsError>;

impl FsError {
    /// An error of `kind` that is not about one path (a bad option, say).
    pub fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            subject: None,
            message: message.into(),
        }
    }

    /// An error of `kind` about `path`.
    pub fn at(kind: Kind, path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind,
            subject: Some(path.display().to_string()),
            message: message.into(),
        }
    }

    /// An I/O failure on `path`, classified.
    pub fn io(e: &io::Error, path: &Path) -> Self {
        Self::at(Kind::from(e.kind()), path, e.to_string())
    }

    /// An I/O failure of an operation on two paths (`copy`, `rename`): the subject reads `from -> to`.
    pub fn io2(e: &io::Error, from: &Path, to: &Path) -> Self {
        Self {
            kind: Kind::from(e.kind()),
            subject: Some(format!("{} -> {}", from.display(), to.display())),
            message: e.to_string(),
        }
    }

    /// The error's kind.
    pub fn kind(&self) -> Kind {
        self.kind
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.subject {
            Some(subject) => write!(f, "{}: {subject}: {}", self.kind.as_str(), self.message),
            None => write!(f, "{}: {}", self.kind.as_str(), self.message),
        }
    }
}

impl std::error::Error for FsError {}

/// The kind name inside an error string a `std:fs` function returned (`fs.read: not_found: ...`), or `None` when
/// the text is not one of ours.
pub fn kind_of(err: &str) -> Option<&'static str> {
    let rest = err.strip_prefix("fs.")?;
    let (function, rest) = rest.split_once(": ")?;
    if function.is_empty()
        || !function
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return None;
    }
    let name = rest.split_once(": ").map_or(rest, |(name, _)| name);
    Kind::parse(name).map(Kind::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_name_round_trips_and_is_snake_case() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
            assert!(
                kind.as_str()
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_')
            );
        }
    }

    #[test]
    fn io_kinds_map_to_stable_names() {
        let cases = [
            (io::ErrorKind::NotFound, "not_found"),
            (io::ErrorKind::PermissionDenied, "permission_denied"),
            (io::ErrorKind::AlreadyExists, "already_exists"),
            (io::ErrorKind::DirectoryNotEmpty, "not_empty"),
            (io::ErrorKind::IsADirectory, "is_directory"),
            (io::ErrorKind::NotADirectory, "not_a_directory"),
            (io::ErrorKind::CrossesDevices, "cross_device"),
            (io::ErrorKind::InvalidInput, "invalid_input"),
            (io::ErrorKind::ReadOnlyFilesystem, "read_only_filesystem"),
            (io::ErrorKind::StorageFull, "storage_full"),
            (io::ErrorKind::Unsupported, "unsupported"),
            (io::ErrorKind::TimedOut, "other"),
        ];
        for (io_kind, name) in cases {
            assert_eq!(Kind::from(io_kind).as_str(), name, "{io_kind:?}");
        }
    }

    #[test]
    fn display_carries_kind_subject_and_message() {
        let e = FsError::at(
            Kind::NotFound,
            Path::new("/a/b"),
            "No such file or directory (os error 2)",
        );
        assert_eq!(
            e.to_string(),
            "not_found: /a/b: No such file or directory (os error 2)"
        );
        let e = FsError::new(Kind::InvalidInput, "unknown option \"x\"");
        assert_eq!(e.to_string(), "invalid_input: unknown option \"x\"");
        let e = FsError::io2(
            &io::Error::from(io::ErrorKind::CrossesDevices),
            Path::new("a"),
            Path::new("b"),
        );
        assert!(e.to_string().starts_with("cross_device: a -> b: "), "{e}");
    }

    #[test]
    fn kind_of_reads_our_strings_only() {
        assert_eq!(
            kind_of("fs.read: not_found: /x: No such file"),
            Some("not_found")
        );
        assert_eq!(
            kind_of("fs.copy: invalid_input: bad option"),
            Some("invalid_input")
        );
        assert_eq!(kind_of("fs.read: nonsense: /x: boom"), None);
        assert_eq!(kind_of("not_found: /x"), None);
        assert_eq!(kind_of("fs.: not_found: x"), None);
        assert_eq!(kind_of("fs.Read: not_found: x"), None);
        assert_eq!(kind_of(""), None);
    }
}
