//! File pattern matching — `lib:glob`.
//!
//! Dual surface: `Glob` / `GlobSet` UserData (compiled, reusable) plus flat
//! `is_match` / `scan` convenience functions. Backed by `globset` for the
//! automaton and `walkdir` for filesystem traversal.
//!
//! Every pattern goes through one bounded compile (also used by `lib:archive`'s `include` and
//! `lib:http`'s `serve_static`): at most 4096 bytes, 256 `{` and 32 levels of `{...}` nesting. A longer or
//! deeper pattern is an error, as is one whose regex the engine refuses.

use std::path::{Path, PathBuf};

use globset::{Glob as GsGlob, GlobSet as GsSet, GlobSetBuilder};
use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use tracing::instrument;
use walkdir::WalkDir;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, from_err, ok};

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:glob` surface through the facade.
///
/// `is_match` is the one fallible flat function the facade can model and is
/// routed through [`crate::lua::facade::Module::try_func`]. `scan` returns the
/// `(results, errors)` dual where *both* slots can be non-nil on a partial walk
/// — outside the facade's `(value, error)` idiom — so it is set verbatim
/// (keeping its `glob.scan` span) with [`crate::lua::facade::Module::set`]. The
/// `Glob` / `GlobSet` constructor sub-tables are also set verbatim because the
/// hand-written constructors carried no span; the userdata types keep their own
/// `impl UserData` and are recorded here via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:glob")?;

    let glob_sub = lua.create_table()?;
    glob_sub.set("new", lua.create_function(glob_new)?)?;
    m.set("Glob", glob_sub)?;

    let set_sub = lua.create_table()?;
    set_sub.set("new", lua.create_function(set_new)?)?;
    m.set("GlobSet", set_sub)?;

    m.try_func(
        "is_match",
        &[("pattern", "string"), ("path", "string")],
        "Compile `pattern` and test whether `path` matches it.",
        |_, (pattern, path): (String, String)| {
            GlobHandle::compile(&pattern).map(|g| g.matches(&path))
        },
    )?;

    m.set("scan", lua.create_function(flat_scan)?)?;

    m.record_fn(
        "scan",
        &[("pattern", "string"), ("opts", "LuaValue?")],
        "(LuaValue, LuaValue)",
        "Compile `pattern`, walk the filesystem, and return `(paths, errors)`: all matching paths, plus a list of `{ path, error }` rows for the entries the walk could not read (nil when there are none). A bad pattern or directory returns `(nil, message)` instead.",
    );

    m.userdata("Glob", |g| {
        g.method(
            "is_match",
            &[("path", "string")],
            "bool",
            "Test whether `path` matches this compiled pattern.",
        )
        .method(
            "scan",
            &[("opts", "LuaValue?")],
            "(LuaValue, LuaValue)",
            "Walk the filesystem and return `(paths, errors)` as `glob.scan` does: the matching paths, plus a list of `{ path, error }` rows for the entries the walk could not read (nil when there are none). A bad directory returns `(nil, message)` instead.",
        )
        .static_fn(
            "new",
            &[("pattern", "string")],
            "Result<Glob, string>",
            "Compile one glob pattern.",
        );
    });

    m.userdata("GlobSet", |s| {
        s.method(
            "is_match",
            &[("path", "string")],
            "bool",
            "True if any pattern in the set matches `path`.",
        )
        .method(
            "matches",
            &[("path", "string")],
            "LuaValue",
            "1-based indices of every pattern in the set that matches `path`.",
        )
        .method("len", &[], "number", "Number of patterns in the set.")
        .static_fn(
            "new",
            &[("patterns", "LuaValue")],
            "Result<GlobSet, string>",
            "Compile a list of glob patterns into one matcher.",
        );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Glob UserData
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GlobHandle {
    pattern: String,
    /// A set of one: `Glob::compile_matcher` compiles with `expect`, which panics (and a panic aborts the host)
    /// on a pattern whose regex the engine refuses, such as deeply nested alternations.
    matcher: GsSet,
}

impl GlobHandle {
    fn compile(pattern: &str) -> Result<Self, String> {
        Ok(Self {
            pattern: pattern.to_owned(),
            matcher: single(pattern)?,
        })
    }

    fn matches(&self, path: &str) -> bool {
        let normalised = normalise_input_path(path);
        self.matcher.is_match(&normalised) && !is_dotfile_filtered(&self.pattern, &normalised)
    }
}

impl UserData for GlobHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_match", |_, this, path: String| Ok(this.matches(&path)));
        methods.add_method("scan", |lua, this, opts: Option<Table>| {
            run_scan(lua, &this.pattern, &this.matcher, opts, "Glob:scan")
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(this.pattern.clone())
        });
    }
}

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
pub(crate) fn check_pattern(pattern: &str) -> Result<(), String> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(format!(
            "glob pattern too long ({} bytes, at most {MAX_PATTERN_BYTES})",
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
                    return Err(format!(
                        "glob pattern nests `{{` more than {MAX_BRACE_DEPTH} deep"
                    ));
                }
                if braces > MAX_BRACES {
                    return Err(format!("glob pattern has more than {MAX_BRACES} `{{`"));
                }
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// The one way the VM compiles a glob (`lib:glob`, `lib:archive`'s `include`, `lib:http`'s `serve_static`):
/// the pattern is bounded first ([`check_pattern`]), and `Glob::compile_matcher`, which panics on a pattern
/// whose regex the engine refuses, is never used. Every failure is the `Err`.
pub(crate) fn compile(pattern: &str) -> Result<GsGlob, String> {
    check_pattern(pattern)?;
    GsGlob::new(pattern).map_err(|e| e.to_string())
}

/// `pattern` compiled, as a set of one, without a path to a panic: every failure is the `Err`.
fn single(pattern: &str) -> Result<GsSet, String> {
    let mut builder = GlobSetBuilder::new();
    builder.add(compile(pattern)?);
    builder.build().map_err(|e| e.to_string())
}

fn glob_new(lua: &Lua, pattern: String) -> mlua::Result<(Value, Value)> {
    match GlobHandle::compile(&pattern) {
        Ok(g) => ok(lua, g),
        Err(msg) => err(lua, format!("glob.Glob.new: {msg}")),
    }
}

// ---------------------------------------------------------------------------
// GlobSet UserData
// ---------------------------------------------------------------------------

pub struct GlobSetHandle {
    patterns: Vec<String>,
    set: GsSet,
}

impl GlobSetHandle {
    fn is_match(&self, path: &str) -> bool {
        let normalised = normalise_input_path(path);
        // Fast path: no dotfile in the candidate → globset's automaton can
        // short-circuit on the first hit. We never need to inspect which
        // pattern matched.
        if !path_has_dotfile(&normalised) {
            return self.set.is_match(&normalised);
        }
        // Dotfile path: at least one matching pattern must explicitly allow
        // dotfiles. globset::matches walks all hits internally — we still
        // short-circuit the per-pattern filter via `.any`.
        self.set
            .matches(&normalised)
            .into_iter()
            .any(|idx| pattern_allows_dotfiles(&self.patterns[idx]))
    }

    fn matching_indices(&self, path: &str) -> Vec<usize> {
        let normalised = normalise_input_path(path);
        self.set
            .matches(&normalised)
            .into_iter()
            .filter(|idx| !is_dotfile_filtered(&self.patterns[*idx], &normalised))
            .collect()
    }
}

impl UserData for GlobSetHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_match", |_, this, path: String| Ok(this.is_match(&path)));
        methods.add_method("matches", |lua, this, path: String| {
            let t = lua.create_table()?;
            for (i, idx) in this.matching_indices(&path).into_iter().enumerate() {
                t.set(i + 1, idx + 1)?;
            }
            Ok(t)
        });
        methods.add_method("len", |_, this, _: ()| Ok(this.patterns.len() as i64));
    }
}

fn set_new(lua: &Lua, patterns: Table) -> mlua::Result<(Value, Value)> {
    let mut pats: Vec<String> = Vec::new();
    for pair in patterns.pairs::<i64, String>() {
        let (_, p) = match pair {
            Ok(v) => v,
            Err(e) => return err(lua, format!("glob.GlobSet.new: {e}")),
        };
        pats.push(p);
    }
    let mut builder = GlobSetBuilder::new();
    for p in &pats {
        let g = match compile(p) {
            Ok(g) => g,
            Err(e) => return err(lua, format!("glob.GlobSet.new: {e}")),
        };
        builder.add(g);
    }
    match builder.build() {
        Ok(set) => ok(
            lua,
            GlobSetHandle {
                patterns: pats,
                set,
            },
        ),
        Err(e) => err(lua, format!("glob.GlobSet.new: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Flat functions
// ---------------------------------------------------------------------------

#[instrument(name = "glob.scan", target = "lua::lib::glob", skip_all)]
fn flat_scan(lua: &Lua, (pattern, opts): (String, Option<Table>)) -> mlua::Result<(Value, Value)> {
    let g = match GlobHandle::compile(&pattern) {
        Ok(g) => g,
        Err(msg) => return err(lua, format!("glob.scan: {msg}")),
    };
    run_scan(lua, &g.pattern, &g.matcher, opts, "glob.scan")
}

// ---------------------------------------------------------------------------
// Scan implementation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum EntryType {
    File,
    Dir,
    Any,
}

struct ScanOpts {
    cwd: PathBuf,
    absolute: bool,
    limit: Option<usize>,
    ignore: Option<GsSet>,
    follow_symlinks: bool,
    entry_type: EntryType,
}

fn read_scan_opts(opts: Option<&Table>, prefix: &str) -> Result<ScanOpts, String> {
    let Some(tbl) = opts else {
        return Ok(ScanOpts {
            cwd: PathBuf::from("."),
            absolute: false,
            limit: None,
            ignore: None,
            follow_symlinks: false,
            entry_type: EntryType::File,
        });
    };

    let cwd: String =
        read_opt(tbl, "cwd", prefix, expect_string)?.unwrap_or_else(|| ".".to_owned());
    let absolute = read_opt(tbl, "absolute", prefix, expect_bool)?.unwrap_or(false);
    let limit = match read_opt(tbl, "limit", prefix, expect_integer)? {
        Some(n) if n < 0 => return Err(format!("{prefix}: limit must be >= 0")),
        Some(n) => Some(n as usize),
        None => None,
    };
    let follow_symlinks = read_opt(tbl, "follow_symlinks", prefix, expect_bool)?.unwrap_or(false);

    let entry_type = match read_opt(tbl, "type", prefix, expect_string)?.as_deref() {
        None | Some("file") => EntryType::File,
        Some("dir") => EntryType::Dir,
        Some("any") => EntryType::Any,
        Some(other) => return Err(format!("{prefix}: invalid type {other:?}")),
    };

    let ignore = match tbl.get::<Option<Table>>("ignore") {
        Ok(Some(ig)) => Some(build_ignore_set(&ig, prefix)?),
        Ok(None) => None,
        Err(e) => return Err(format!("{prefix}: ignore: {e}")),
    };

    Ok(ScanOpts {
        cwd: PathBuf::from(cwd),
        absolute,
        limit,
        ignore,
        follow_symlinks,
        entry_type,
    })
}

fn build_ignore_set(tbl: &Table, prefix: &str) -> Result<GsSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pair in tbl.clone().pairs::<i64, String>() {
        let (_, p) = pair.map_err(|e| format!("{prefix}: ignore: {e}"))?;
        let g = compile(&p).map_err(|e| format!("{prefix}: ignore: {e}"))?;
        builder.add(g);
    }
    builder
        .build()
        .map_err(|e| format!("{prefix}: ignore: {e}"))
}

fn run_scan(
    lua: &Lua,
    pattern: &str,
    matcher: &GsSet,
    opts: Option<Table>,
    prefix: &str,
) -> mlua::Result<(Value, Value)> {
    let scan = match read_scan_opts(opts.as_ref(), prefix) {
        Ok(o) => o,
        Err(msg) => return err(lua, msg),
    };

    if !scan.cwd.exists() {
        return err(
            lua,
            format!("{prefix}: directory not found: {}", scan.cwd.display()),
        );
    }
    if !scan.cwd.is_dir() {
        return err(
            lua,
            format!("{prefix}: not a directory: {}", scan.cwd.display()),
        );
    }

    let absolute_root = if scan.absolute {
        match scan.cwd.canonicalize() {
            Ok(p) => Some(p),
            Err(e) => return from_err(lua, prefix, e),
        }
    } else {
        None
    };

    let (results, errors) = collect_matches(pattern, matcher, &scan, absolute_root.as_deref());
    build_scan_value(lua, results, errors)
}

/// Walk `scan.cwd`, apply type/dotfile/ignore/matcher filters, return
/// (sorted+truncated matches, per-entry walk errors). Pure data — no Lua
/// allocation lives in here.
fn collect_matches(
    pattern: &str,
    matcher: &GsSet,
    scan: &ScanOpts,
    absolute_root: Option<&Path>,
) -> (Vec<String>, Vec<(String, String)>) {
    let allow_dotfiles = pattern_allows_dotfiles(pattern);
    let mut results: Vec<String> = Vec::new();
    let mut errors: Vec<(String, String)> = Vec::new();

    for entry in WalkDir::new(&scan.cwd).follow_links(scan.follow_symlinks) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push((
                    e.path().map(normalise_path).unwrap_or_default(),
                    e.to_string(),
                ));
                continue;
            }
        };

        if entry.depth() == 0 {
            continue;
        }

        let ft = entry.file_type();
        let type_ok = match scan.entry_type {
            EntryType::File => ft.is_file(),
            EntryType::Dir => ft.is_dir(),
            EntryType::Any => true,
        };
        if !type_ok {
            continue;
        }

        let rel = entry.path().strip_prefix(&scan.cwd).unwrap_or(entry.path());
        let rel_str = normalise_path(rel);

        if !allow_dotfiles && path_has_dotfile(&rel_str) {
            continue;
        }
        if let Some(set) = &scan.ignore
            && set.is_match(&rel_str)
        {
            continue;
        }
        if !matcher.is_match(&rel_str) {
            continue;
        }

        let final_path = match absolute_root {
            Some(root) => normalise_path(&root.join(&rel_str)),
            None => rel_str,
        };
        results.push(final_path);
    }

    results.sort();
    if let Some(n) = scan.limit {
        results.truncate(n);
    }
    (results, errors)
}

/// Convert the (results, errors) tuple into the `(Value, Value)` shape
/// every fallible Lua function returns.
fn build_scan_value(
    lua: &Lua,
    results: Vec<String>,
    errors: Vec<(String, String)>,
) -> mlua::Result<(Value, Value)> {
    let results_table = lua.create_table()?;
    for (i, r) in results.iter().enumerate() {
        results_table.set(i + 1, r.as_str())?;
    }

    let errors_value = if errors.is_empty() {
        Value::Nil
    } else {
        let t = lua.create_table()?;
        for (i, (p, e)) in errors.iter().enumerate() {
            let row = lua.create_table()?;
            row.set("path", p.as_str())?;
            row.set("error", e.as_str())?;
            t.set(i + 1, row)?;
        }
        Value::Table(t)
    };

    Ok((Value::Table(results_table), errors_value))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn normalise_path(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR == '/' {
        s
    } else {
        s.replace('\\', "/")
    }
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

/// Read an optional field from a Lua options table. `convert` is responsible
/// for mapping a non-Nil `Value` to `T` or returning the bare reason string
/// (this helper adds the `"{prefix}: {key}: "` envelope).
fn read_opt<T>(
    tbl: &Table,
    key: &str,
    prefix: &str,
    convert: impl FnOnce(Value) -> Result<T, String>,
) -> Result<Option<T>, String> {
    match tbl.get::<Value>(key) {
        Ok(Value::Nil) => Ok(None),
        Ok(v) => convert(v)
            .map(Some)
            .map_err(|e| format!("{prefix}: {key}: {e}")),
        Err(e) => Err(format!("{prefix}: {key}: {e}")),
    }
}

fn expect_string(v: Value) -> Result<String, String> {
    match v {
        Value::String(s) => s.to_str().map(|c| c.to_string()).map_err(|e| e.to_string()),
        other => Err(format!("must be a string, got {}", other.type_name())),
    }
}

fn expect_bool(v: Value) -> Result<bool, String> {
    match v {
        Value::Boolean(b) => Ok(b),
        other => Err(format!("must be a boolean, got {}", other.type_name())),
    }
}

fn expect_integer(v: Value) -> Result<i64, String> {
    match v {
        // Whole-valued numbers surface as `Integer` under LuaJIT.
        Value::Integer(i) => Ok(i),
        Value::Number(f) if f.is_finite() && f.fract() == 0.0 => Ok(f as i64),
        Value::Number(_) => Err("must be an integer".to_owned()),
        other => Err(format!("must be a number, got {}", other.type_name())),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use std::hint::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("glob");

    // Pattern compile + single match — pure globset path, no mlua.
    g.bench_function("library/compile_and_match", |b| {
        b.iter(|| {
            let m = single("src/**/*.rs").unwrap();
            let _ = m.is_match(black_box("src/lua/lib/glob.rs"));
        });
    });

    // Compiled matcher reused — dominant cost should be the match itself.
    let matcher = single("src/**/*.rs").unwrap();
    g.bench_function("library/match_only", |b| {
        b.iter(|| {
            let _ = matcher.is_match(black_box("src/lua/lib/glob.rs"));
        });
    });

    // End-to-end is_match call from Lua.
    let chunk: mlua::Function = lua
        .load(r#"return function(p, x) return (require("lib:glob")).is_match(p, x) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua/is_match", |b| {
        b.iter(|| {
            let _: bool = chunk
                .call((black_box("src/**/*.rs"), black_box("src/lua/lib/glob.rs")))
                .unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{MAX_BRACE_DEPTH, MAX_BRACES, MAX_PATTERN_BYTES, compile};
    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:glob")"#).eval().unwrap();
        lua.globals().set("glob", m).unwrap();
        lua
    }

    /// The bounds of the shared compile helper: a pattern at each limit still compiles, one past it is refused
    /// (every entry point that compiles a glob goes through it).
    #[test]
    fn the_shared_helper_bounds_length_and_alternation_nesting() {
        let nested = |n: usize| format!("{}a{}", "{a,".repeat(n), "}".repeat(n));
        assert!(compile(&nested(MAX_BRACE_DEPTH)).is_ok());
        assert!(
            compile(&nested(MAX_BRACE_DEPTH + 1))
                .unwrap_err()
                .contains("nests")
        );
        let flat = |n: usize| "{a,b}".repeat(n);
        assert!(compile(&flat(MAX_BRACES)).is_ok());
        assert!(
            compile(&flat(MAX_BRACES + 1))
                .unwrap_err()
                .contains("more than")
        );
        assert!(compile(&"a".repeat(MAX_PATTERN_BYTES)).is_ok());
        assert!(
            compile(&"a".repeat(MAX_PATTERN_BYTES + 1))
                .unwrap_err()
                .contains("too long")
        );
        // an escaped brace is not an alternation
        assert!(compile(&"\\{".repeat(MAX_BRACES + 1)).is_ok());
    }

    #[test]
    fn flat_is_match_basic() {
        let lua = engine();
        let ok: bool = lua
            .load(r#"return glob.is_match("src/**/*.rs", "src/lua/fs.rs")"#)
            .eval()
            .unwrap();
        assert!(ok);
        let no: bool = lua
            .load(r#"return glob.is_match("src/**/*.rs", "README.md")"#)
            .eval()
            .unwrap();
        assert!(!no);
    }

    /// `Glob::compile_matcher` compiles with `expect`, which panics when globset's regex is refused, and a panic
    /// aborts the host. Every entry point now builds a `GlobSet` and returns the error instead. This pattern is
    /// refused by `check_pattern` first (nothing within the bounds is known to reach the build error), so the
    /// test pins that each entry point reports an error, not that one particular guard fired.
    #[test]
    fn a_pattern_whose_regex_is_refused_is_an_error_not_a_panic() {
        let lua = engine();
        let pattern = format!("{}a{}", "{a,".repeat(130), "}".repeat(130));
        lua.globals().set("pattern", pattern).unwrap();
        for call in [
            "glob.is_match(pattern, 'a')",
            "glob.Glob.new(pattern)",
            "glob.GlobSet.new({ pattern })",
            "glob.scan(pattern, { cwd = '.' })",
        ] {
            let (val, e): (Value, Option<String>) = lua
                .load(format!("return {call}"))
                .eval()
                .unwrap_or_else(|e| panic!("{call}: {e}"));
            assert!(matches!(val, Value::Nil), "{call}: {val:?}");
            assert!(
                e.is_some_and(|e| e.contains("glob")),
                "{call}: no error message"
            );
        }
    }

    #[test]
    fn flat_is_match_invalid_pattern_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return glob.is_match("src/[*.rs", "src/foo.rs")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("glob.is_match:"), "got: {e}");
        assert!(e.contains("[*.rs") || e.contains("class"), "got: {e}");
    }

    #[test]
    fn glob_userdata_is_match_and_tostring() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local g = glob.Glob.new("src/**/*.rs")
                return g:is_match("src/lua/fs.rs")
            "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
        let pat: String = lua
            .load(
                r#"
                local g = glob.Glob.new("src/**/*.rs")
                return tostring(g)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(pat, "src/**/*.rs");
    }

    #[test]
    fn glob_new_invalid_pattern_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return glob.Glob.new("src/[*.rs")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("glob.Glob.new:"), "got: {e}");
    }

    #[test]
    fn globset_matches_one_based_indices() {
        let lua = engine();
        let indices: Vec<i64> = lua
            .load(
                r#"
                local s = glob.GlobSet.new({ "*.rs", "*.toml" })
                return s:matches("Cargo.toml")
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(indices, vec![2]);
    }

    #[test]
    fn globset_empty_matches_nothing() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local s = glob.GlobSet.new({})
                return s:is_match("anything")
            "#,
            )
            .eval()
            .unwrap();
        assert!(!ok);
        let n: i64 = lua
            .load(r#"return glob.GlobSet.new({}):len()"#)
            .eval()
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn globset_rejects_whole_set_on_bad_pattern() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return glob.GlobSet.new({ "valid", "inv[alid" })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("glob.GlobSet.new:"), "got: {e}");
    }

    #[test]
    fn scan_returns_sorted_results() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("c.txt"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*.txt", { cwd = cwd })"#)
            .eval()
            .unwrap();
        assert_eq!(names, vec!["a.txt", "b.txt", "c.txt"]);
    }

    #[test]
    fn scan_cwd_missing_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return glob.scan("*.rs", { cwd = "/no/such/place_xyzzy" })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("glob.scan: directory not found:"), "got: {e}");
    }

    #[test]
    fn scan_dotfile_excluded_by_default() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("visible.txt"), "").unwrap();
        std::fs::write(dir.path().join(".hidden.txt"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*", { cwd = cwd })"#)
            .eval()
            .unwrap();
        assert_eq!(names, vec!["visible.txt"]);
    }

    #[test]
    fn scan_dotfile_included_when_pattern_dotted() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan(".git*", { cwd = cwd })"#)
            .eval()
            .unwrap();
        assert_eq!(names, vec![".gitignore"]);
    }

    #[test]
    fn scan_limit_truncates() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        for n in 0..5 {
            std::fs::write(dir.path().join(format!("f{n}.txt")), "").unwrap();
        }
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*.txt", { cwd = cwd, limit = 2 })"#)
            .eval()
            .unwrap();
        assert_eq!(names, vec!["f0.txt", "f1.txt"]);
    }

    #[test]
    fn scan_limit_zero_returns_empty() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.txt"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*.txt", { cwd = cwd, limit = 0 })"#)
            .eval()
            .unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn scan_ignore_excludes_patterns() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("keep")).unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("keep").join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("target").join("b.rs"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(
                r#"return glob.scan("**/*.rs", {
                    cwd = cwd, ignore = { "**/target/**" }
                })"#,
            )
            .eval()
            .unwrap();
        assert_eq!(names, vec!["keep/a.rs"]);
    }

    #[test]
    fn scan_type_dir() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("d1")).unwrap();
        std::fs::write(dir.path().join("f1"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*", { cwd = cwd, type = "dir" })"#)
            .eval()
            .unwrap();
        assert_eq!(names, vec!["d1"]);
    }

    #[test]
    fn scan_type_any_includes_files_and_dirs() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("d1")).unwrap();
        std::fs::write(dir.path().join("f1"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*", { cwd = cwd, type = "any" })"#)
            .eval()
            .unwrap();
        assert_eq!(names, vec!["d1", "f1"]);
    }

    #[test]
    fn scan_invalid_type_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return glob.scan("*", { type = "block" })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("invalid type"), "got: {e}");
    }

    #[test]
    fn scan_absolute_returns_absolute_paths() {
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let names: Vec<String> = lua
            .load(r#"return glob.scan("*.txt", { cwd = cwd, absolute = true })"#)
            .eval()
            .unwrap();
        assert_eq!(names.len(), 1);
        // canonicalize may differ from input on macOS (/private/var); just
        // assert it's absolute and ends with the file.
        let p = std::path::Path::new(&names[0]);
        assert!(p.is_absolute(), "not absolute: {}", names[0]);
        assert!(names[0].ends_with("file.txt"), "got: {}", names[0]);
    }

    #[test]
    fn scan_limit_negative_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return glob.scan("*", { limit = -1 })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("limit must be >= 0"), "got: {e}");
    }

    #[test]
    fn round_trip_invariant() {
        // For every emitted path: glob.is_match(pattern, path) == true.
        let lua = engine();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "").unwrap();
        std::fs::write(dir.path().join("src").join("lib.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let all_match: bool = lua
            .load(
                r#"
                local pat = "**/*.rs"
                local results = glob.scan(pat, { cwd = cwd })
                for _, p in ipairs(results) do
                    if not glob.is_match(pat, p) then return false end
                end
                return #results > 0
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
                local g = glob.Glob.new("*.rs")
                local s = glob.GlobSet.new({ "*.rs" })
                return g:is_match("foo.rs") == s:is_match("foo.rs")
                   and g:is_match("foo.md") == s:is_match("foo.md")
            "#,
            )
            .eval()
            .unwrap();
        assert!(same);
    }

    /// `GlobSet:is_match` must agree with `:matches` after dotfile filtering,
    /// across the no-dotfile fast path and the dotfile-aware path. Locks the
    /// contract independently of which internal branch was taken.
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
                for _, c in ipairs(cases) do
                    local s = glob.GlobSet.new(c[1])
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

    /// Nested dotfile components are filtered, not just leaves. Pins the
    /// ripgrep/fd-style behaviour so anyone changing it sees a red test.
    #[test]
    fn nested_dotfile_component_filters_path() {
        let lua = engine();
        // Path `src/.cache/foo.rs` should NOT match `**/*.rs` — the `.cache`
        // intermediate segment is hidden and the pattern doesn't opt-in.
        let m: bool = lua
            .load(r#"return glob.is_match("**/*.rs", "src/.cache/foo.rs")"#)
            .eval()
            .unwrap();
        assert!(!m, "nested dotfile component must filter the path");
        // Opt-in: pattern with a `.` segment unlocks dotfile traversal.
        let m: bool = lua
            .load(r#"return glob.is_match("**/.cache/*.rs", "src/.cache/foo.rs")"#)
            .eval()
            .unwrap();
        assert!(m, "explicit dotted pattern segment must allow the path");
    }

    /// Permission-denied entries are surfaced through the `errors` return
    /// slot, never silently dropped. Unix-only — Windows permission model is
    /// different enough that this style of test isn't portable. Skipped when
    /// the test process can still read the chmod-000 dir (root bypasses Unix
    /// perms; the CI runner is rootful).
    #[cfg(unix)]
    #[test]
    fn scan_permission_denied_surfaces_in_errors() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("visible.txt"), "").unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("inner.txt"), "").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        if std::fs::read_dir(&locked).is_ok() {
            // Running as root — chmod is a no-op. The behaviour is still
            // covered when run by an unprivileged user; CI runs as root.
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).ok();
            return;
        }

        let lua = engine();
        lua.globals()
            .set("cwd", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let (results, errors): (Vec<String>, Option<Vec<mlua::Table>>) = lua
            .load(r#"return glob.scan("**/*.txt", { cwd = cwd })"#)
            .eval()
            .unwrap();

        // Restore perms now so tempdir Drop can clean up regardless of what
        // we assert below.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).ok();

        assert!(
            results.contains(&"visible.txt".to_owned()),
            "visible file should still match: {results:?}"
        );
        let errors = errors.expect("errors slot should be populated, not nil");
        let mut found_perm = false;
        for row in errors {
            let msg: String = row.get("error").unwrap();
            if msg.to_lowercase().contains("permission") {
                found_perm = true;
                break;
            }
        }
        assert!(found_perm, "expected a Permission error among walk errors");
    }
}
