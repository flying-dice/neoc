//! Command logic for the neoc compiler subcommands.
//!
//! The `neoc` binary mounts [`Command`] into its top-level `clap` tree and
//! calls [`Command::run`] to dispatch. Keeping the logic in its own module lets
//! the integration tests exercise it through the built binary.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use neoc_autosql::Dialect;
use neoc_compiler::{
    Diagnostic, Project, ProjectDiagnostic, SourceMap, Stage, Target, discover_project, transpile,
    transpile_project,
};

/// The compiler subcommands: `build`, `check`, `fmt`, and `lsp`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Transpile neoc source files to a target language.
    ///
    /// With no path, builds the project discovered from the working directory
    /// (`neoc.toml`); with paths or globs, builds each file as its own entry.
    Build {
        /// Source files or globs (for example `src/**/*.neoc`). Omit to build
        /// the project rooted at a `neoc.toml` above the working directory.
        paths: Vec<String>,
        /// Write output under this directory instead of next to the source.
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,
        /// Target language: `luau`. Defaults to the project's manifest target,
        /// or `luau` for a lone file.
        #[arg(short, long)]
        target: Option<Target>,
    },
    /// Parse and type-check without writing any output.
    ///
    /// With no path, checks the project discovered from the working directory;
    /// with paths or globs, checks each file and the modules it reaches.
    Check {
        /// Source files or globs (for example `src/**/*.neoc`). Omit to check
        /// the project rooted at a `neoc.toml` above the working directory.
        paths: Vec<String>,
    },
    /// Rewrite source files to canonical form in place.
    Fmt {
        /// Source files or globs.
        #[arg(required = true)]
        paths: Vec<String>,
        /// Write nothing; exit non-zero if any file is not already formatted.
        #[arg(long)]
        check: bool,
    },
    /// Launch the language server over stdio.
    Lsp,
    /// Generate a typed neoc data-access layer from a project's SQL.
    ///
    /// Convention over flags: the project is discovered from the working
    /// directory. DDL migrations live in `src/main/sql/ddl/{n}-{name}.sql` and
    /// run in ascending `{n}` order; one query per file lives in
    /// `src/main/sql/dml/{name}.sql`. Output is written to
    /// `src/main/neoc/autosql/` — `ports.neoc`, `migrate.neoc`, and one file
    /// per query.
    Autosql {
        /// SQL dialect to parse against.
        #[arg(long, default_value_t = Dialect::Sqlite)]
        dialect: Dialect,
    },
}

impl Command {
    /// Runs the parsed command, streaming diagnostics to `err`.
    ///
    /// Returns the process exit code: `0` on success, non-zero when any file
    /// reports a diagnostic, a path matches nothing, or an I/O error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if expanding a glob or reading/writing a file fails.
    pub fn run(self, err: &mut impl Write) -> Result<i32> {
        match self {
            Command::Build {
                paths,
                output,
                target,
            } => build(&paths, output.as_deref(), target, err),
            Command::Check { paths } => check(&paths, err),

            Command::Fmt { paths, check } => fmt(&paths, check, err),
            Command::Lsp => {
                neoc_lsp::run().context("running the language server")?;
                Ok(0)
            }
            Command::Autosql { dialect } => autosql(dialect, err),
        }
    }
}

/// Generates the `autosql/` data-access layer for the project discovered from
/// the working directory. Reads DDL migrations from `src/main/sql/ddl/` (in
/// ascending numeric order) and one-query-per-file from `src/main/sql/dml/`,
/// then writes `ports.neoc`, `migrate.neoc`, and a file per query under
/// `src/main/neoc/autosql/`.
///
/// Returns a non-zero code if the SQL fails to generate (reported in neoc/SQL
/// terms, not host or `sqlparser` internals).
///
/// # Errors
///
/// Returns an error when run outside a project, or if reading an input or
/// writing an output file fails.
fn autosql(dialect: Dialect, err: &mut impl Write) -> Result<i32> {
    let Some(project) = current_project(err)? else {
        writeln!(
            err,
            "autosql: no neoc.toml found. Run inside a project — autosql reads \
             src/main/sql/ddl and src/main/sql/dml."
        )?;
        return Ok(1);
    };

    let sql_dir = project.sql_dir();
    let migrations = match read_migrations(&sql_dir.join("ddl")) {
        Ok(m) => m,
        Err(e) => {
            writeln!(err, "autosql: {e}")?;
            return Ok(1);
        }
    };
    let queries = match read_queries(&sql_dir.join("dml")) {
        Ok(q) => q,
        Err(e) => {
            writeln!(err, "autosql: {e}")?;
            return Ok(1);
        }
    };

    let files = match neoc_autosql::generate_project(&migrations, &queries, dialect) {
        Ok(files) => files,
        Err(e) => {
            writeln!(err, "autosql: {e}")?;
            return Ok(1);
        }
    };

    for file in &files {
        let dest = project.source_root.join(&file.relative_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        // Emit canonical neoc: run the generated source through the same
        // formatter `neoc fmt` uses, so output never drifts from the canonical
        // style. Fall back to the raw text if formatting ever fails (cosmetic).
        let contents = neoc_fmt::format(&file.contents).unwrap_or_else(|_| file.contents.clone());
        fs::write(&dest, &contents).with_context(|| format!("writing {}", dest.display()))?;
        writeln!(err, "autosql: wrote {}", dest.display())?;
    }
    Ok(0)
}

/// Reads `ddl/{n}-{name}.sql` migration files, parsing the leading `{n}` as the
/// apply order and the remainder as the descriptive name.
///
/// # Errors
///
/// Returns an error if the directory cannot be read, or a file name does not
/// match the `{n}-{name}.sql` convention.
fn read_migrations(dir: &Path) -> Result<Vec<neoc_autosql::Migration>> {
    let mut migrations = Vec::new();
    for path in sql_files(dir)? {
        let stem = path.file_stem().and_then(|s| s.to_str()).with_context(|| {
            format!("migration file name is not valid text: {}", path.display())
        })?;
        let (order, name) = stem.split_once('-').with_context(|| {
            format!("migration `{stem}.sql` must be named `{{n}}-{{description}}.sql`")
        })?;
        let order: u32 = order
            .parse()
            .with_context(|| format!("migration `{stem}.sql` has a non-numeric order `{order}`"))?;
        let sql =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        migrations.push(neoc_autosql::Migration {
            order,
            name: name.to_string(),
            sql,
        });
    }
    Ok(migrations)
}

/// Reads `dml/{name}.sql` query files. The file stem (kebab-case) becomes the
/// query name in `snake_case`.
///
/// # Errors
///
/// Returns an error if the directory cannot be read or a file name is invalid.
fn read_queries(dir: &Path) -> Result<Vec<neoc_autosql::Query>> {
    let mut queries = Vec::new();
    for path in sql_files(dir)? {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .with_context(|| format!("query file name is not valid text: {}", path.display()))?;
        let name = stem.replace('-', "_");
        let sql =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        queries.push(neoc_autosql::Query { name, sql });
    }
    Ok(queries)
}

/// Lists every `*.sql` file in `dir`, sorted by file name for a deterministic
/// order. A missing directory yields an empty list — a project may have only
/// DDL or only DML.
fn sql_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    files.sort();
    Ok(files)
}

/// Transpiles each resolved entry file to `target`, writing output with the
/// target's extension. An entry's `use` declarations pull in sibling modules,
/// which are resolved, checked, and emitted as part of the same project; a file
/// with no `use` builds as a single module. Skips writing every file in a
/// project that reported a diagnostic. Returns a non-zero code if anything went
/// wrong.
fn build(
    patterns: &[String],
    output: Option<&Path>,
    target: Option<Target>,
    err: &mut impl Write,
) -> Result<i32> {
    if patterns.is_empty() {
        return build_project(output, target, err);
    }
    build_paths(patterns, output, target.unwrap_or_default(), err)
}

/// Builds the manifest-driven project discovered from the working directory:
/// compiles from `src/main/neoc/main.neoc`, follows its `use` graph, and writes
/// each module to `src/auto/<target>/`, preserving its path under the source
/// root (`src/main/neoc/a/b.neoc` → `src/auto/<target>/a/b.<ext>`). An explicit
/// `--target` overrides the manifest target; an explicit `--output` writes under
/// that directory instead.
fn build_project(
    output: Option<&Path>,
    target: Option<Target>,
    err: &mut impl Write,
) -> Result<i32> {
    let Some(project) = current_project(err)? else {
        writeln!(
            err,
            "neoc build: no source given and no neoc.toml found. \
             Run inside a project or pass a file."
        )?;
        return Ok(1);
    };
    let target = target.unwrap_or(project.target);
    // Output lands in `src/auto/<target>/` by default (the generated tree
    // mirroring `src/main/`), preserving the module's path under the source
    // root (`src/main/neoc/a/b.neoc` → `src/auto/<target>/a/b.luau`). `-o`
    // overrides.
    let out_dir = output.map_or_else(|| project.output_dir(target), Path::to_path_buf);

    let compiled = transpile_project(&project.entry, target)
        .with_context(|| format!("reading {}", project.entry.display()))?;
    if !compiled.diagnostics.is_empty() {
        report_project(&compiled.diagnostics, err)?;
        return Ok(1);
    }

    // The compiler reports each module's destination keyed off its
    // canonicalised source path, so strip against the canonical source root for
    // a stable relative path (macOS `/var`↔`/private/var`, etc.).
    let source_root =
        fs::canonicalize(&project.source_root).unwrap_or_else(|_| project.source_root.clone());
    let mut written: Vec<PathBuf> = Vec::new();
    for (src_dest, contents) in compiled.files {
        let dest = project_out_path(&src_dest, &source_root, &out_dir, target);
        write_output(&dest, &contents, &mut written)?;
    }
    Ok(0)
}

/// Writes `contents` to `dest`, creating parent directories, unless `dest` was
/// already written this build (a module reachable from several entries is
/// emitted once). Records every path it writes.
fn write_output(dest: &Path, contents: &str, written: &mut Vec<PathBuf>) -> Result<()> {
    if written.contains(&dest.to_path_buf()) {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(dest, contents).with_context(|| format!("writing {}", dest.display()))?;
    written.push(dest.to_path_buf());
    Ok(())
}

/// Builds each explicit path/glob as its own entry, using single-file
/// resolution when a file is not in a project. The lone-file / glob form,
/// behaviour-identical to before projects existed.
fn build_paths(
    patterns: &[String],
    output: Option<&Path>,
    target: Target,
    err: &mut impl Write,
) -> Result<i32> {
    let files = resolve(patterns, err)?;
    if files.is_empty() {
        return Ok(1);
    }

    let mut failed = false;
    let mut written: Vec<PathBuf> = Vec::new();
    for file in &files {
        let project = transpile_project(file, target)
            .with_context(|| format!("reading {}", file.display()))?;

        if !project.diagnostics.is_empty() {
            report_project(&project.diagnostics, err)?;
            failed = true;
            continue;
        }

        for (src_dest, contents) in project.files {
            let dest = out_path(&src_dest, output, target);
            write_output(&dest, &contents, &mut written)?;
        }
    }

    Ok(i32::from(failed))
}

/// Discovers the project rooted at a `neoc.toml` above the current working
/// directory, reporting a malformed manifest in neoc terms (a non-zero return
/// the caller turns into an exit code). `Ok(None)` means no manifest was found.
fn current_project(err: &mut impl Write) -> Result<Option<Project>> {
    let cwd = std::env::current_dir().context("reading the working directory")?;
    match discover_project(&cwd) {
        Ok(found) => Ok(found),
        Err(e) => {
            writeln!(err, "neoc: {e}")?;
            // A malformed manifest is fatal; surface it rather than silently
            // falling back to single-file resolution.
            Err(anyhow::anyhow!("{e}"))
        }
    }
}

/// Parses and type-checks each resolved entry file and the modules its `use`
/// declarations reach, without emitting output.
fn check(patterns: &[String], err: &mut impl Write) -> Result<i32> {
    if patterns.is_empty() {
        return check_project(err);
    }

    let files = resolve(patterns, err)?;
    if files.is_empty() {
        return Ok(1);
    }

    let mut failed = false;
    for file in &files {
        // Parse + type errors are target-independent; check against the default.
        let project = transpile_project(file, Target::default())
            .with_context(|| format!("reading {}", file.display()))?;
        if !project.diagnostics.is_empty() {
            report_project(&project.diagnostics, err)?;
            failed = true;
        }
    }

    Ok(i32::from(failed))
}

/// Checks the manifest-driven project from its entry module, reporting
/// diagnostics without writing output.
fn check_project(err: &mut impl Write) -> Result<i32> {
    let Some(project) = current_project(err)? else {
        writeln!(
            err,
            "neoc check: no source given and no neoc.toml found. \
             Run inside a project or pass a file."
        )?;
        return Ok(1);
    };
    // Parse + type errors are target-independent; check against the default.
    let compiled = transpile_project(&project.entry, Target::default())
        .with_context(|| format!("reading {}", project.entry.display()))?;
    if !compiled.diagnostics.is_empty() {
        report_project(&compiled.diagnostics, err)?;
        return Ok(1);
    }
    Ok(0)
}

/// Rewrites each resolved file to canonical form in place. With `check` set,
/// writes nothing and reports any file that is not already formatted. Returns
/// a non-zero code on a parse failure or, in check mode, on any unformatted
/// file.
fn fmt(patterns: &[String], check: bool, err: &mut impl Write) -> Result<i32> {
    let files = resolve(patterns, err)?;
    if files.is_empty() {
        return Ok(1);
    }

    let mut failed = false;
    for file in &files {
        let source =
            fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        match neoc_fmt::format(&source) {
            Ok(formatted) => {
                if formatted == source {
                    continue;
                }
                if check {
                    writeln!(err, "{}: not formatted", file.display())?;
                    failed = true;
                } else {
                    fs::write(file, formatted)
                        .with_context(|| format!("writing {}", file.display()))?;
                }
            }
            Err(e) => {
                writeln!(err, "{}: {e}", file.display())?;
                failed = true;
            }
        }
    }

    Ok(i32::from(failed))
}

/// Expands the path patterns to a deduplicated list of files. Each pattern is
/// treated as a glob; a plain path is its own single match. Warns on any
/// pattern that matches nothing.
fn resolve(patterns: &[String], err: &mut impl Write) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for pattern in patterns {
        let mut matched = false;
        let entries =
            glob::glob(pattern).with_context(|| format!("invalid path pattern `{pattern}`"))?;
        for entry in entries {
            let path = entry.with_context(|| format!("reading match for `{pattern}`"))?;
            if path.is_file() {
                matched = true;
                if !files.contains(&path) {
                    files.push(path);
                }
            }
        }
        if !matched {
            writeln!(err, "neoc: no files matched `{pattern}`")?;
        }
    }
    Ok(files)
}

/// The destination for a source file: alongside it with the target's
/// extension, or under `output` keeping the source's file name.
fn out_path(source: &Path, output: Option<&Path>, target: Target) -> PathBuf {
    let name = source.with_extension(target.extension());
    match output {
        Some(dir) => {
            let base = name.file_name().unwrap_or_else(|| name.as_os_str());
            dir.join(base)
        }
        None => name,
    }
}

/// The destination for a project module: under `out_dir`, preserving the
/// module's path relative to `source_root`, with the target's extension
/// (`<source_root>/a/b.neoc` → `<out_dir>/a/b.<ext>`).
///
/// `source` is the module's emitted-destination path as the compiler reports it
/// — the source path with the target extension already applied. A module that
/// somehow sits outside `source_root` falls back to its file name under
/// `out_dir`, so output never escapes the directory.
fn project_out_path(source: &Path, source_root: &Path, out_dir: &Path, target: Target) -> PathBuf {
    let name = source.with_extension(target.extension());
    let rel = name
        .strip_prefix(source_root)
        .unwrap_or_else(|_| Path::new(name.file_name().unwrap_or(name.as_os_str())));
    out_dir.join(rel)
}

/// A transpiled `.neoc` entry ready to run: the emitted Lua and the line map
/// back to the neoc source, so `neoc run` can rewrite a runtime fault's Lua
/// line numbers to neoc positions.
pub struct TranspiledEntry {
    /// The emitted Lua source for the entry, with `main()` appended when the
    /// program defines a `main`.
    pub lua: String,
    /// Maps emitted-Lua lines back to neoc source lines.
    pub source_map: SourceMap,
}

/// Transpiles a single `.neoc` entry file to Lua so `neoc run` can execute it
/// in-process, with no intermediate file. Diagnostics (parse / type / emit) are
/// reported to `err` exactly as `neoc build` reports them.
///
/// Returns the emitted Lua source for the entry and its line map on success.
/// Returns `None` when the program reported a diagnostic, or when it imports
/// other modules via `use` — `run` executes a single file, and the lowered
/// `require("a.b")` calls have no resolver under the runtime's `namespace:name`
/// require, so such projects must go through `neoc build`.
///
/// # Errors
///
/// Returns an error if the entry file cannot be read.
pub fn transpile_entry_to_lua(
    file: &Path,
    err: &mut impl Write,
) -> Result<Option<TranspiledEntry>> {
    let project = transpile_project(file, Target::Luau)
        .with_context(|| format!("reading {}", file.display()))?;

    if !project.diagnostics.is_empty() {
        report_project(&project.diagnostics, err)?;
        return Ok(None);
    }

    if project.files.len() > 1 {
        writeln!(
            err,
            "neoc run: `{}` imports other modules; `run` executes a single file. \
             Build multi-module projects with `neoc build`.",
            file.display()
        )?;
        return Ok(None);
    }

    let mut files = project.files.into_iter();
    let mut maps = project.source_maps.into_iter();
    let (Some((_, mut lua)), Some(source_map)) = (files.next(), maps.next()) else {
        return Ok(None);
    };

    // A project build emits every module as a requireable library, closing with
    // a `return { … }` export table (`lang/DYNAMIC.md` §3.7). `neoc run` executes
    // the entry as a script, not a library: drop that trailing `return` so the
    // appended `main()` call is reachable (a statement after `return` is a Lua
    // syntax error). The lowered functions stay in scope as locals.
    strip_export_return(&mut lua);

    // neoc is item-only at the top level (Rust-style), so a transpiled module
    // just defines functions. `fn main` is the entry point: if the program
    // defines one, invoke it so `neoc run app.neoc` actually runs the program —
    // the call is appended to the same chunk, where the lowered
    // `local function main` is in scope.
    if defines_main(&lua) {
        lua.push_str("\nmain()\n");
    }

    Ok(Some(TranspiledEntry { lua, source_map }))
}

/// Transpiles an inline neoc source string to Lua so `neoc run --eval` can
/// execute it in-process, with no file on disk. The snippet is a single module:
/// item-only at the top level, like any `.neoc` file. Diagnostics (parse / type
/// / emit) are reported to `err` under the chunk name `<eval>`, matching how
/// `build` and `run` report a `.neoc` fault.
///
/// Returns the emitted Lua and its line map on success, or `None` when the
/// snippet reported a diagnostic. As with [`transpile_entry_to_lua`], a `main`
/// is invoked if the snippet defines one, so `neoc run --eval` runs the program;
/// a snippet that defines only other items compiles and runs to no effect.
///
/// # Errors
///
/// Returns an error only if writing a diagnostic to `err` fails. Transpiling an
/// in-memory string does no I/O.
pub fn transpile_source_to_lua(
    source: &str,
    err: &mut impl Write,
) -> Result<Option<TranspiledEntry>> {
    let compiled = transpile(source, Target::Luau);

    if !compiled.diagnostics.is_empty() {
        report_eval(&compiled.diagnostics, err)?;
        return Ok(None);
    }

    let mut lua = compiled.output;
    if defines_main(&lua) {
        lua.push_str("\nmain()\n");
    }

    Ok(Some(TranspiledEntry {
        lua,
        source_map: compiled.source_map,
    }))
}

/// Whether the emitted Lua defines a top-level `main` function — the entry
/// point `neoc run` invokes.
///
/// The codegen emits top-level items at column zero, so the check is anchored
/// there (no `trim_start`): an indented `main` inside a nested block — or the
/// substring appearing mid-line — is never mistaken for the entry point.
fn defines_main(lua: &str) -> bool {
    lua.lines()
        .any(|l| l.starts_with("local function main(") || l.starts_with("function main("))
}

/// Strip the library export table a project build appends to a module — the
/// trailing top-level `return { … }` (`lang/DYNAMIC.md` §3.7) — so `neoc run` can
/// run the entry as a script and append `main()`. A no-op when no such line is
/// present (e.g. an inline `--eval` snippet, which is never a project build). The
/// export `return` is emitted at column zero as the chunk's last non-empty line,
/// so the match is anchored there.
fn strip_export_return(lua: &mut String) {
    let Some(last) = lua
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty() && !l.starts_with("--"))
    else {
        return;
    };
    if last.starts_with("return {")
        && let Some(idx) = lua.rfind(last)
    {
        lua.truncate(idx);
    }
}

/// Writes each project diagnostic to `err` as `<path>: <stage>: <message>` —
/// the shared reporting used by `build`, `check`, and `neoc run`'s in-process
/// transpile so a `.neoc` fault reads the same regardless of entry point.
fn report_project(diagnostics: &[ProjectDiagnostic], err: &mut impl Write) -> Result<()> {
    for d in diagnostics {
        writeln!(
            err,
            "{}: {}: {}",
            d.path.display(),
            stage_label(d.diagnostic.stage),
            d.diagnostic.message
        )?;
    }
    Ok(())
}

/// Writes each diagnostic from an inline `--eval` snippet to `err` as
/// `<eval>: <stage>: <message>`. The snippet has no file path, so the chunk
/// name `<eval>` stands in for one — the same name the runtime gives the chunk,
/// so a compile diagnostic and a later runtime fault read against one source.
fn report_eval(diagnostics: &[Diagnostic], err: &mut impl Write) -> Result<()> {
    for d in diagnostics {
        writeln!(err, "<eval>: {}: {}", stage_label(d.stage), d.message)?;
    }
    Ok(())
}

/// The user-facing label for a diagnostic stage.
fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Parse => "parse",
        Stage::Type => "type",
        Stage::Emit => "emit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_path_replaces_extension_next_to_source() {
        let got = out_path(Path::new("src/foo.neoc"), None, Target::Luau);
        assert_eq!(got, PathBuf::from("src/foo.luau"));
    }

    #[test]
    fn out_path_uses_output_dir_with_base_name() {
        let got = out_path(
            Path::new("src/nested/foo.neoc"),
            Some(Path::new("out")),
            Target::Luau,
        );
        assert_eq!(got, PathBuf::from("out/foo.luau"));
    }

    #[test]
    fn project_out_path_preserves_path_under_source_root() {
        let got = project_out_path(
            Path::new("/proj/src/main/neoc/a/b.luau"),
            Path::new("/proj/src/main/neoc"),
            Path::new("/proj/src/main/luau"),
            Target::Luau,
        );
        assert_eq!(got, PathBuf::from("/proj/src/main/luau/a/b.luau"));
    }

    #[test]
    fn project_out_path_swaps_extension_for_target() {
        let got = project_out_path(
            Path::new("/proj/src/main/neoc/main.neoc"),
            Path::new("/proj/src/main/neoc"),
            Path::new("/proj/src/auto/luau"),
            Target::Luau,
        );
        assert_eq!(got, PathBuf::from("/proj/src/auto/luau/main.luau"));
    }

    #[test]
    fn project_out_path_outside_source_root_falls_back_to_file_name() {
        let got = project_out_path(
            Path::new("/elsewhere/x.luau"),
            Path::new("/proj/src/main/neoc"),
            Path::new("/proj/src/main/luau"),
            Target::Luau,
        );
        assert_eq!(got, PathBuf::from("/proj/src/main/luau/x.luau"));
    }

    #[test]
    fn stage_label_has_no_host_internals() {
        assert_eq!(stage_label(Stage::Parse), "parse");
        assert_eq!(stage_label(Stage::Type), "type");
        assert_eq!(stage_label(Stage::Emit), "emit");
    }
}
