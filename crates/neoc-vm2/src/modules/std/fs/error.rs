//! The error of `std:fs`: a stable kind, the path (or the two paths) it is about, and the platform's own words.
//!
//! A script gets `{ kind, path?, to?, reason, message, op }`. The `kind` means the same on every platform, so a
//! script branches on it (`err.kind == "not_found"`); `reason` is the operating system's wording and differs between
//! platforms, so a script must not parse it. `message` reads `<path>: <reason>`, `<path> -> <to>: <reason>` for an
//! operation on two paths, or `<reason>` alone when no path is involved.
//!
//! Ported from the old VM's `std/fs/error.rs`: the error was one string (`fs.<function>: <kind>: <path>: <message>`)
//! read back with `fs.error_kind` and `fs.error_message`; it is a typed value now, so those two functions are gone.

use std::fmt;
use std::io;
use std::path::Path;

use mlua_ir::LuaError;
use serde::Serialize;

/// The one list of kinds: [`Kind`] (what the operations classify with) and [`FsError`] (what a script gets) are
/// both written from it, so neither can have a kind the other lacks.
macro_rules! kinds {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        /// What went wrong, stable across platforms.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Kind {
            $($(#[$doc])* $name,)*
        }

        impl Kind {
            /// Every kind, for the tests that hold them to each other.
            pub const ALL: &[Kind] = &[$(Kind::$name,)*];
        }

        /// Why a filesystem operation failed. The kind means the same on every platform; `reason` is the operating
        /// system's wording, for a person.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, LuaError)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        #[shape(module = "std:fs")]
        pub enum FsError {
            $(
                $(#[$doc])*
                $name {
                    /// The path the failure is about; none when it is about no path (an empty path, say).
                    path: Option<String>,
                    /// The second path of an operation on two (`copy`, `rename`, `symlink`), when the failure is
                    /// about the pair.
                    to: Option<String>,
                    /// What the operating system said, or why the call was refused before reaching it.
                    reason: String,
                },
            )*
        }

        impl FsError {
            fn build(kind: Kind, path: Option<String>, to: Option<String>, reason: String) -> Self {
                match kind {
                    $(Kind::$name => FsError::$name { path, to, reason },)*
                }
            }

            /// The error's kind.
            pub fn kind(&self) -> Kind {
                match self {
                    $(FsError::$name { .. } => Kind::$name,)*
                }
            }

            fn parts(&self) -> (Option<&str>, Option<&str>, &str) {
                match self {
                    $(FsError::$name { path, to, reason } => (path.as_deref(), to.as_deref(), reason),)*
                }
            }
        }
    };
}

kinds! {
    /// The path does not exist.
    NotFound,
    /// The process may not do this.
    PermissionDenied,
    /// What was to be made is already there.
    AlreadyExists,
    /// The directory has entries.
    NotEmpty,
    /// The path is a directory where a file was needed.
    IsDirectory,
    /// The path, or a parent of it, is not a directory.
    NotADirectory,
    /// The two paths are on different filesystems, and a rename cannot cross them (`move` can).
    CrossDevice,
    /// The path or an argument is not one the operation takes: a NUL byte, an empty path, a file copied onto itself.
    InvalidInput,
    /// The filesystem is mounted read-only.
    ReadOnlyFilesystem,
    /// The disk or the quota is full.
    StorageFull,
    /// The platform cannot do this (Unix permission bits on Windows).
    Unsupported,
    /// More than fits: a listing past its bound, a file past the filesystem's limit.
    TooLarge,
    /// Nothing happened within the time allowed.
    TimedOut,
    /// Any other failure.
    Other,
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
            io::ErrorKind::TimedOut => Kind::TimedOut,
            // `io::ErrorKind` is non-exhaustive: anything else is reported, never hidden, as `other`.
            _ => Kind::Other,
        }
    }
}

/// The result of an operation of this module.
pub type FsResult<T> = Result<T, FsError>;

fn shown(path: &Path) -> Option<String> {
    Some(path.display().to_string())
}

impl FsError {
    /// An error of `kind` that is not about one path (an empty path, say).
    pub fn new(kind: Kind, reason: impl Into<String>) -> Self {
        Self::build(kind, None, None, reason.into())
    }

    /// An error of `kind` about `path`.
    pub fn at(kind: Kind, path: &Path, reason: impl Into<String>) -> Self {
        Self::build(kind, shown(path), None, reason.into())
    }

    /// An I/O failure on `path`, classified.
    pub fn io(e: &io::Error, path: &Path) -> Self {
        Self::at(Kind::from(e.kind()), path, e.to_string())
    }

    /// An I/O failure of an operation on two paths (`copy`, `rename`).
    pub fn io2(e: &io::Error, from: &Path, to: &Path) -> Self {
        Self::build(Kind::from(e.kind()), shown(from), shown(to), e.to_string())
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.parts() {
            (Some(path), Some(to), reason) => write!(f, "{path} -> {to}: {reason}"),
            (Some(path), None, reason) => write!(f, "{path}: {reason}"),
            (None, _, reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for FsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_snake_case_name() {
        let names: Vec<&str> =
            Kind::ALL.iter().map(|kind| LuaError::kind(&FsError::new(*kind, "x"))).collect();
        assert_eq!(
            names,
            [
                "not_found",
                "permission_denied",
                "already_exists",
                "not_empty",
                "is_directory",
                "not_a_directory",
                "cross_device",
                "invalid_input",
                "read_only_filesystem",
                "storage_full",
                "unsupported",
                "too_large",
                "timed_out",
                "other",
            ]
        );
        for kind in Kind::ALL {
            assert_eq!(FsError::new(*kind, "x").kind(), *kind);
        }
    }

    #[test]
    fn io_kinds_map_to_stable_kinds() {
        let cases = [
            (io::ErrorKind::NotFound, Kind::NotFound),
            (io::ErrorKind::PermissionDenied, Kind::PermissionDenied),
            (io::ErrorKind::AlreadyExists, Kind::AlreadyExists),
            (io::ErrorKind::DirectoryNotEmpty, Kind::NotEmpty),
            (io::ErrorKind::IsADirectory, Kind::IsDirectory),
            (io::ErrorKind::NotADirectory, Kind::NotADirectory),
            (io::ErrorKind::CrossesDevices, Kind::CrossDevice),
            (io::ErrorKind::InvalidInput, Kind::InvalidInput),
            (io::ErrorKind::ReadOnlyFilesystem, Kind::ReadOnlyFilesystem),
            (io::ErrorKind::StorageFull, Kind::StorageFull),
            (io::ErrorKind::Unsupported, Kind::Unsupported),
            (io::ErrorKind::TimedOut, Kind::TimedOut),
            (io::ErrorKind::InvalidFilename, Kind::InvalidInput),
            (io::ErrorKind::QuotaExceeded, Kind::StorageFull),
            (io::ErrorKind::FileTooLarge, Kind::TooLarge),
            (io::ErrorKind::AddrInUse, Kind::Other),
        ];
        for (io_kind, kind) in cases {
            assert_eq!(Kind::from(io_kind), kind, "{io_kind:?}");
        }
    }

    #[test]
    fn the_message_carries_the_paths_and_the_reason() {
        let e = FsError::at(Kind::NotFound, Path::new("/a/b"), "No such file or directory (os error 2)");
        assert_eq!(e.to_string(), "/a/b: No such file or directory (os error 2)");
        assert_eq!(FsError::new(Kind::InvalidInput, "the path is empty").to_string(), "the path is empty");
        let e = FsError::io2(&io::Error::from(io::ErrorKind::CrossesDevices), Path::new("a"), Path::new("b"));
        assert_eq!(e.kind(), Kind::CrossDevice);
        assert!(e.to_string().starts_with("a -> b: "), "{e}");
    }
}
