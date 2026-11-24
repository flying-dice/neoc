//! `app.files`: the app's managed files, found from the app's identity, one shared set per app (not per person
//! or device).
//!
//! ```lua
//! local docs = app.files.documents                 -- also app.files.cache and app.files.temp
//! local ok, err = docs:createDir("notes")          -- true | nil, err (and the missing directories above it)
//! ok, err = docs:write("notes/a.txt", "hello")     -- replaces the file in one step; the parent must exist
//! local text, err = docs:read("notes/a.txt")       -- the whole file, binary-safe
//! local entries = docs:list("notes")               -- { { name, kind, size, modified } }, sorted by name
//! local info = docs:info("notes/a.txt")            -- { name = "a.txt", kind = "file", size = 5, modified = ... }
//! docs:move("notes/a.txt", "notes/b.txt")          -- replaces an existing file at the destination
//! docs:copy("notes", "backup")                     -- a file, or a whole tree to a new path
//! docs:remove("backup", { recursive = true })
//! if err and err.kind == "not_found" then ... end  -- a typed error: `kind`, `message`, `op`
//! ```
//!
//! A path is relative to its location, written with `/`; the rules are the use case's (`vos_application::Files`)
//! and the adapter's (`crate::files`, which says how links and races are handled). A failure is returned as
//! `nil, err`, a [`FileError`] with the kinds of `std:fs`; a wrong argument type, an unknown option, and a write
//! inside a `readOnly` tool raise, as everywhere in `app.*`. A file call holds the app's turn while it runs
//! (`Role::Vm`, like `app.db`): the app's other calls wait for it, and none of them sees it half done. It does not
//! need `std:fs`: the host does the work.

use super::{shared, Sh};
use mlua_ir::{Bytes, LuaError, Object, Options, Shape};
use neoc_vm::mlua::{self, Lua};
use serde::{Deserialize, Serialize};
use std::future::Future;
use vos_application::Files as FilesPort;
use vos_domain::{
    FileErrorKind, FileInfo as DomainFileInfo, FileKind as DomainFileKind, FileResult, Location as At,
};

/// The one list of kinds: [`FileError`] is written from it and held to the domain's [`FileErrorKind`], so neither
/// can have a kind the other lacks.
macro_rules! kinds {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        /// Why a file call failed. The kinds are the ones `std:fs` uses and mean the same on every platform;
        /// `message` names the path the failure is about (never a path of the host) and is for a person.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        #[shape(module = "vos:app")]
        pub enum FileError {
            $(
                $(#[$doc])*
                #[error("{text}")]
                $name {
                    #[serde(skip)]
                    text: String,
                },
            )*
        }

        impl From<vos_domain::FileError> for FileError {
            fn from(error: vos_domain::FileError) -> Self {
                let text = error.message;
                match error.kind {
                    $(FileErrorKind::$name => FileError::$name { text },)*
                }
            }
        }
    };
}

kinds! {
    /// The path does not exist.
    NotFound,
    /// The host may not do this.
    PermissionDenied,
    /// What was to be made is already there.
    AlreadyExists,
    /// The directory has entries.
    NotEmpty,
    /// The path is a directory where a file was needed.
    IsDirectory,
    /// The path, or a parent of it, is not a directory.
    NotADirectory,
    /// The two paths are on different filesystems.
    CrossDevice,
    /// The path is not one a location takes: absolute, `..`, `:`, a backslash, NUL, or a name over 255 bytes.
    InvalidInput,
    /// The filesystem is mounted read-only.
    ReadOnlyFilesystem,
    /// The disk or the quota is full.
    StorageFull,
    /// The host has no file store for apps, or the platform cannot do this.
    Unsupported,
    /// More than fits: a file over 64 MiB.
    TooLarge,
    /// Any other failure.
    Other,
}

/// What an entry is; a symbolic link is never followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "vos:app")]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// An entry of `app.files`.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct FileInfo {
    /// The last component of its path.
    pub name: String,
    /// What it is; a symbolic link is not followed.
    pub kind: FileKind,
    /// Its size in bytes.
    pub size: u64,
    /// When it was last modified, in seconds since the Unix epoch; `nil` where the filesystem does not record it.
    pub modified: Option<f64>,
}

impl From<DomainFileInfo> for FileInfo {
    fn from(info: DomainFileInfo) -> Self {
        let kind = match info.kind {
            DomainFileKind::File => FileKind::File,
            DomainFileKind::Dir => FileKind::Dir,
            DomainFileKind::Symlink => FileKind::Symlink,
            DomainFileKind::Other => FileKind::Other,
        };
        Self { name: info.name, kind, size: info.size, modified: info.modified }
    }
}

/// How `remove` treats a directory.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vos:app")]
pub struct RemoveOptions {
    /// Remove a directory with everything in it.
    pub recursive: bool,
}

/// The app's managed files: one shared set per app (not per person or device), found from the app's identity.
/// `documents` for what the person would miss, `cache` for what the app can rebuild, `temp` for scratch files.
/// All three are deleted when the app's data is deleted (uninstall with purge, "delete data"), and kept otherwise.
#[derive(Clone, Copy, Debug, Object)]
#[shape(module = "vos:app")]
pub struct Files;

#[mlua_ir::object(module = "vos:app")]
impl Files {
    /// What the person would miss.
    #[get]
    pub fn documents(&self) -> Location {
        Location { at: At::Documents }
    }

    /// What the app can rebuild.
    #[get]
    pub fn cache(&self) -> Location {
        Location { at: At::Cache }
    }

    /// Scratch files.
    #[get]
    pub fn temp(&self) -> Location {
        Location { at: At::Temp }
    }
}

/// One managed location of the app (`app.files.documents`, `.cache`, `.temp`). A path is relative to it, written
/// with `/` (`"notes/a.txt"`; `""` is the location itself); an absolute path, `..`, `:`, a backslash, NUL or a name
/// over 255 bytes is `invalid_input`, and a symbolic link inside it is never followed. A failure is returned as
/// `nil, err` with the kinds of `std:fs` (`err.kind`). A write inside a `readOnly` tool raises. A call holds the
/// app's turn until it ends. Scoped to the app by convention: not a sandbox.
#[derive(Clone, Copy, Debug, Object)]
#[shape(module = "vos:app")]
pub struct Location {
    at: At,
}

#[mlua_ir::object(module = "vos:app")]
#[allow(non_snake_case)] // `createDir` is the script's name
impl Location {
    /// The whole file (binary-safe); at most 64 MiB (`too_large`). A directory is `is_directory`.
    #[host("Vm")]
    pub async fn read(&self, lua: Lua, path: String) -> mlua::Result<Result<Bytes, FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "read", false, |f| async move { f.read(at, &path).await.map(Bytes) }).await
    }

    /// Create or replace the file in one step (a reader never sees half of it). The parent must exist (`not_found`).
    #[host("Vm")]
    pub async fn write(&self, lua: Lua, path: String, data: Bytes) -> mlua::Result<Result<(), FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "write", true, |f| async move { f.write(at, &path, data.0).await }).await
    }

    /// The entries of a directory (default: the location), sorted by name.
    #[host("Vm")]
    pub async fn list(
        &self,
        lua: Lua,
        path: Option<String>,
    ) -> mlua::Result<Result<Vec<FileInfo>, FileError>> {
        let (at, path) = (self.at, path.unwrap_or_default());
        call(&shared(&lua)?, "list", false, |f| async move {
            Ok(f.list(at, &path).await?.into_iter().map(FileInfo::from).collect())
        })
        .await
    }

    /// What `path` is: type, size, modification time.
    #[host("Vm")]
    pub async fn info(&self, lua: Lua, path: String) -> mlua::Result<Result<FileInfo, FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "info", false, |f| async move { f.info(at, &path).await.map(FileInfo::from) })
            .await
    }

    /// Create a directory and the missing ones above it; one already there is not an error.
    #[host("Vm")]
    pub async fn createDir(&self, lua: Lua, path: String) -> mlua::Result<Result<(), FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "createDir", true, |f| async move { f.create_dir(at, &path).await }).await
    }

    /// Remove a file, or a directory: empty (`not_empty` otherwise) unless `{ recursive = true }`.
    #[host("Vm")]
    pub async fn remove(
        &self,
        lua: Lua,
        path: String,
        options: RemoveOptions,
    ) -> mlua::Result<Result<(), FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "remove", true, |f| async move { f.remove(at, &path, options.recursive).await })
            .await
    }

    /// Rename or move inside the location; an existing file at `to` is replaced.
    #[host("Vm")]
    pub async fn r#move(&self, lua: Lua, from: String, to: String) -> mlua::Result<Result<(), FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "move", true, |f| async move { f.move_to(at, &from, &to).await }).await
    }

    /// Copy a file (an existing file at `to` is replaced) or a directory tree (`to` must not exist).
    #[host("Vm")]
    pub async fn copy(&self, lua: Lua, from: String, to: String) -> mlua::Result<Result<(), FileError>> {
        let at = self.at;
        call(&shared(&lua)?, "copy", true, |f| async move { f.copy(at, &from, &to).await }).await
    }
}

/// One file call, holding the turn (`Role::Vm`). A write is refused first inside a `readOnly` call
/// (it raises, like `app.kv:set`); the files are the read-only handle inside a `readOnly` call either way.
async fn call<T, Fut>(
    s: &Sh,
    method: &str,
    write: bool,
    op: impl FnOnce(FilesPort) -> Fut,
) -> mlua::Result<Result<T, FileError>>
where
    Fut: Future<Output = FileResult<T>>,
{
    if write {
        s.require_write(&format!("app.files:{method}"))?;
    }
    let files = s.files();
    // Nothing on the first poll. mlua polls a host call once before it tries to yield, and where Luau refuses the
    // yield (a `string.gsub` callback, a `table.sort` comparator, a metamethod) it drops the call after that poll:
    // a job started now would go on although the script was told the call failed (`std:fs` does the same).
    tokio::task::yield_now().await;
    let done = tokio::select! {
        done = op(files) => done,
        () = s.life.closed() => return Err(mlua::Error::runtime("the app was unloaded")),
    };
    Ok(done.map_err(FileError::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script matches on the names the domain (and `std:fs`) uses: each kind keeps its name on the way out.
    #[test]
    fn every_kind_of_the_domain_is_a_kind_of_the_error_under_the_same_name() {
        for kind in FileErrorKind::ALL {
            let error = FileError::from(vos_domain::FileError::new(kind, "notes/a.txt: why"));
            assert_eq!(error.kind(), kind.as_str());
            assert_eq!(error.to_string(), "notes/a.txt: why");
        }
    }
}
