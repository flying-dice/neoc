//! Path manipulation — `std:path` (mirrors `std::path`).
//!
//! Text operations on paths: none looks at the files (`fs.canonicalize` is the one that resolves symlinks). Only
//! `absolute` can fail on a valid path (an empty path, or no current directory, which it reads), and
//! `with_extension` raises on an extension that holds a separator. They follow the platform's own rules (`/` on Unix, `\`
//! and `/` plus drive prefixes on Windows), so `join`, `parent` and the rest behave as the operating system's
//! paths do; `separator` says which. On Windows a string that is not valid UTF-8 is not a path and raises a Lua
//! error; on Unix every string is one.

use std::path::{Component, Path, PathBuf};

use mlua::{IntoLua, Lua, LuaString, Table, Value, Variadic};

use crate::lua::facade::{DeclType, Module};
use crate::lua::std::osstr;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:path` surface through the facade.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:path")?;

    m.set("separator", std::path::MAIN_SEPARATOR_STR)?;
    m.const_(
        "separator",
        "string",
        "The platform's preferred path separator: `\"/\"` on Unix, `\"\\\\\"` on Windows.",
    );

    /// Join path parts with the platform separator. A later part that is absolute replaces everything before it (`join("a", "/b")` is `"/b"`), as `Path::join` does. Does not normalise: `join("a", "..")` is `"a/.."`.
    #[lua]
    m.func(
        "join",
        |lua, (first, rest): (LuaString, Variadic<LuaString>)| {
            let mut joined = path(&first)?;
            for part in rest.iter() {
                joined.push(path(part)?);
            }
            osstr::to_lua(lua, joined.as_os_str())
        },
    )?;
    /// Clean a path by text alone: drop `.` and repeated separators, and fold `x/..` away. A leading `..` of a relative path stays; a `..` at the root is dropped; an empty result is `"."`. It does not look at the disk, so with symlinks `a/link/..` is not necessarily `a` (use `fs.canonicalize` for that).
    #[lua(names(p = "path"))]
    m.func("normalize", |lua, p: LuaString| {
        osstr::to_lua(lua, normalized(&path(&p)?).as_os_str())
    })?;
    /// The path without its last component, or `nil` for a root (`"/"`) and for an empty path. The parent of `"a"` is `""`.
    #[lua(names(p = "path"))]
    m.func("parent", |lua, p: LuaString| {
        path(&p)?
            .parent()
            .map(|parent| osstr::to_lua(lua, parent.as_os_str()))
            .transpose()
    })?;
    /// The last component, or `nil` when the path ends in `..` or is a root. `file_name("a/b.txt")` is `"b.txt"`.
    #[lua(names(p = "path"))]
    m.func("file_name", |lua, p: LuaString| {
        path(&p)?
            .file_name()
            .map(|name| osstr::to_lua(lua, name))
            .transpose()
    })?;
    /// The file name without its extension (`"b"` for `"a/b.txt"`; `".bashrc"` has no extension, so its stem is `".bashrc"`), or `nil` when there is no file name.
    #[lua(names(p = "path"))]
    m.func("file_stem", |lua, p: LuaString| {
        path(&p)?
            .file_stem()
            .map(|stem| osstr::to_lua(lua, stem))
            .transpose()
    })?;
    /// The extension without the dot (`"txt"` for `"a/b.txt"`), or `nil` when there is none.
    #[lua(names(p = "path"))]
    m.func("extension", |lua, p: LuaString| {
        path(&p)?
            .extension()
            .map(|ext| osstr::to_lua(lua, ext))
            .transpose()
    })?;
    /// The path with its extension replaced by `extension` (given without the dot); an empty `extension` removes it. A path with no file name is returned unchanged. An `extension` that contains a path separator is a Lua error.
    #[lua(names(p = "path"))]
    m.func(
        "with_extension",
        |lua, (p, extension): (LuaString, LuaString)| {
            let extension = path(&extension)?;
            // `Path::with_extension` panics on a separator, and a panic out of a host function ends the call
            if extension
                .to_string_lossy()
                .chars()
                .any(std::path::is_separator)
            {
                return Err(mlua::Error::runtime(
                    "an extension cannot contain a path separator",
                ));
            }
            osstr::to_lua(lua, path(&p)?.with_extension(extension).as_os_str())
        },
    )?;
    /// The path with its last component replaced by `name` (appended when the path has none). A `name` that is absolute or has several components is joined as `Path::with_file_name` does, so an absolute one replaces the whole path.
    #[lua(names(p = "path"))]
    m.func(
        "with_file_name",
        |lua, (p, name): (LuaString, LuaString)| {
            let name = path(&name)?;
            osstr::to_lua(lua, path(&p)?.with_file_name(name).as_os_str())
        },
    )?;
    /// Whether the path is absolute for this platform (on Windows `"/a"` is not: it needs a drive).
    #[lua(names(p = "path"))]
    m.func("is_absolute", |_, p: LuaString| Ok(path(&p)?.is_absolute()))?;
    /// The path made absolute by text alone: a relative path is joined to the current directory and `.` and repeated separators are dropped, but `..` is kept and nothing is looked up on disk (a symlink stays a symlink, the path need not exist; `fs.canonicalize` resolves them). An empty path is an error, and so is a current directory that is gone. Mirrors `std::path::absolute`.
    #[lua(names(p = "path"))]
    m.try_func("absolute", |lua, p: LuaString| {
        let p = path_checked(&p)?;
        let absolute = std::path::absolute(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        osstr::to_lua(lua, absolute.as_os_str()).map_err(|e| e.to_string())
    })?;
    /// The components of a path as an array of strings, as `Path::components` yields them: a root or drive prefix first (`"/"`, `"C:"`), then each name, with `".."` kept and a `"."` only at the start (`components("/a/./b")` is `{ "/", "a", "b" }`).
    #[lua(names(p = "path"))]
    m.func("components", |lua, p: LuaString| {
        path(&p)?
            .components()
            .map(|c| osstr::to_lua(lua, c.as_os_str()))
            .collect::<mlua::Result<Vec<_>>>()
            .map(Components)
    })?;
    /// The path relative to `base`, or `nil` when `base` is not a prefix of it, compared component by component (`strip_prefix("/a/b/c", "/a/b")` is `"c"`, but `"/a/bc"` is not under `"/a/b"`).
    #[lua(names(p = "path"))]
    m.func("strip_prefix", |lua, (p, base): (LuaString, LuaString)| {
        let (p, base) = (path(&p)?, path(&base)?);
        p.strip_prefix(&base)
            .ok()
            .map(|rest| osstr::to_lua(lua, rest.as_os_str()))
            .transpose()
    })?;

    Ok(m)
}

/// The components of a path: a Lua array of strings.
struct Components(Vec<LuaString>);

impl IntoLua for Components {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        lua.create_sequence_from(self.0).map(Value::Table)
    }
}

impl DeclType for Components {
    fn decl_type() -> String {
        "{ string }".to_owned()
    }
}

/// A script's path argument, or an error message when this platform cannot represent it.
fn path_checked(s: &LuaString) -> Result<PathBuf, String> {
    osstr::to_path(s).ok_or_else(|| "a path must be valid UTF-8 on this platform".to_owned())
}

/// A script's path argument, or a Lua error when this platform cannot represent it.
fn path(s: &LuaString) -> mlua::Result<PathBuf> {
    osstr::to_path(s)
        .ok_or_else(|| mlua::Error::runtime("a path must be valid UTF-8 on this platform"))
}

/// `path` cleaned by text alone (see the `normalize` doc).
fn normalized(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        PathBuf::from(".")
    } else {
        parts.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::sandbox::make_engine;

    /// Evaluates a Lua expression with `path` bound to the module.
    fn eval<T: mlua::FromLua>(source: &str) -> T {
        let lua = make_engine().unwrap();
        lua.load(format!("local path = require('std:path'); return {source}"))
            .eval()
            .unwrap()
    }

    fn text(source: &str) -> String {
        eval(source)
    }

    fn maybe(source: &str) -> Option<String> {
        eval(source)
    }

    #[test]
    fn normalize_folds_dots_and_parents_by_text() {
        let cases = [
            ("a/./b//c", "a/b/c"),
            ("a/b/..", "a"),
            ("a/../..", ".."),
            ("../a", "../a"),
            ("/a/../..", "/"),
            ("/..", "/"),
            ("", "."),
            (".", "."),
            ("a/..", "."),
            ("./a", "a"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalized(Path::new(input)),
                Path::new(expected),
                "{input:?}"
            );
        }
    }

    #[test]
    fn normalize_does_not_resolve_symlinks_or_touch_the_disk() {
        assert_eq!(
            text("path.normalize('/definitely/not/there/../here')"),
            "/definitely/not/here"
        );
    }

    #[cfg(unix)]
    #[test]
    fn join_uses_the_separator_and_lets_an_absolute_part_win() {
        assert_eq!(text("path.join('a', 'b', 'c.txt')"), "a/b/c.txt");
        assert_eq!(text("path.join('a', '/b', 'c')"), "/b/c");
        assert_eq!(text("path.join('a')"), "a");
        assert_eq!(text("path.join('a', '..')"), "a/..");
        assert_eq!(text("path.separator"), "/");
    }

    #[cfg(unix)]
    #[test]
    fn inspection_functions_answer_nil_where_there_is_no_answer() {
        assert_eq!(maybe("path.parent('/a/b')").as_deref(), Some("/a"));
        assert_eq!(maybe("path.parent('a')").as_deref(), Some(""));
        assert_eq!(maybe("path.parent('/')"), None);
        assert_eq!(maybe("path.file_name('a/b.txt')").as_deref(), Some("b.txt"));
        assert_eq!(maybe("path.file_name('a/..')"), None);
        assert_eq!(
            maybe("path.file_stem('a/b.tar.gz')").as_deref(),
            Some("b.tar")
        );
        assert_eq!(
            maybe("path.file_stem('.bashrc')").as_deref(),
            Some(".bashrc")
        );
        assert_eq!(maybe("path.extension('a/b.txt')").as_deref(), Some("txt"));
        assert_eq!(maybe("path.extension('a/b')"), None);
        assert_eq!(maybe("path.extension('.bashrc')"), None);
    }

    #[test]
    fn with_extension_refuses_a_separator_instead_of_panicking() {
        let lua = make_engine().unwrap();
        for extension in [
            "'a/b'",
            "'/'",
            "'<a>' .. string.rep('</a>', 3)",
            "'x' .. std_sep",
        ] {
            let source = format!(
                "local path = require('std:path'); local std_sep = path.separator; local ok, err = pcall(path.with_extension, 'a/b.txt', {extension}); return ok, tostring(err)"
            );
            let (ok, err): (bool, String) = lua.load(source).eval().unwrap();
            assert!(!ok && err.contains("path separator"), "{extension}: {err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn with_extension_and_with_file_name_replace_one_part() {
        assert_eq!(text("path.with_extension('a/b.txt', 'md')"), "a/b.md");
        assert_eq!(text("path.with_extension('a/b.txt', '')"), "a/b");
        assert_eq!(text("path.with_extension('a/b', 'md')"), "a/b.md");
        assert_eq!(text("path.with_file_name('a/b.txt', 'c.md')"), "a/c.md");
    }

    #[cfg(unix)]
    #[test]
    fn components_and_strip_prefix_work_by_component() {
        assert_eq!(
            text("table.concat(path.components('/a/./b/../c'), '|')"),
            "/|a|b|..|c"
        );
        assert_eq!(text("table.concat(path.components('a/b'), '|')"), "a|b");
        assert_eq!(
            maybe("path.strip_prefix('/a/b/c', '/a/b')").as_deref(),
            Some("c")
        );
        assert_eq!(maybe("path.strip_prefix('/a/bc', '/a/b')"), None);
        assert_eq!(maybe("path.strip_prefix('/a', '/a')").as_deref(), Some(""));
        assert!(eval::<bool>("path.is_absolute('/a')"));
        assert!(!eval::<bool>("path.is_absolute('a')"));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_joins_the_current_directory_and_drops_dots_without_touching_the_disk() {
        assert_eq!(text("path.absolute('/no/such/./dir//x')"), "/no/such/dir/x");
        assert_eq!(text("path.absolute('/a/../b')"), "/a/../b");
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            text("path.absolute('x/y')"),
            cwd.join("x/y").to_str().unwrap()
        );
        let lua = make_engine().unwrap();
        let (value, err): (Option<String>, Option<String>) = lua
            .load("return require('std:path').absolute('')")
            .eval()
            .unwrap();
        assert_eq!(value, None);
        assert!(err.unwrap().starts_with("path.absolute: "));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_bytes_round_trip_on_unix() {
        assert_eq!(eval::<i64>("#path.join('a', '\\255\\254')"), 4);
        assert_eq!(eval::<i64>("#path.file_name('a/\\255')"), 1);
    }

    #[test]
    fn a_missing_argument_is_a_lua_error_not_a_panic() {
        let lua = make_engine().unwrap();
        let outcome: mlua::Result<mlua::Value> =
            lua.load("return require('std:path').join()").eval();
        assert!(outcome.is_err());
        let outcome: mlua::Result<mlua::Value> =
            lua.load("return require('std:path').normalize({})").eval();
        assert!(outcome.is_err());
    }
}
