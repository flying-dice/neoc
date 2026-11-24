//! Path manipulation: `std:path` (mirrors `std::path`).
//!
//! Text operations on paths: none looks at the files. They follow the platform's own rules (`/` on Unix, `\` and
//! `/` plus drive prefixes on Windows), so `join`, `parent` and the rest behave as the operating system's paths do;
//! `separator` says which. On Windows a string that is not valid UTF-8 is not a path and is refused as an
//! argument; on Unix every string is one.
//!
//! Ported from the old VM's `std:path`. What changed on purpose:
//! - `with_extension` returns a typed error for an extension that holds a separator (it raised a string).
//! - `absolute`'s failure is a typed error (`empty_path`, `current_dir`).

use std::path::{Component, Path, PathBuf};

/// `path` cleaned by text alone (see `normalize`).
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
    if parts.is_empty() { PathBuf::from(".") } else { parts.into_iter().collect() }
}

#[mlua_ir::module("std:path")]
pub mod path {
    use mlua_ir::{LuaError, Variadic};
    use serde::Serialize;

    use super::normalized;
    use crate::modules::std::osstr::OsPath;

    /// The platform's preferred path separator: `"/"` on Unix, `"\\"` on Windows.
    #[allow(non_upper_case_globals)]
    pub const separator: &str = std::path::MAIN_SEPARATOR_STR;

    /// Why a path cannot be given this extension.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ExtensionError {
        /// The extension holds a path separator, so it would name another file.
        #[error("an extension cannot contain a path separator")]
        Separator,
    }

    /// Why a path cannot be made absolute.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum AbsoluteError {
        /// The path is empty.
        #[error("cannot make an empty path absolute")]
        EmptyPath,
        /// The path is relative and the current directory cannot be read (it is gone, say).
        #[error("the current directory is unavailable: {reason}")]
        CurrentDir {
            /// What the operating system said.
            reason: String,
        },
    }

    /// `base` and each of `parts` joined with the platform separator. A part that is absolute replaces everything
    /// before it (`join("a", "/b", "c")` is `"/b/c"`), as `Path::join` does. Does not normalise: `join("a", "..")`
    /// is `"a/.."`. With no parts it is `base`.
    ///
    /// ```luau
    /// assert(path.join("a", "b", "c.txt") == "a" .. path.separator .. "b" .. path.separator .. "c.txt")
    /// ```
    pub fn join(base: OsPath, parts: Variadic<OsPath>) -> OsPath {
        let mut joined = base.0;
        for part in parts.0 {
            joined.push(part.0);
        }
        OsPath(joined)
    }

    /// The path cleaned by text alone: `.` and repeated separators are dropped and `x/..` is folded away. A leading
    /// `..` of a relative path stays; a `..` at the root is dropped; an empty result is `"."`. It does not look at
    /// the disk, so with symlinks `a/link/..` is not necessarily `a`.
    ///
    /// ```luau
    /// assert(path.normalize("a/./b/../c") == path.join("a", "c"))
    /// assert(path.normalize("") == ".")
    /// ```
    pub fn normalize(path: OsPath) -> OsPath {
        OsPath(normalized(&path.0))
    }

    /// The path without its last component, or `nil` for a root (`"/"`) and for an empty path. The parent of `"a"`
    /// is `""`.
    pub fn parent(path: OsPath) -> Option<OsPath> {
        path.0.parent().map(OsPath::of)
    }

    /// The last component, or `nil` when the path ends in `..` or is a root. `file_name("a/b.txt")` is `"b.txt"`.
    pub fn file_name(path: OsPath) -> Option<OsPath> {
        path.0.file_name().map(OsPath::of)
    }

    /// The file name without its extension (`"b"` for `"a/b.txt"`; `".bashrc"` has no extension, so its stem is
    /// `".bashrc"`), or `nil` when there is no file name.
    pub fn file_stem(path: OsPath) -> Option<OsPath> {
        path.0.file_stem().map(OsPath::of)
    }

    /// The extension without the dot (`"txt"` for `"a/b.txt"`), or `nil` when there is none.
    pub fn extension(path: OsPath) -> Option<OsPath> {
        path.0.extension().map(OsPath::of)
    }

    /// The path with its extension replaced by `extension` (given without the dot); an empty `extension` removes
    /// it. A path with no file name is returned unchanged.
    ///
    /// ```luau
    /// assert(path.with_extension("b.txt", "md") == "b.md")
    /// local _, err = path.with_extension("b.txt", "a/b")
    /// assert(err ~= nil and err.kind == "separator")
    /// ```
    pub fn with_extension(path: OsPath, extension: OsPath) -> Result<OsPath, ExtensionError> {
        // `Path::with_extension` panics on a separator
        if extension.0.to_string_lossy().chars().any(std::path::is_separator) {
            return Err(ExtensionError::Separator);
        }
        Ok(OsPath(path.0.with_extension(extension.0)))
    }

    /// The path with its last component replaced by `name` (appended when the path has none). A `name` that is
    /// absolute or has several components is joined as `Path::with_file_name` does, so an absolute one replaces
    /// the whole path.
    pub fn with_file_name(path: OsPath, name: OsPath) -> OsPath {
        OsPath(path.0.with_file_name(name.0))
    }

    /// Whether the path is absolute for this platform (on Windows `"/a"` is not: it needs a drive).
    pub fn is_absolute(path: OsPath) -> bool {
        path.0.is_absolute()
    }

    /// The path made absolute by text alone: a relative path is joined to the current directory and `.` and
    /// repeated separators are dropped, and nothing is looked up on disk (a symlink stays a symlink, the path
    /// need not exist). On Unix `..` is kept; Windows folds it away by text, as its own paths do. Mirrors
    /// `std::path::absolute`.
    ///
    /// ```luau
    /// assert(path.is_absolute((assert(path.absolute("x")))))
    /// local _, err = path.absolute("")
    /// assert(err ~= nil and err.kind == "empty_path")
    /// ```
    pub fn absolute(path: OsPath) -> Result<OsPath, AbsoluteError> {
        if path.0.as_os_str().is_empty() {
            return Err(AbsoluteError::EmptyPath);
        }
        std::path::absolute(&path.0).map(OsPath).map_err(|error| AbsoluteError::CurrentDir { reason: error.to_string() })
    }

    /// The components of a path, as `Path::components` yields them: a root or drive prefix first (`"/"`, `"C:"`),
    /// then each name, with `".."` kept and a `"."` only at the start (`components("/a/./b")` is
    /// `{ "/", "a", "b" }`).
    pub fn components(path: OsPath) -> Vec<OsPath> {
        path.0.components().map(OsPath::of).collect()
    }

    /// The path relative to `base`, or `nil` when `base` is not a prefix of it, compared component by component
    /// (`strip_prefix("/a/b/c", "/a/b")` is `"c"`, but `"/a/bc"` is not under `"/a/b"`).
    pub fn strip_prefix(path: OsPath, base: OsPath) -> Option<OsPath> {
        path.0.strip_prefix(&base.0).ok().map(OsPath::of)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_engine;

    /// Evaluates a Lua expression with `path` bound to the module.
    fn eval<T: mlua::FromLuaMulti>(source: &str) -> T {
        let lua = make_engine().unwrap();
        lua.load(format!("local path = require('std:path'); return {source}")).eval().unwrap()
    }

    fn text(source: &str) -> String {
        eval(source)
    }

    fn maybe(source: &str) -> Option<String> {
        eval(source)
    }

    /// `text` with `/` as this platform's separator.
    fn native(text: &str) -> String {
        text.replace('/', std::path::MAIN_SEPARATOR_STR)
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
            assert_eq!(normalized(Path::new(input)), Path::new(expected), "{input:?}");
        }
    }

    #[test]
    fn normalize_does_not_resolve_symlinks_or_touch_the_disk() {
        assert_eq!(text("path.normalize('/definitely/not/there/../here')"), native("/definitely/not/here"));
    }

    #[test]
    fn join_uses_the_separator_and_does_not_normalise() {
        assert_eq!(text("path.join('a', 'b')"), native("a/b"));
        assert_eq!(text("path.join(path.join('a', 'b'), 'c.txt')"), native("a/b/c.txt"));
        assert_eq!(text("path.join('a', 'b', 'c.txt')"), native("a/b/c.txt"));
        assert_eq!(text("path.join('a', 'b', 'c', 'd', 'e')"), native("a/b/c/d/e"));
        assert_eq!(text("path.join('a')"), "a");
        assert_eq!(text("path.join('a', '..')"), native("a/.."));
        assert_eq!(text("path.join('a', '')"), native("a/"));
        assert_eq!(text("path.separator"), std::path::MAIN_SEPARATOR_STR);
    }

    #[cfg(unix)]
    #[test]
    fn an_absolute_part_wins() {
        assert_eq!(text("path.join('a', '/b')"), "/b");
        assert_eq!(text("path.join('a', '/b', 'c')"), "/b/c");
    }

    #[cfg(windows)]
    #[test]
    fn an_absolute_part_wins() {
        assert_eq!(text(r"path.join('a', 'C:\\b')"), r"C:\b");
        assert_eq!(text(r"path.join('C:\\a', '\\b')"), r"C:\b");
        assert_eq!(text(r"path.join('a', 'C:\\b', 'c')"), r"C:\b\c");
    }

    #[test]
    fn inspection_functions_answer_nil_where_there_is_no_answer() {
        assert_eq!(maybe("path.parent('/a/b')").as_deref(), Some("/a"));
        assert_eq!(maybe("path.parent('a')").as_deref(), Some(""));
        assert_eq!(maybe("path.parent('/')"), None);
        assert_eq!(maybe("path.parent('')"), None);
        assert_eq!(maybe("path.file_name('a/b.txt')").as_deref(), Some("b.txt"));
        assert_eq!(maybe("path.file_name('a/..')"), None);
        assert_eq!(maybe("path.file_stem('a/b.tar.gz')").as_deref(), Some("b.tar"));
        assert_eq!(maybe("path.file_stem('.bashrc')").as_deref(), Some(".bashrc"));
        assert_eq!(maybe("path.extension('a/b.txt')").as_deref(), Some("txt"));
        assert_eq!(maybe("path.extension('a/b')"), None);
        assert_eq!(maybe("path.extension('.bashrc')"), None);
    }

    #[test]
    fn with_extension_refuses_a_separator_instead_of_panicking() {
        for extension in ["'a/b'", "'/'", "'<a>' .. string.rep('</a>', 3)", "'x' .. path.separator"] {
            let (value, kind, shown): (Option<String>, String, String) = eval(&format!(
                "(function() local value, err = path.with_extension('a/b.txt', {extension}); return value, err.kind, tostring(err) end)()"
            ));
            assert_eq!(value, None, "{extension}");
            assert_eq!(kind, "separator", "{extension}");
            assert_eq!(shown, "std:path.with_extension: separator: an extension cannot contain a path separator");
        }
    }

    #[test]
    fn with_extension_and_with_file_name_replace_one_part() {
        assert_eq!(text("path.with_extension('a/b.txt', 'md')"), "a/b.md");
        assert_eq!(text("path.with_extension('a/b.txt', '')"), "a/b");
        assert_eq!(text("path.with_extension('a/b', 'md')"), "a/b.md");
        assert_eq!(text("path.with_extension('/', 'md')"), "/");
        assert_eq!(text("path.with_file_name('a/b.txt', 'c.md')"), native("a/c.md"));
    }

    #[test]
    fn components_and_strip_prefix_work_by_component() {
        assert_eq!(text("table.concat(path.components('a/./b/../c'), '|')"), "a|b|..|c");
        assert_eq!(text("table.concat(path.components('./a/b'), '|')"), ".|a|b");
        assert_eq!(text("table.concat(path.components('/a'), '|')"), format!("{}|a", std::path::MAIN_SEPARATOR));
        assert_eq!(maybe("path.strip_prefix('/a/b/c', '/a/b')").as_deref(), Some("c"));
        assert_eq!(maybe("path.strip_prefix('/a/bc', '/a/b')"), None);
        assert_eq!(maybe("path.strip_prefix('/a', '/a')").as_deref(), Some(""));
        assert!(!eval::<bool>("path.is_absolute('a')"));
        assert!(eval::<bool>(if cfg!(windows) { r"path.is_absolute('C:\\a')" } else { "path.is_absolute('/a')" }));
    }

    #[test]
    fn absolute_joins_the_current_directory_without_touching_the_disk() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(text("path.absolute('x/./y')"), cwd.join("x").join("y").to_str().unwrap());
        let (value, kind, op): (Option<String>, String, String) =
            eval("(function() local value, err = path.absolute(''); return value, err.kind, err.op end)()");
        assert_eq!((value, kind.as_str(), op.as_str()), (None, "empty_path", "std:path.absolute"));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_drops_dots_and_keeps_parents() {
        assert_eq!(text("path.absolute('/no/such/./dir//x')"), "/no/such/dir/x");
        assert_eq!(text("path.absolute('/a/../b')"), "/a/../b");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_bytes_round_trip_on_unix() {
        assert_eq!(eval::<i64>("#path.join('a', '\\255\\254')"), 4);
        assert_eq!(eval::<i64>("#path.file_name('a/\\255')"), 1);
    }

    #[cfg(not(unix))]
    #[test]
    fn non_utf8_bytes_are_refused_as_an_argument_where_a_path_is_text() {
        let refused: String =
            eval("(function() local ok, err = pcall(path.join, 'a', '\\255\\254'); assert(not ok); return tostring(err) end)()");
        assert!(
            refused.contains("std:path.join: argument #2 'parts': a path must be valid UTF-8 on this platform"),
            "got: {refused}"
        );
    }

    #[test]
    fn a_nul_byte_and_a_very_long_path_are_just_paths() {
        assert_eq!(eval::<i64>("#path.join('a', 'b\\0c')"), 5);
        assert_eq!(eval::<i64>("#path.normalize(string.rep('a/', 100000))"), 199_999);
    }

    #[test]
    fn a_missing_or_wrong_argument_is_a_lua_error_not_a_panic() {
        let lua = make_engine().unwrap();
        for source in ["join()", "join('a', nil)", "join('a', 'b', 5)", "normalize({})", "parent(1)", "components(nil)"] {
            let outcome: mlua::Result<mlua::Value> = lua.load(format!("return require('std:path').{source}")).eval();
            assert!(outcome.is_err(), "{source}");
        }
    }
}
