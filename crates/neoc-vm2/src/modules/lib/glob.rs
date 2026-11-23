//! File pattern matching: `lib:glob`.
//!
//! `Glob` and `GlobSet` are compiled, reusable patterns; `is_match` and `scan` compile one for a single use.
//! Backed by `globset` for matching and `walkdir` for the walk.
//!
//! Every pattern goes through one bounded compile: at most 4096 bytes, 256 `{` and 32 levels of `{...}` nesting.
//! A longer or deeper pattern is an error, as is one whose regex the engine refuses.
//!
//! A name that starts with `.` is hidden: a path with a hidden component matches only a pattern that names one
//! (`.git*`, `**/.cache/*.rs`), as ripgrep and fd do.
//!
//! Ported from neoc-vm's `lib:glob`. What changed on purpose:
//! - A failure is a typed error: `invalid_pattern` (with the pattern and why), and for a scan also `not_found` and
//!   `not_a_directory` (with the directory).
//! - A scan returns one value, `{ paths, errors }`: the matches, and the entries the walk could not read (it
//!   returned the two as `(paths, errors)`, in the place every other function has its error).
//! - A wrong option (an unknown name, a `type` that is not one, a negative `limit`) is refused as a wrong argument,
//!   with its place (it was a returned string).
//! - `GlobSet:is_empty()` is new, beside `len()`.

use globset::{Glob as GsGlob, GlobSet as GsSet, GlobSetBuilder};

/// The longest glob pattern the VM compiles, in bytes. Real patterns are a few dozen bytes; the bound only
/// keeps a hostile one from reaching globset's recursive parser.
pub(crate) const MAX_PATTERN_BYTES: usize = 4096;

/// The deepest `{...}` alternation nesting a pattern may have. globset recurses once per level (in its parser
/// and in the regex it builds), and a stack overflow aborts the host. Real patterns nest a level or two.
pub(crate) const MAX_BRACE_DEPTH: usize = 32;

/// The most `{` a pattern may contain, nested or not.
pub(crate) const MAX_BRACES: usize = 256;

/// Refuse a pattern that is too long or has too many or too deeply nested alternations, before globset sees
/// it. Counts every unescaped `{` and `}` without parsing character classes, so `[{]` counts as a brace and
/// a `}` inside a class closes one; the total length bound keeps either error small.
fn check_pattern(pattern: &str) -> Result<(), String> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(format!(
            "the pattern is too long ({} bytes, at most {MAX_PATTERN_BYTES})",
            pattern.len()
        ));
    }
    let (mut depth, mut braces, mut escaped) = (0_usize, 0_usize, false);
    for c in pattern.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => {
                depth += 1;
                braces += 1;
                if depth > MAX_BRACE_DEPTH {
                    return Err(format!("the pattern nests `{{` more than {MAX_BRACE_DEPTH} deep"));
                }
                if braces > MAX_BRACES {
                    return Err(format!("the pattern has more than {MAX_BRACES} `{{`"));
                }
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// The one way the VM compiles a glob: the pattern is bounded first ([`check_pattern`]), and
/// `Glob::compile_matcher`, which panics on a pattern whose regex the engine refuses, is never used. Every failure
/// is the `Err`, as the reason.
pub(crate) fn compile(pattern: &str) -> Result<GsGlob, String> {
    check_pattern(pattern)?;
    GsGlob::new(pattern).map_err(|e| e.kind().to_string())
}

/// `patterns` compiled into one set, without a path to a panic: the pattern that failed and why.
fn set_of<'a>(patterns: impl IntoIterator<Item = &'a String>) -> Result<GsSet, (&'a str, String)> {
    let mut builder = GlobSetBuilder::new();
    let mut first = "";
    for pattern in patterns {
        if first.is_empty() {
            first = pattern;
        }
        builder.add(compile(pattern).map_err(|reason| (pattern.as_str(), reason))?);
    }
    builder.build().map_err(|e| (first, e.to_string()))
}

fn normalise_input_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn pattern_allows_dotfiles(pattern: &str) -> bool {
    pattern.split('/').any(|seg| seg.starts_with('.'))
}

fn path_has_dotfile(path: &str) -> bool {
    path.split('/').any(|seg| seg.starts_with('.'))
}

fn is_dotfile_filtered(pattern: &str, path: &str) -> bool {
    path_has_dotfile(path) && !pattern_allows_dotfiles(pattern)
}

#[mlua_ir::module("lib:glob")]
pub mod glob {
    use std::path::{Path, PathBuf};

    use globset::GlobSet as GsSet;
    use mlua_ir::{LuaError, Object, Options, Shape};
    use serde::{Deserialize, Serialize};
    use walkdir::WalkDir;

    use super::{
        is_dotfile_filtered, normalise_input_path, path_has_dotfile, pattern_allows_dotfiles, set_of,
    };

    /// How much of a refused pattern its error carries.
    const SHOWN_PATTERN_BYTES: usize = 128;

    /// Why a pattern cannot be compiled.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum PatternError {
        /// The pattern is not a glob (an unclosed `[`, say), or is past a bound: 4096 bytes, 256 `{`, 32 levels of
        /// `{...}`.
        #[error("{pattern}: {reason}")]
        InvalidPattern {
            /// The pattern refused (its first 128 bytes).
            pattern: String,
            /// What is wrong with it.
            reason: String,
        },
    }

    /// Why a scan could not start.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ScanError {
        /// The pattern, or one of the `ignore` patterns, cannot be compiled.
        #[error("{pattern}: {reason}")]
        InvalidPattern {
            /// The pattern refused (its first 128 bytes).
            pattern: String,
            /// What is wrong with it.
            reason: String,
        },
        /// The directory to scan does not exist.
        #[error("directory not found: {path}")]
        NotFound {
            /// The directory asked for.
            path: String,
        },
        /// The path to scan is not a directory.
        #[error("not a directory: {path}")]
        NotADirectory {
            /// The path asked for.
            path: String,
        },
        /// The directory cannot be resolved for `absolute`.
        #[error("{path}: {reason}")]
        Other {
            /// The directory asked for.
            path: String,
            /// What the operating system said.
            reason: String,
        },
    }

    impl From<PatternError> for ScanError {
        fn from(error: PatternError) -> Self {
            let PatternError::InvalidPattern { pattern, reason } = error;
            ScanError::InvalidPattern { pattern, reason }
        }
    }

    fn invalid((pattern, reason): (&str, String)) -> PatternError {
        let mut end = pattern.len().min(SHOWN_PATTERN_BYTES);
        while !pattern.is_char_boundary(end) {
            end -= 1;
        }
        PatternError::InvalidPattern { pattern: pattern[..end].to_owned(), reason }
    }

    /// Which entries a scan reports.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum EntryType {
        /// Files only.
        #[default]
        File,
        /// Directories only.
        Dir,
        /// Everything: files, directories, symlinks.
        Any,
    }

    /// How a scan walks and what it reports.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct ScanOptions {
        /// The directory to scan; the pattern is matched against paths relative to it. The current directory when
        /// not given.
        pub cwd: Option<String>,
        /// Report absolute paths (the directory resolved, joined with each match) instead of paths relative to
        /// `cwd`.
        pub absolute: bool,
        /// Report at most this many paths: the first ones in sorted order.
        pub limit: Option<u32>,
        /// Patterns of paths to leave out (`"**/target/**"`).
        pub ignore: Vec<String>,
        /// Walk into directories that symlinks point at.
        pub follow_symlinks: bool,
        /// Which entries to report.
        #[serde(rename = "type")]
        pub entry_type: EntryType,
    }

    /// An entry a scan could not read.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct ScanFailure {
        /// The entry, relative to nothing: as the walk named it. Empty when the walk had no path to name.
        pub path: String,
        /// What the operating system said.
        pub reason: String,
    }

    /// What a scan found.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct Scan {
        /// The matching paths, sorted, with `/` as the separator on every platform.
        pub paths: Vec<String>,
        /// The entries the walk could not read (a directory without permission): what is under them is not in
        /// `paths`. Empty when everything was read.
        pub errors: Vec<ScanFailure>,
    }

    fn normalise_path(path: &Path) -> String {
        let text = path.to_string_lossy().into_owned();
        if std::path::MAIN_SEPARATOR == '/' { text } else { text.replace('\\', "/") }
    }

    /// Walk the directory, apply the type, dotfile, ignore and pattern filters, and return the sorted matches with
    /// the entries the walk could not read.
    fn run_scan(pattern: &str, matcher: &GsSet, opts: &ScanOptions) -> Result<Scan, ScanError> {
        let ignore = if opts.ignore.is_empty() { None } else { Some(set_of(&opts.ignore).map_err(invalid)?) };
        let cwd = PathBuf::from(opts.cwd.as_deref().unwrap_or("."));
        let shown = || cwd.display().to_string();
        if !cwd.exists() {
            return Err(ScanError::NotFound { path: shown() });
        }
        if !cwd.is_dir() {
            return Err(ScanError::NotADirectory { path: shown() });
        }
        let absolute_root = if opts.absolute {
            Some(cwd.canonicalize().map_err(|e| ScanError::Other { path: shown(), reason: e.to_string() })?)
        } else {
            None
        };

        let allow_dotfiles = pattern_allows_dotfiles(pattern);
        let mut paths: Vec<String> = Vec::new();
        let mut errors: Vec<ScanFailure> = Vec::new();
        for entry in WalkDir::new(&cwd).follow_links(opts.follow_symlinks) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    errors.push(ScanFailure {
                        path: e.path().map(normalise_path).unwrap_or_default(),
                        reason: e.to_string(),
                    });
                    continue;
                }
            };
            if entry.depth() == 0 {
                continue;
            }
            let file_type = entry.file_type();
            let wanted = match opts.entry_type {
                EntryType::File => file_type.is_file(),
                EntryType::Dir => file_type.is_dir(),
                EntryType::Any => true,
            };
            if !wanted {
                continue;
            }
            let relative = normalise_path(entry.path().strip_prefix(&cwd).unwrap_or(entry.path()));
            if !allow_dotfiles && path_has_dotfile(&relative) {
                continue;
            }
            if ignore.as_ref().is_some_and(|set| set.is_match(&relative)) || !matcher.is_match(&relative) {
                continue;
            }
            paths.push(match &absolute_root {
                Some(root) => normalise_path(&root.join(&relative)),
                None => relative,
            });
        }
        paths.sort();
        if let Some(limit) = opts.limit {
            paths.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        Ok(Scan { paths, errors })
    }

    /// A compiled pattern. `tostring` gives the pattern.
    #[derive(Clone, Object)]
    pub struct Glob {
        pattern: String,
        /// A set of one: `globset::Glob::compile_matcher` panics on a pattern whose regex the engine refuses.
        matcher: GsSet,
    }

    impl Glob {
        fn compile(pattern: String) -> Result<Self, PatternError> {
            let matcher = set_of([&pattern]).map_err(invalid)?;
            Ok(Self { pattern, matcher })
        }

        fn matches(&self, path: &str) -> bool {
            let normalised = normalise_input_path(path);
            self.matcher.is_match(&normalised) && !is_dotfile_filtered(&self.pattern, &normalised)
        }
    }

    #[mlua_ir::object]
    impl Glob {
        /// Compile one pattern.
        ///
        /// ```luau
        /// local sources = assert(glob.Glob.new("src/**/*.rs"))
        /// assert(sources:is_match("src/lua/fs.rs") and not sources:is_match("README.md"))
        /// local _, err = glob.Glob.new("src/[*.rs")
        /// assert(err ~= nil and err.kind == "invalid_pattern")
        /// ```
        pub fn new(pattern: String) -> Result<Glob, PatternError> {
            Glob::compile(pattern)
        }

        /// Whether `path` matches the pattern. `\` in `path` is a separator, as `/` is.
        pub fn is_match(&self, path: String) -> bool {
            self.matches(&path)
        }

        /// Walk a directory (the current one, or `opts.cwd`) and return the paths under it that match, as
        /// `glob.scan` does.
        pub fn scan(&self, opts: ScanOptions) -> Result<Scan, ScanError> {
            run_scan(&self.pattern, &self.matcher, &opts)
        }

        /// The pattern.
        #[to_string]
        pub fn display(&self) -> String {
            self.pattern.clone()
        }
    }

    /// Several compiled patterns, matched at once.
    #[derive(Clone, Object)]
    pub struct GlobSet {
        patterns: Vec<String>,
        set: GsSet,
    }

    impl GlobSet {
        fn matching_indices(&self, path: &str) -> Vec<usize> {
            let normalised = normalise_input_path(path);
            self.set
                .matches(&normalised)
                .into_iter()
                .filter(|index| !is_dotfile_filtered(&self.patterns[*index], &normalised))
                .collect()
        }
    }

    #[mlua_ir::object]
    impl GlobSet {
        /// Compile a list of patterns into one matcher. One pattern that cannot be compiled refuses the set.
        ///
        /// ```luau
        /// local set = assert(glob.GlobSet.new({ "*.rs", "*.toml" }))
        /// assert(set:is_match("Cargo.toml") and set:matches("Cargo.toml")[1] == 2)
        /// ```
        pub fn new(patterns: Vec<String>) -> Result<GlobSet, PatternError> {
            let set = set_of(&patterns).map_err(invalid)?;
            Ok(GlobSet { patterns, set })
        }

        /// Whether any pattern of the set matches `path`.
        pub fn is_match(&self, path: String) -> bool {
            let normalised = normalise_input_path(&path);
            // No hidden component: the automaton answers on the first hit, whichever pattern it is.
            if !path_has_dotfile(&normalised) {
                return self.set.is_match(&normalised);
            }
            // A hidden component: one of the matching patterns must name one.
            self.set
                .matches(&normalised)
                .into_iter()
                .any(|index| pattern_allows_dotfiles(&self.patterns[index]))
        }

        /// The positions, from 1, of every pattern of the set that matches `path`.
        pub fn matches(&self, path: String) -> Vec<u32> {
            self.matching_indices(&path)
                .into_iter()
                .map(|index| u32::try_from(index + 1).unwrap_or(u32::MAX))
                .collect()
        }

        /// How many patterns the set holds.
        pub fn len(&self) -> u32 {
            u32::try_from(self.patterns.len()).unwrap_or(u32::MAX)
        }

        /// Whether the set holds no pattern, and so matches nothing.
        pub fn is_empty(&self) -> bool {
            self.patterns.is_empty()
        }
    }

    /// Compile `pattern` and test whether `path` matches it.
    ///
    /// ```luau
    /// assert(glob.is_match("src/**/*.rs", "src/lua/fs.rs") == true)
    /// assert(glob.is_match("**/*.rs", "src/.cache/foo.rs") == false) -- `.cache` is hidden
    /// ```
    pub fn is_match(pattern: String, path: String) -> Result<bool, PatternError> {
        Glob::compile(pattern).map(|glob| glob.matches(&path))
    }

    /// Compile `pattern`, walk a directory (the current one, or `opts.cwd`) and return the paths under it that
    /// match, sorted, relative to the directory unless `opts.absolute`. Files only, unless `opts.type` says
    /// otherwise. An entry the walk cannot read does not fail the scan: it is in the result's `errors`.
    ///
    /// ```luau
    /// local found = assert(glob.scan("*.none", { cwd = ".", limit = 10 }))
    /// assert(#found.paths == 0 and #found.errors == 0)
    /// ```
    pub fn scan(pattern: String, opts: ScanOptions) -> Result<Scan, ScanError> {
        let glob = Glob::compile(pattern)?;
        run_scan(&glob.pattern, &glob.matcher, &opts)
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    use super::{MAX_BRACE_DEPTH, MAX_BRACES, MAX_PATTERN_BYTES, compile};

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:glob")"#).eval().unwrap();
        lua.globals().set("glob", m).unwrap();
        lua
    }

    /// An engine with `cwd` set to a fresh directory, which the caller fills and keeps alive.
    fn engine_in() -> (Lua, tempfile::TempDir) {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        lua.globals().set("cwd", dir.path().to_string_lossy().into_owned()).unwrap();
        (lua, dir)
    }

    fn paths(lua: &Lua, call: &str) -> Vec<String> {
        lua.load(format!("local found = assert({call}); assert(#found.errors == 0); return found.paths"))
            .eval()
            .unwrap()
    }

    /// The bounds of the shared compile helper: a pattern at each limit still compiles, one past it is refused
    /// (every entry point that compiles a glob goes through it).
    #[test]
    fn the_shared_helper_bounds_length_and_alternation_nesting() {
        let nested = |n: usize| format!("{}a{}", "{a,".repeat(n), "}".repeat(n));
        assert!(compile(&nested(MAX_BRACE_DEPTH)).is_ok());
        assert!(compile(&nested(MAX_BRACE_DEPTH + 1)).unwrap_err().contains("nests"));
        let flat = |n: usize| "{a,b}".repeat(n);
        assert!(compile(&flat(MAX_BRACES)).is_ok());
        assert!(compile(&flat(MAX_BRACES + 1)).unwrap_err().contains("more than"));
        assert!(compile(&"a".repeat(MAX_PATTERN_BYTES)).is_ok());
        assert!(compile(&"a".repeat(MAX_PATTERN_BYTES + 1)).unwrap_err().contains("too long"));
        // an escaped brace is not an alternation (where `\` escapes: on Windows globset reads it as a separator)
        assert_eq!(compile(&"\\{".repeat(MAX_BRACES + 1)).is_ok(), !cfg!(windows));
    }

    #[test]
    fn flat_is_match_basic() {
        let lua = engine();
        let (yes, no, err): (bool, bool, Value) = lua
            .load(
                r#"
                local yes, err = glob.is_match("src/**/*.rs", "src/lua/fs.rs")
                return yes, glob.is_match("src/**/*.rs", "README.md"), err
                "#,
            )
            .eval()
            .unwrap();
        assert!(yes && !no && err.is_nil());
    }

    /// `Glob::compile_matcher` compiles with `expect`, which panics when globset's regex is refused. Every entry
    /// point builds a `GlobSet` and returns the error instead. This pattern is refused by `check_pattern` first
    /// (nothing within the bounds is known to reach the build error), so the test pins that each entry point
    /// reports an error, not that one particular guard fired.
    #[test]
    fn a_pattern_whose_regex_is_refused_is_an_error_not_a_panic() {
        let lua = engine();
        let pattern = format!("{}a{}", "{a,".repeat(130), "}".repeat(130));
        lua.globals().set("pattern", pattern).unwrap();
        for (call, op) in [
            ("glob.is_match(pattern, 'a')", "lib:glob.is_match"),
            ("glob.Glob.new(pattern)", "lib:glob.Glob.new"),
            ("glob.GlobSet.new({ 'ok', pattern })", "lib:glob.GlobSet.new"),
            ("glob.scan(pattern, { cwd = '.' })", "lib:glob.scan"),
            ("glob.scan('*', { cwd = '.', ignore = { pattern } })", "lib:glob.scan"),
        ] {
            let (value, kind, shown, reason, at): (Value, String, String, String, String) = lua
                .load(format!(
                    "local value, err = {call}; return value, err.kind, err.pattern, err.reason, err.op"
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{call}: {e}"));
            assert!(value.is_nil(), "{call}: {value:?}");
            assert_eq!((kind.as_str(), at.as_str()), ("invalid_pattern", op), "{call}");
            assert_eq!(shown.len(), 128, "{call}: the pattern is cut to its first 128 bytes");
            assert!(reason.contains("nests"), "{call}: {reason}");
        }
    }

    #[test]
    fn an_invalid_pattern_is_a_typed_error() {
        let lua = engine();
        for (call, op) in [
            (r#"glob.is_match("src/[*.rs", "src/foo.rs")"#, "lib:glob.is_match"),
            (r#"glob.Glob.new("src/[*.rs")"#, "lib:glob.Glob.new"),
            (r#"glob.GlobSet.new({ "valid", "src/[*.rs" })"#, "lib:glob.GlobSet.new"),
        ] {
            let (value, kind, pattern, text): (Value, String, String, String) = lua
                .load(format!(
                    "local value, err = {call}; return value, err.kind, err.pattern, tostring(err)"
                ))
                .eval()
                .unwrap();
            assert!(value.is_nil(), "{call}");
            assert_eq!((kind.as_str(), pattern.as_str()), ("invalid_pattern", "src/[*.rs"), "{call}");
            assert!(text.starts_with(&format!("{op}: invalid_pattern: src/[*.rs: ")), "{call}: {text}");
            assert!(text.contains("class"), "{call}: {text}");
        }
    }

    #[test]
    fn glob_userdata_is_match_and_tostring() {
        let lua = engine();
        let (ok, shown): (bool, String) = lua
            .load(
                r#"
                local g = assert(glob.Glob.new("src/**/*.rs"))
                return g:is_match("src/lua/fs.rs") and g:is_match("src\\lua\\fs.rs") and not g:is_match("a.rs"), tostring(g)
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
        assert_eq!(shown, "src/**/*.rs");
    }

    #[test]
    fn globset_matches_one_based_indices() {
        let lua = engine();
        let (indices, both): (Vec<u32>, Vec<u32>) = lua
            .load(
                r#"
                local s = assert(glob.GlobSet.new({ "*.rs", "*.toml", "Cargo.*" }))
                return s:matches("Cargo.toml"), s:matches("x.md")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(indices, [2, 3]);
        assert!(both.is_empty());
    }

    #[test]
    fn globset_empty_matches_nothing() {
        let lua = engine();
        let (matched, len): (bool, u32) = lua
            .load(
                r#"
                local s = assert(glob.GlobSet.new({}))
                assert(s:is_empty() and not assert(glob.GlobSet.new({ "a" })):is_empty())
                return s:is_match("anything"), s:len()
                "#,
            )
            .eval()
            .unwrap();
        assert!(!matched);
        assert_eq!(len, 0);
        let len: u32 = lua.load(r#"return assert(glob.GlobSet.new({ "a", "b" })):len()"#).eval().unwrap();
        assert_eq!(len, 2);
    }

    #[test]
    fn scan_returns_sorted_results() {
        let (lua, dir) = engine_in();
        for name in ["b.txt", "a.txt", "c.txt", "d.md"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(paths(&lua, r#"glob.scan("*.txt", { cwd = cwd })"#), ["a.txt", "b.txt", "c.txt"]);
        assert_eq!(
            paths(&lua, r#"assert(glob.Glob.new("*.txt")):scan({ cwd = cwd })"#),
            ["a.txt", "b.txt", "c.txt"]
        );
    }

    #[test]
    fn a_scan_of_what_is_not_a_directory_is_a_typed_error() {
        let (lua, dir) = engine_in();
        std::fs::write(dir.path().join("f"), "").unwrap();
        let (value, kind, path, text): (Value, String, String, String) = lua
            .load(
                r#"
                local value, err = glob.scan("*.rs", { cwd = "/no/such/place_xyzzy" })
                return value, err.kind, err.path, tostring(err)
                "#,
            )
            .eval()
            .unwrap();
        assert!(value.is_nil());
        assert_eq!(kind, "not_found");
        assert!(path.contains("place_xyzzy"), "{path}");
        assert!(text.starts_with("lib:glob.scan: not_found: directory not found: "), "{text}");
        let (kind, op): (String, String) = lua
            .load(
                r#"
                local _, err = assert(glob.Glob.new("*")):scan({ cwd = cwd .. "/f" })
                return err.kind, err.op
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!((kind.as_str(), op.as_str()), ("not_a_directory", "lib:glob.Glob:scan"));
    }

    #[test]
    fn scan_dotfile_excluded_by_default() {
        let (lua, dir) = engine_in();
        std::fs::write(dir.path().join("visible.txt"), "").unwrap();
        std::fs::write(dir.path().join(".hidden.txt"), "").unwrap();
        assert_eq!(paths(&lua, r#"glob.scan("*", { cwd = cwd })"#), ["visible.txt"]);
    }

    #[test]
    fn scan_dotfile_included_when_pattern_dotted() {
        let (lua, dir) = engine_in();
        std::fs::write(dir.path().join(".gitignore"), "").unwrap();
        assert_eq!(paths(&lua, r#"glob.scan(".git*", { cwd = cwd })"#), [".gitignore"]);
    }

    #[test]
    fn scan_limit_truncates() {
        let (lua, dir) = engine_in();
        for n in 0..5 {
            std::fs::write(dir.path().join(format!("f{n}.txt")), "").unwrap();
        }
        assert_eq!(paths(&lua, r#"glob.scan("*.txt", { cwd = cwd, limit = 2 })"#), ["f0.txt", "f1.txt"]);
        assert!(paths(&lua, r#"glob.scan("*.txt", { cwd = cwd, limit = 0 })"#).is_empty());
    }

    #[test]
    fn scan_ignore_excludes_patterns() {
        let (lua, dir) = engine_in();
        std::fs::create_dir(dir.path().join("keep")).unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("keep").join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("target").join("b.rs"), "").unwrap();
        assert_eq!(
            paths(&lua, r#"glob.scan("**/*.rs", { cwd = cwd, ignore = { "**/target/**" } })"#),
            ["keep/a.rs"]
        );
        assert_eq!(
            paths(&lua, r#"glob.scan("**/*.rs", { cwd = cwd, ignore = {} })"#),
            ["keep/a.rs", "target/b.rs"]
        );
    }

    #[test]
    fn scan_reports_files_directories_or_both() {
        let (lua, dir) = engine_in();
        std::fs::create_dir(dir.path().join("d1")).unwrap();
        std::fs::write(dir.path().join("f1"), "").unwrap();
        assert_eq!(paths(&lua, r#"glob.scan("*", { cwd = cwd })"#), ["f1"]);
        assert_eq!(paths(&lua, r#"glob.scan("*", { cwd = cwd, type = "file" })"#), ["f1"]);
        assert_eq!(paths(&lua, r#"glob.scan("*", { cwd = cwd, type = "dir" })"#), ["d1"]);
        assert_eq!(paths(&lua, r#"glob.scan("*", { cwd = cwd, type = "any" })"#), ["d1", "f1"]);
    }

    #[test]
    fn a_wrong_option_is_refused_as_an_argument() {
        let lua = engine();
        for (call, place) in [
            (r#"glob.scan("*", { type = "block" })"#, "lib:glob.scan: argument #2 'opts'"),
            (r#"glob.scan("*", { limit = -1 })"#, "lib:glob.scan: argument #2 'opts'"),
            (r#"glob.scan("*", { limit = 1.5 })"#, "lib:glob.scan: argument #2 'opts'"),
            (r#"glob.scan("*", { cwd = 5 })"#, "lib:glob.scan: argument #2 'opts'"),
            (r#"glob.scan("*", { ignore = "x" })"#, "lib:glob.scan: argument #2 'opts'"),
            (r#"glob.scan("*", { absolut = true })"#, "lib:glob.scan: argument #2 'opts'"),
            (r#"glob.Glob.new("*"):scan({ type = "block" })"#, "lib:glob.Glob:scan: argument #1 'opts'"),
            (r#"glob.GlobSet.new({ "a", 5 })"#, "lib:glob.GlobSet.new: argument #1 'patterns'"),
            (r#"glob.is_match("*")"#, "lib:glob.is_match: argument #2 'path'"),
        ] {
            let refused = lua.load(format!("return {call}")).eval::<Value>().unwrap_err().to_string();
            assert!(refused.contains(place), "{call}: {refused}");
        }
    }

    #[test]
    fn scan_absolute_returns_absolute_paths() {
        let (lua, dir) = engine_in();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let names = paths(&lua, r#"glob.scan("*.txt", { cwd = cwd, absolute = true })"#);
        assert_eq!(names.len(), 1);
        // canonicalize may differ from input on macOS (/private/var); just assert it's absolute and ends with the file.
        assert!(std::path::Path::new(&names[0]).is_absolute(), "not absolute: {}", names[0]);
        assert!(names[0].ends_with("/file.txt"), "got: {}", names[0]);
    }

    #[test]
    fn round_trip_invariant() {
        // For every emitted path: glob.is_match(pattern, path) == true.
        let (lua, dir) = engine_in();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "").unwrap();
        std::fs::write(dir.path().join("src").join("lib.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        let all_match: bool = lua
            .load(
                r#"
                local pat = "**/*.rs"
                local results = assert(glob.scan(pat, { cwd = cwd })).paths
                for _, p in results do
                    if not glob.is_match(pat, p) then return false end
                end
                return #results == 2
                "#,
            )
            .eval()
            .unwrap();
        assert!(all_match);
    }

    #[test]
    fn glob_and_globset_single_pattern_equivalent() {
        let lua = engine();
        let same: bool = lua
            .load(
                r#"
                local g = assert(glob.Glob.new("*.rs"))
                local s = assert(glob.GlobSet.new({ "*.rs" }))
                return g:is_match("foo.rs") == s:is_match("foo.rs")
                   and g:is_match("foo.md") == s:is_match("foo.md")
                "#,
            )
            .eval()
            .unwrap();
        assert!(same);
    }

    /// `GlobSet:is_match` must agree with `:matches` after dotfile filtering, across the no-dotfile fast path and
    /// the dotfile-aware path. Locks the contract independently of which internal branch was taken.
    #[test]
    fn globset_is_match_agrees_with_matches() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local cases = {
                    -- { patterns, path, expected }
                    { { "*.rs" },         "foo.rs",        true  },  -- fast path
                    { { "*.rs" },         "foo.md",        false },  -- fast path
                    { { "*", ".git*" },   ".gitignore",    true  },  -- dotfile path
                    { { "*" },            ".gitignore",    false },  -- dotfile path
                    { { "src/*.rs" },     "src/.hidden.rs", false }, -- nested dotfile
                }
                for _, c in cases do
                    local s = assert(glob.GlobSet.new(c[1]))
                    local matches_nonempty = #s:matches(c[2]) > 0
                    local is = s:is_match(c[2])
                    if is ~= c[3] then return false end
                    if matches_nonempty ~= is then return false end
                end
                return true
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok, "is_match/matches disagreement");
    }

    /// Nested dotfile components are filtered, not just leaves. Pins the ripgrep/fd-style behaviour so anyone
    /// changing it sees a red test.
    #[test]
    fn nested_dotfile_component_filters_path() {
        let lua = engine();
        let (hidden, named): (bool, bool) = lua
            .load(
                r#"return glob.is_match("**/*.rs", "src/.cache/foo.rs"), glob.is_match("**/.cache/*.rs", "src/.cache/foo.rs")"#,
            )
            .eval()
            .unwrap();
        assert!(!hidden, "nested dotfile component must filter the path");
        assert!(named, "explicit dotted pattern segment must allow the path");
    }

    /// Permission-denied entries are surfaced in the result's `errors`, never silently dropped. Unix-only: the
    /// Windows permission model is different enough that this style of test isn't portable. Skipped when the test
    /// process can still read the chmod-000 directory (root bypasses Unix permissions).
    #[cfg(unix)]
    #[test]
    fn scan_permission_denied_surfaces_in_errors() {
        use std::os::unix::fs::PermissionsExt;

        let (lua, dir) = engine_in();
        std::fs::write(dir.path().join("visible.txt"), "").unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("inner.txt"), "").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).ok();
            return;
        }

        let (results, failed, reasons): (Vec<String>, Vec<String>, Vec<String>) = lua
            .load(
                r#"
                local found = assert(glob.scan("**/*.txt", { cwd = cwd }))
                local failed, reasons = {}, {}
                for i, failure in found.errors do failed[i], reasons[i] = failure.path, failure.reason end
                return found.paths, failed, reasons
                "#,
            )
            .eval()
            .unwrap();
        // Restore the permissions so the directory can be removed whatever is asserted below.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).ok();

        assert_eq!(results, ["visible.txt"]);
        assert!(failed.iter().any(|path| path.ends_with("locked")), "{failed:?}");
        assert!(reasons.iter().any(|reason| reason.to_lowercase().contains("permission")), "{reasons:?}");
    }
}
