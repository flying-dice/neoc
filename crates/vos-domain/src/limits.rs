//! What the host accepts: of an app package (the fetcher adapter bounds what it reads; the domain decides how much of
//! an app is acceptable) and of one message on the `/rpc` socket.

use crate::DomainError;

/// The largest single package file (the app ref itself, a `.lua` module or a migration).
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
/// The most bytes of files an install or load fetches for one app.
pub const MAX_PACKAGE_BYTES: usize = 8 * 1024 * 1024;
/// The most files an app ref may list.
pub const MAX_PACKAGE_FILES: usize = 256;
/// The largest message a client may send on the `/rpc` socket (1 MiB).
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;

/// The largest file a paired device may upload through the Files page (64 MiB: the same as the largest file that
/// can be read back, `files::MAX_READ_BYTES`, so whatever was uploaded can be downloaded).
pub const MAX_UPLOAD_BYTES: usize = 64 * 1024 * 1024;

/// The most entries of one directory a listing carries (`host.files.list`). A directory with more is cut off here and
/// the answer says so: a folder of a million files is not sent whole to a tablet.
pub const MAX_LIST_ENTRIES: usize = 2000;

/// Counts the bytes fetched for one package and refuses the file that tips it over a limit.
#[derive(Debug, Default)]
pub struct PackageBudget {
    used: usize,
}

impl PackageBudget {
    /// Account for a fetched file of `len` bytes.
    pub fn charge(&mut self, path: &str, len: usize) -> Result<(), DomainError> {
        if len > MAX_FILE_BYTES {
            return Err(DomainError::PackageTooLarge(format!(
                "`{path}` is {len} bytes, a file may be at most {MAX_FILE_BYTES}"
            )));
        }
        self.used = self.used.saturating_add(len);
        if self.used > MAX_PACKAGE_BYTES {
            return Err(DomainError::PackageTooLarge(format!(
                "the files fetched by `{path}` total {} bytes, a package may be at most {MAX_PACKAGE_BYTES}",
                self.used
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_over_the_cap_is_refused() {
        let mut b = PackageBudget::default();
        assert!(b.charge("a.lua", MAX_FILE_BYTES).is_ok());
        assert!(
            matches!(b.charge("b.lua", MAX_FILE_BYTES + 1), Err(DomainError::PackageTooLarge(m)) if m.contains("b.lua"))
        );
    }

    #[test]
    fn the_package_total_is_capped() {
        let mut b = PackageBudget::default();
        for i in 0..MAX_PACKAGE_BYTES / MAX_FILE_BYTES {
            b.charge(&format!("{i}.lua"), MAX_FILE_BYTES).unwrap();
        }
        assert!(
            matches!(b.charge("one-more.lua", 1), Err(DomainError::PackageTooLarge(m)) if m.contains("package"))
        );
    }
}
