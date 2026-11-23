//! Temporary files and directories: `lib:tempfile`.
//!
//! `tempfile()` and `tempdir()` make a file or a directory with a name of its own under the system's temporary
//! directory (or `opts.dir`) and return a handle on it. The handle owns it: it is removed when the handle is
//! closed or collected, unless `keep()` was called first. Backed by the `tempfile` crate's `NamedTempFile` and
//! `TempDir`.
//!
//! ```lua
//! local tempfile = require("lib:tempfile")
//! local dir = assert(tempfile.tempdir({ prefix = "build-" }))
//! -- ... work under dir:path() ...
//! dir:close()
//! ```
//!
//! Ported from neoc-vm's `lib:tempfile`. What changed on purpose:
//! - A failure is a typed `TempError` (`not_found`, `permission_denied`, `closed`, ...).
//! - `TempFile.new` and `TempDir.new` are gone: they were `tempfile` and `tempdir` under a second name.
//!   `gettempdir` is gone too: it is `std:env`'s `temp_dir`.
//! - `TempFile:close()` reports a failure to remove the file, as `TempDir:close()` always did (it returned
//!   nothing).
//! - An option that does not exist, or of the wrong type, is refused as a wrong argument, with its place.

#[mlua_ir::module("lib:tempfile")]
pub mod tempfile {
    use std::path::PathBuf;
    use std::sync::Arc;

    use ::tempfile::{Builder, NamedTempFile};
    use mlua_ir::{LuaError, Object, Options};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};

    use crate::modules::std::osstr::OsPath;

    /// Why a temporary file or directory could not be made, kept or removed. The kind is the operating system's.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum TempError {
        /// The parent directory does not exist.
        #[error("{reason}")]
        NotFound {
            /// What the operating system said.
            reason: String,
        },
        /// The process may not write there.
        #[error("{reason}")]
        PermissionDenied {
            /// What the operating system said.
            reason: String,
        },
        /// The prefix or the suffix is not part of a file name (it holds a path separator, say).
        #[error("{reason}")]
        InvalidInput {
            /// What the operating system said.
            reason: String,
        },
        /// The handle no longer owns anything: it was kept or closed before.
        #[error("already closed")]
        Closed,
        /// Any other failure.
        #[error("{reason}")]
        Other {
            /// What the operating system said.
            reason: String,
        },
    }

    impl From<std::io::Error> for TempError {
        fn from(error: std::io::Error) -> Self {
            use std::io::ErrorKind as Kind;
            let reason = error.to_string();
            match error.kind() {
                Kind::NotFound => TempError::NotFound { reason },
                Kind::PermissionDenied => TempError::PermissionDenied { reason },
                Kind::InvalidInput | Kind::InvalidFilename => TempError::InvalidInput { reason },
                _ => TempError::Other { reason },
            }
        }
    }

    /// Where a temporary file or directory is made and how it is named.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct TempOptions {
        /// What the name starts with; `.tmp` when not given. The rest of the name is random.
        pub prefix: Option<String>,
        /// What the name ends with; nothing when not given.
        pub suffix: Option<String>,
        /// The directory to make it in; the system's temporary directory when not given.
        pub dir: Option<String>,
    }

    impl TempOptions {
        fn builder(&self) -> Builder<'_, '_> {
            let mut builder = Builder::new();
            if let Some(prefix) = self.prefix.as_deref() {
                builder.prefix(prefix);
            }
            if let Some(suffix) = self.suffix.as_deref() {
                builder.suffix(suffix);
            }
            builder
        }
    }

    /// A temporary file. It is removed when the handle is closed or collected, unless it was kept. `tostring`
    /// gives `TempFile(<path>)`.
    #[derive(Clone, Object)]
    pub struct TempFile {
        /// The file, until `keep` or `close` takes it.
        file: Arc<Mutex<Option<NamedTempFile>>>,
        path: PathBuf,
    }

    #[mlua_ir::object]
    impl TempFile {
        /// The file's absolute path. It stays the same, also after `keep` and `close`.
        pub fn path(&self) -> OsPath {
            OsPath(self.path.clone())
        }

        /// Keep the file: it is no longer removed with the handle. Returns its path. A handle that was kept or
        /// closed before is `closed`.
        pub fn keep(&self) -> Result<OsPath, TempError> {
            let file = self.file.lock().take().ok_or(TempError::Closed)?;
            let (_, path) = file.keep().map_err(|e| TempError::from(e.error))?;
            Ok(OsPath(path))
        }

        /// Remove the file now. Closing again, or after `keep`, does nothing.
        pub fn close(&self) -> Result<(), TempError> {
            let file = self.file.lock().take();
            file.map_or(Ok(()), |file| Ok(file.close()?))
        }

        /// `TempFile(<path>)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("TempFile({})", self.path.display())
        }
    }

    /// A temporary directory. It is removed with everything in it when the handle is closed or collected, unless
    /// it was kept. `tostring` gives `TempDir(<path>)`.
    #[derive(Clone, Object)]
    pub struct TempDir {
        /// The directory, until `keep` or `close` takes it.
        dir: Arc<Mutex<Option<::tempfile::TempDir>>>,
        path: PathBuf,
    }

    #[mlua_ir::object]
    impl TempDir {
        /// The directory's absolute path. It stays the same, also after `keep` and `close`.
        pub fn path(&self) -> OsPath {
            OsPath(self.path.clone())
        }

        /// Keep the directory: it is no longer removed with the handle. Returns its path. A handle that was kept
        /// or closed before is `closed`.
        pub fn keep(&self) -> Result<OsPath, TempError> {
            let dir = self.dir.lock().take().ok_or(TempError::Closed)?;
            Ok(OsPath(dir.keep()))
        }

        /// Remove the directory and everything in it now. This is how a script learns that it could not be
        /// removed: a handle that is collected removes it silently. Closing again, or after `keep`, does nothing.
        pub fn close(&self) -> Result<(), TempError> {
            let dir = self.dir.lock().take();
            dir.map_or(Ok(()), |dir| Ok(dir.close()?))
        }

        /// `TempDir(<path>)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("TempDir({})", self.path.display())
        }
    }

    /// Make a temporary file.
    ///
    /// ```luau
    /// local file = assert(tempfile.tempfile({ prefix = "report-", suffix = ".csv" }))
    /// assert(file:close())
    /// ```
    #[allow(clippy::module_name_repetitions)]
    pub fn tempfile(opts: TempOptions) -> Result<TempFile, TempError> {
        let builder = opts.builder();
        let file = match &opts.dir {
            Some(dir) => builder.tempfile_in(dir),
            None => builder.tempfile(),
        }?;
        let path = file.path().to_path_buf();
        Ok(TempFile { file: Arc::new(Mutex::new(Some(file))), path })
    }

    /// Make a temporary directory.
    ///
    /// ```luau
    /// local dir = assert(tempfile.tempdir())
    /// local kept = assert(dir:keep()) -- now the script's to remove
    /// local _, err = dir:keep()
    /// assert(kept == dir:path() and err ~= nil and err.kind == "closed")
    /// ```
    pub fn tempdir(opts: TempOptions) -> Result<TempDir, TempError> {
        let builder = opts.builder();
        let dir = match &opts.dir {
            Some(dir) => builder.tempdir_in(dir),
            None => builder.tempdir(),
        }?;
        let path = dir.path().to_path_buf();
        Ok(TempDir { dir: Arc::new(Mutex::new(Some(dir))), path })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:tempfile")"#).eval().unwrap();
        lua.globals().set("tempfile", m).unwrap();
        lua
    }

    #[test]
    fn tempfile_creates_real_file_under_system_temp() {
        let lua = engine();
        let p: String = lua
            .load(
                r#"
                held = assert(tempfile.tempfile())
                return held:path()
                "#,
            )
            .eval()
            .unwrap();
        assert!(Path::new(&p).is_file(), "expected temp file at {p:?} to exist before drop");
        let sys = std::env::temp_dir();
        assert!(Path::new(&p).starts_with(&sys), "expected {p:?} under system temp {sys:?}");
    }

    #[test]
    fn tempdir_creates_real_directory() {
        let lua = engine();
        let p: String = lua
            .load(
                r#"
                held = assert(tempfile.tempdir())
                return held:path()
                "#,
            )
            .eval()
            .unwrap();
        assert!(Path::new(&p).is_dir(), "expected temp dir at {p:?} to exist as directory");
    }

    /// A handle that goes out of scope is collected, and what it owned is removed: a file, and a directory with
    /// what a script put in it.
    #[test]
    fn drop_unlinks_what_the_handle_owned() {
        let lua = engine();
        lua.load(
            r#"
            do
                local f = assert(tempfile.tempfile())
                local d = assert(tempfile.tempdir())
                assert(require("std:fs").write(require("std:path").join(d:path(), "inner"), "x"))
                captured_file, captured_dir = f:path(), d:path()
            end
            "#,
        )
        .exec()
        .unwrap();
        // Collect outside the chunk: the dropped userdata is no longer reachable from a live stack frame.
        lua.gc_collect().unwrap();
        let (file, dir): (String, String) = lua.load("return captured_file, captured_dir").eval().unwrap();
        assert!(!Path::new(&file).exists(), "expected temp file at {file:?} to be unlinked after GC");
        assert!(!Path::new(&dir).exists(), "expected temp dir at {dir:?} to be removed after GC");
    }

    #[test]
    fn keep_persists_past_drop() {
        let lua = engine();
        lua.load(
            r#"
            do
                local f = assert(tempfile.tempfile())
                local d = assert(tempfile.tempdir())
                kept_file, kept_dir = assert(f:keep()), assert(d:keep())
                assert(kept_file == f:path() and kept_dir == d:path())
            end
            "#,
        )
        .exec()
        .unwrap();
        lua.gc_collect().unwrap();
        let (file, dir): (String, String) = lua.load("return kept_file, kept_dir").eval().unwrap();
        assert!(Path::new(&file).is_file(), "expected kept file at {file:?} to survive drop");
        assert!(Path::new(&dir).is_dir(), "expected kept dir at {dir:?} to survive drop");
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_keep_and_a_keep_after_close_are_closed() {
        let lua = engine();
        let seen: Vec<String> = lua
            .load(
                r#"
                local out = {}
                local f, d = assert(tempfile.tempfile()), assert(tempfile.tempdir())
                local kept = { assert(f:keep()), assert(d:keep()) }
                local closed_f, closed_d = assert(tempfile.tempfile()), assert(tempfile.tempdir())
                assert(closed_f:close() and closed_d:close())
                for _, handle in { f, d, closed_f, closed_d } do
                    local value, err = handle:keep()
                    assert(value == nil and err.kind == "closed")
                    table.insert(out, tostring(err))
                end
                local fs = require("std:fs")
                assert(fs.remove_file(kept[1]) and fs.remove_dir(kept[2]))
                return out
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            seen,
            [
                "lib:tempfile.TempFile:keep: closed: already closed",
                "lib:tempfile.TempDir:keep: closed: already closed",
                "lib:tempfile.TempFile:keep: closed: already closed",
                "lib:tempfile.TempDir:keep: closed: already closed",
            ]
        );
    }

    #[test]
    fn close_removes_now_and_is_idempotent() {
        let lua = engine();
        let (file, dir): (String, String) = lua
            .load(
                r#"
                local f, d = assert(tempfile.tempfile()), assert(tempfile.tempdir())
                assert(require("std:fs").write(require("std:path").join(d:path(), "inner"), "x"))
                for _, handle in { f, d } do
                    local ok1, err1 = handle:close()
                    local ok2, err2 = handle:close()
                    assert(ok1 == true and ok2 == true and err1 == nil and err2 == nil)
                end
                held = { f, d } -- still referenced: the close removed them, not the collector
                return f:path(), d:path()
                "#,
            )
            .eval()
            .unwrap();
        assert!(!Path::new(&file).exists() && !Path::new(&dir).exists());
    }

    #[test]
    fn prefix_and_suffix_applied() {
        let lua = engine();
        for make in ["tempfile", "tempdir"] {
            let p: String = lua
                .load(format!(r#"held = assert(tempfile.{make}({{ prefix = "neoc-", suffix = ".tmp" }})); return held:path()"#))
                .eval()
                .unwrap();
            let name = Path::new(&p).file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("neoc-") && name.ends_with(".tmp"), "{make}: got name {name:?}");
        }
    }

    #[test]
    fn dir_option_places_it_under_the_chosen_parent() {
        let lua = engine();
        let parent = tempfile::tempdir().unwrap();
        lua.globals().set("parent_dir", parent.path().to_string_lossy().into_owned()).unwrap();
        let (file, dir): (String, String) = lua
            .load(
                r#"
                held = { assert(tempfile.tempfile({ dir = parent_dir })), assert(tempfile.tempdir({ dir = parent_dir })) }
                return held[1]:path(), held[2]:path()
                "#,
            )
            .eval()
            .unwrap();
        assert!(Path::new(&file).starts_with(parent.path()), "expected {file:?} under {:?}", parent.path());
        assert!(Path::new(&dir).starts_with(parent.path()) && Path::new(&dir).is_dir());
    }

    #[test]
    fn a_missing_parent_is_a_typed_error() {
        let lua = engine();
        for (make, op) in [("tempfile", "lib:tempfile.tempfile"), ("tempdir", "lib:tempfile.tempdir")] {
            let (value, kind, at, text): (Value, String, String, String) = lua
                .load(format!(
                    r#"
                    local value, err = tempfile.{make}({{ dir = "/does/not/exist/anywhere" }})
                    assert(err.message == err.reason)
                    return value, err.kind, err.op, tostring(err)
                    "#
                ))
                .eval()
                .unwrap();
            assert!(value.is_nil());
            assert_eq!((kind.as_str(), at.as_str()), ("not_found", op));
            assert!(text.starts_with(&format!("{op}: not_found: ")), "{text}");
        }
    }

    #[test]
    fn a_wrong_option_is_refused_as_an_argument() {
        let lua = engine();
        for call in
            ["tempfile.tempfile({ prefx = 'a' })", "tempfile.tempdir({ dir = 5 })", "tempfile.tempfile('x')"]
        {
            let refused = lua.load(format!("return {call}")).eval::<Value>().unwrap_err().to_string();
            assert!(refused.contains("argument #1 'opts'"), "{call}: {refused}");
        }
    }

    #[test]
    fn tostring_includes_path() {
        let lua = engine();
        let (file, dir, file_path, dir_path): (String, String, String, String) = lua
            .load(
                r#"
                local f, d = assert(tempfile.tempfile()), assert(tempfile.tempdir())
                return tostring(f), tostring(d), f:path(), d:path()
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(file, format!("TempFile({file_path})"));
        assert_eq!(dir, format!("TempDir({dir_path})"));
    }

    #[test]
    fn the_second_names_are_gone() {
        let lua = engine();
        let gone: bool = lua
            .load(
                "return tempfile.TempFile == nil and tempfile.TempDir == nil and tempfile.gettempdir == nil",
            )
            .eval()
            .unwrap();
        assert!(gone);
    }
}
