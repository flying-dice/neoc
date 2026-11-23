//! Temporary files and directories — `lib:tempfile`.
//!
//! Two `UserData` types — `TempFile`, `TempDir` — wrap the `tempfile` crate's
//! `NamedTempFile` / `TempDir`. Both unlink on drop, so a script that lets the
//! handle go out of scope gets RAII cleanup via mlua's GC. Methods follow the
//! upstream Rust API: `path()`, `keep()` (consume → persist), `close()`
//! (explicit cleanup).
//!
//! Flat helpers (`tempfile`, `tempdir`, `gettempdir`) layer Python/Deno-style
//! ergonomics on top — they delegate to the constructors, never re-decide.
//! `gettempdir()` mirrors Python `tempfile.gettempdir()` (Node `os.tmpdir`,
//! Go `os.TempDir`).
//!
//! Naming convention matches Python's `tempfile` module: the namespace is
//! `lib:tempfile` (Python pattern: distinct top-level module), and the
//! constructor names mirror the `tempfile` crate (`NamedTempFile`, `TempDir`).
//!
//! ## Options
//!
//! `tempfile()` / `tempdir()` accept an optional table:
//!
//! - `prefix` — filename prefix (default `"tmp"`)
//! - `suffix` — filename suffix (default empty)
//! - `dir`    — parent directory (default: system temp dir)
//!
//! Invalid prefix/suffix containing a path separator surfaces as the
//! `tempfile` crate's own error rather than silently substituting.

use std::path::PathBuf;

use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use tempfile::{Builder, NamedTempFile, TempDir};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Options {
    prefix: Option<String>,
    suffix: Option<String>,
    dir: Option<String>,
}

impl Options {
    fn from_table(opts: Option<Table>) -> mlua::Result<Self> {
        let Some(t) = opts else {
            return Ok(Self::default());
        };
        Ok(Self {
            prefix: t.get::<Option<String>>("prefix")?,
            suffix: t.get::<Option<String>>("suffix")?,
            dir: t.get::<Option<String>>("dir")?,
        })
    }

    fn builder(&self) -> Builder<'_, '_> {
        let mut b = Builder::new();
        if let Some(p) = self.prefix.as_deref() {
            b.prefix(p);
        }
        if let Some(s) = self.suffix.as_deref() {
            b.suffix(s);
        }
        b
    }

    fn parent(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(PathBuf::from)
    }
}

// ---------------------------------------------------------------------------
// TempFile UserData
// ---------------------------------------------------------------------------

/// Wraps `NamedTempFile` so the Lua side can call `:path()`, `:keep()`,
/// `:close()`. The inner `Option` is `Some` until the handle is consumed by
/// `keep` or `close`; subsequent calls error with "already closed".
pub struct TempFileHandle {
    inner: Option<NamedTempFile>,
    path: String,
}

impl TempFileHandle {
    fn new(file: NamedTempFile) -> Self {
        let path = file.path().to_string_lossy().into_owned();
        Self {
            inner: Some(file),
            path,
        }
    }
}

#[neoc_macros::lua_userdata]
impl UserData for TempFileHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Return the absolute path to the temp file; stable across calls and does not consume the handle.
        #[lua("string")]
        methods.add_method("path", |_, this, _: ()| Ok(this.path.clone()));

        /// Consume the handle and keep the file on disk past drop, returning the path.
        #[lua("Result<string, string>")]
        methods.add_method_mut("keep", |lua, this, _: ()| match this.inner.take() {
            Some(file) => match file.keep() {
                Ok((_, p)) => ok(lua, p.to_string_lossy().into_owned()),
                Err(e) => from_err(lua, "tempfile.TempFile.keep", e.error),
            },
            None => from_err(lua, "tempfile.TempFile.keep", "already closed"),
        });

        /// Release the handle eagerly; idempotent.
        #[lua("()")]
        methods.add_method_mut("close", |_, this, _: ()| {
            this.inner.take();
            Ok(())
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("TempFile({})", this.path))
        });
    }
}

// ---------------------------------------------------------------------------
// TempDir UserData
// ---------------------------------------------------------------------------

pub struct TempDirHandle {
    inner: Option<TempDir>,
    path: String,
}

impl TempDirHandle {
    fn new(dir: TempDir) -> Self {
        let path = dir.path().to_string_lossy().into_owned();
        Self {
            inner: Some(dir),
            path,
        }
    }
}

#[neoc_macros::lua_userdata]
impl UserData for TempDirHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Return the absolute path to the temp directory as a string.
        #[lua("string")]
        methods.add_method("path", |_, this, _: ()| Ok(this.path.clone()));

        /// Consume the handle and keep the directory on disk past drop, returning the path.
        #[lua("Result<string, string>")]
        methods.add_method_mut("keep", |lua, this, _: ()| match this.inner.take() {
            Some(dir) => ok(lua, dir.keep().to_string_lossy().into_owned()),
            None => from_err(lua, "tempfile.TempDir.keep", "already closed"),
        });

        /// Remove the directory eagerly, surfacing unlink errors; idempotent.
        #[lua("Result<bool, string>")]
        methods.add_method_mut("close", |lua, this, _: ()| match this.inner.take() {
            // Explicit close surfaces unlink errors; drop swallows them by
            // design, so this is the only way for a script to learn that
            // the directory could not be removed.
            Some(dir) => match dir.close() {
                Ok(()) => Ok((Value::Boolean(true), Value::Nil)),
                Err(e) => from_err(lua, "tempfile.TempDir.close", e),
            },
            None => Ok((Value::Boolean(true), Value::Nil)),
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("TempDir({})", this.path))
        });
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:tempfile` surface through the facade.
///
/// The `TempFile` / `TempDir` constructor sub-tables and the `tempfile` /
/// `tempdir` flat helpers are set verbatim with
/// [`crate::lua::facade::Module::set`]: the constructors carry
/// `tempfile.<Type>.new`-named spans the facade's `type_` would rename, and the
/// flat helpers delegate to those constructors so their error prefix is
/// `tempfile.<Type>.new` (not their own name) under a nested span — neither
/// shape the facade models. `gettempdir` is an infallible one-shot routed
/// through [`crate::lua::facade::Module::func`]; its `tempfile.gettempdir` span
/// matches what the facade emits. Both userdata types keep their own
/// `impl UserData` and are recorded here via
/// [`crate::lua::facade::Module::userdata`].
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:tempfile")?;

    let tf_sub = lua.create_table()?;
    tf_sub.set("new", lua.create_function(tempfile_new)?)?;
    m.set("TempFile", tf_sub)?;

    let td_sub = lua.create_table()?;
    td_sub.set("new", lua.create_function(tempdir_new)?)?;
    m.set("TempDir", td_sub)?;

    /// Return the system's temporary-directory path.
    #[lua]
    m.func("gettempdir", |_, _: ()| flat_gettempdir())?;
    /// Create a temporary file under the system temp directory, returning a `TempFile` handle.
    #[lua("Result<TempFile, string>", takes("opts: LuaValue?"))]
    m.set("tempfile", lua.create_function(flat_tempfile)?)?;
    m.declared(flat_tempdir::lua(), lua.create_function(flat_tempdir)?)?;

    m.userdata("TempFile", |t| {
        TempFileHandle::lua(t);
        t.static_of(tempfile_new::lua());
    });

    m.userdata("TempDir", |t| {
        TempDirHandle::lua(t);
        t.static_of(tempdir_new::lua());
    });

    Ok(m)
}

/// Create a temp file; `opts` is `{ prefix?, suffix?, dir? }`. Deleted on close.
#[neoc_macros::lua_fn(name = "new", returns = "Result<TempFile, string>")]
#[instrument(
    name = "tempfile.TempFile.new",
    target = "lua::lib::tempfile",
    skip_all
)]
fn tempfile_new(lua: &Lua, opts: Option<Table>) -> mlua::Result<(Value, Value)> {
    let o = Options::from_table(opts)?;
    let builder = o.builder();
    let result = match o.parent() {
        Some(dir) => builder.tempfile_in(dir),
        None => builder.tempfile(),
    };
    match result {
        Ok(f) => ok(lua, TempFileHandle::new(f)),
        Err(e) => from_err(lua, "tempfile.TempFile.new", e),
    }
}

/// Create a temp directory; `opts` is `{ prefix?, suffix?, dir? }`. Deleted on close.
#[neoc_macros::lua_fn(name = "new", returns = "Result<TempDir, string>")]
#[instrument(name = "tempfile.TempDir.new", target = "lua::lib::tempfile", skip_all)]
fn tempdir_new(lua: &Lua, opts: Option<Table>) -> mlua::Result<(Value, Value)> {
    let o = Options::from_table(opts)?;
    let builder = o.builder();
    let result = match o.parent() {
        Some(dir) => builder.tempdir_in(dir),
        None => builder.tempdir(),
    };
    match result {
        Ok(d) => ok(lua, TempDirHandle::new(d)),
        Err(e) => from_err(lua, "tempfile.TempDir.new", e),
    }
}

#[instrument(name = "tempfile.tempfile", target = "lua::lib::tempfile", skip_all)]
fn flat_tempfile(lua: &Lua, opts: Option<Table>) -> mlua::Result<(Value, Value)> {
    tempfile_new(lua, opts)
}

/// Create a temporary directory under the system temp directory, returning a `TempDir` handle.
#[neoc_macros::lua_fn(name = "tempdir", returns = "Result<TempDir, string>")]
#[instrument(name = "tempfile.tempdir", target = "lua::lib::tempfile", skip_all)]
fn flat_tempdir(lua: &Lua, opts: Option<Table>) -> mlua::Result<(Value, Value)> {
    tempdir_new(lua, opts)
}

fn flat_gettempdir() -> mlua::Result<String> {
    Ok(std::env::temp_dir().to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use std::hint::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("tempfile");

    g.bench_function("library/tempfile", |b| {
        b.iter(|| {
            let f = tempfile::NamedTempFile::new().unwrap();
            let _ = black_box(f.path().to_path_buf());
        });
    });

    g.bench_function("library/tempdir", |b| {
        b.iter(|| {
            let d = tempfile::TempDir::new().unwrap();
            let _ = black_box(d.path().to_path_buf());
        });
    });

    let ident: mlua::Function = lua.create_function(|_, s: String| Ok(s)).expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: String = ident.call(black_box("/tmp/x".to_string())).unwrap();
        });
    });

    let tf_chunk: mlua::Function = lua
        .load(
            r#"return function()
                local t = require("lib:tempfile")
                local f = assert(t.tempfile())
                local p = f:path()
                f:close()
                return p
            end"#,
        )
        .eval()
        .expect("tf chunk");
    g.bench_function("through_lua/tempfile", |b| {
        b.iter(|| {
            let _: String = tf_chunk.call(()).unwrap();
        });
    });

    let td_chunk: mlua::Function = lua
        .load(
            r#"return function()
                local t = require("lib:tempfile")
                local d = assert(t.tempdir())
                local p = d:path()
                d:close()
                return p
            end"#,
        )
        .eval()
        .expect("td chunk");
    g.bench_function("through_lua/tempdir", |b| {
        b.iter(|| {
            let _: String = td_chunk.call(()).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: Table = lua
            .load(r#"return require("lib:tempfile")"#)
            .eval()
            .unwrap();
        lua.globals().set("tempfile", m).unwrap();
        lua
    }

    #[test]
    fn tempfile_creates_real_file_under_system_temp() {
        let lua = engine();
        let p: String = lua
            .load(
                r#"
                local f = assert(tempfile.tempfile())
                return f:path()
            "#,
            )
            .eval()
            .unwrap();
        assert!(
            Path::new(&p).exists(),
            "expected temp file at {p:?} to exist before drop"
        );
        let sys = std::env::temp_dir();
        assert!(
            Path::new(&p).starts_with(&sys),
            "expected {p:?} under system temp {sys:?}"
        );
    }

    #[test]
    fn tempdir_creates_real_directory() {
        let lua = engine();
        let p: String = lua
            .load(
                r#"
                local d = assert(tempfile.tempdir())
                return d:path()
            "#,
            )
            .eval()
            .unwrap();
        assert!(
            Path::new(&p).is_dir(),
            "expected temp dir at {p:?} to exist as directory"
        );
    }

    #[test]
    fn drop_unlinks_temp_file() {
        let lua = engine();
        lua.load(
            r#"
                do
                    local f = assert(tempfile.tempfile())
                    captured_path = f:path()
                end
            "#,
        )
        .exec()
        .unwrap();
        // Collect outside the chunk: the dropped userdata is no longer reachable
        // from a live stack frame, so the collector can run its destructor.
        lua.gc_collect().unwrap();
        let p: String = lua.load("return captured_path").eval().unwrap();
        assert!(
            !Path::new(&p).exists(),
            "expected temp file at {p:?} to be unlinked after GC"
        );
    }

    #[test]
    fn drop_unlinks_temp_dir_recursively() {
        let lua = engine();
        lua.load(
            r#"
                do
                    local d = assert(tempfile.tempdir())
                    captured_path = d:path()
                end
            "#,
        )
        .exec()
        .unwrap();
        // Collect outside the chunk: the dropped userdata is no longer reachable
        // from a live stack frame, so the collector can run its destructor.
        lua.gc_collect().unwrap();
        let p: String = lua.load("return captured_path").eval().unwrap();
        assert!(
            !Path::new(&p).exists(),
            "expected temp dir at {p:?} to be removed after GC"
        );
    }

    #[test]
    fn keep_persists_file_past_drop() {
        let lua = engine();
        lua.load(
            r#"
                do
                    local f = assert(tempfile.tempfile())
                    kept_path = assert(f:keep())
                end
            "#,
        )
        .exec()
        .unwrap();
        // Collect outside the chunk: the dropped userdata is no longer reachable
        // from a live stack frame, so the collector can run its destructor.
        lua.gc_collect().unwrap();
        let p: String = lua.load("return kept_path").eval().unwrap();
        assert!(
            Path::new(&p).exists(),
            "expected kept file at {p:?} to survive drop"
        );
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn keep_persists_dir_past_drop() {
        let lua = engine();
        lua.load(
            r#"
                do
                    local d = assert(tempfile.tempdir())
                    kept_path = assert(d:keep())
                end
            "#,
        )
        .exec()
        .unwrap();
        // Collect outside the chunk: the dropped userdata is no longer reachable
        // from a live stack frame, so the collector can run its destructor.
        lua.gc_collect().unwrap();
        let p: String = lua.load("return kept_path").eval().unwrap();
        assert!(
            Path::new(&p).is_dir(),
            "expected kept dir at {p:?} to survive drop"
        );
        std::fs::remove_dir_all(&p).unwrap();
    }

    #[test]
    fn double_keep_returns_error() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua
            .load(
                r#"
                local f = assert(tempfile.tempfile())
                f:keep()
                return f:keep()
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(v, Value::Nil));
        let msg = e.expect("error message");
        assert!(
            msg.starts_with("tempfile.TempFile.keep:"),
            "expected prefixed error, got {msg:?}"
        );
        assert!(msg.contains("already closed"), "got {msg:?}");
    }

    #[test]
    fn close_is_idempotent_for_files() {
        let lua = engine();
        lua.load(
            r#"
                local f = assert(tempfile.tempfile())
                f:close()
                f:close()
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn close_is_idempotent_for_dirs() {
        let lua = engine();
        lua.load(
            r#"
                local d = assert(tempfile.tempdir())
                local ok1 = d:close()
                local ok2 = d:close()
                assert(ok1 == true and ok2 == true)
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn prefix_and_suffix_applied() {
        let lua = engine();
        let p: String = lua
            .load(
                r#"
                local f = assert(tempfile.tempfile({prefix = "neoc-", suffix = ".tmp"}))
                return f:path()
            "#,
            )
            .eval()
            .unwrap();
        let name = Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with("neoc-"), "got name {name:?}");
        assert!(name.ends_with(".tmp"), "got name {name:?}");
    }

    #[test]
    fn dir_option_places_file_under_chosen_parent() {
        let lua = engine();
        let parent = tempfile::tempdir().unwrap();
        lua.globals()
            .set("parent_dir", parent.path().to_string_lossy().into_owned())
            .unwrap();
        let p: String = lua
            .load(
                r#"
                local f = assert(tempfile.tempfile({dir = parent_dir}))
                return f:path()
            "#,
            )
            .eval()
            .unwrap();
        assert!(
            Path::new(&p).starts_with(parent.path()),
            "expected {p:?} under {:?}",
            parent.path()
        );
    }

    #[test]
    fn dir_missing_returns_error() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua
            .load(
                r#"
                return tempfile.tempfile({dir = "/does/not/exist/anywhere"})
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(v, Value::Nil));
        let msg = e.expect("error message");
        assert!(
            msg.starts_with("tempfile.TempFile.new:"),
            "expected prefixed error, got {msg:?}"
        );
    }

    #[test]
    fn gettempdir_matches_env_temp_dir() {
        let lua = engine();
        let p: String = lua.load("return tempfile.gettempdir()").eval().unwrap();
        assert_eq!(p, std::env::temp_dir().to_string_lossy());
    }

    #[test]
    fn tostring_includes_path() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local f = assert(tempfile.tempfile())
                return tostring(f)
            "#,
            )
            .eval()
            .unwrap();
        assert!(s.starts_with("TempFile("), "got {s:?}");
        assert!(s.ends_with(")"), "got {s:?}");
    }

    #[test]
    fn constructor_alias_matches_flat_function() {
        let lua = engine();
        lua.load(
            r#"
                local a = assert(tempfile.TempFile.new())
                local b = assert(tempfile.TempDir.new())
                assert(type(a:path()) == "string")
                assert(type(b:path()) == "string")
            "#,
        )
        .exec()
        .unwrap();
    }
}
