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
use neoc_compiler::{ProjectDiagnostic, SourceMap, Stage, Target, transpile_project};

/// The compiler subcommands: `build`, `check`, `fmt`, and `lsp`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Transpile neoc source files to a target language.
    Build {
        /// Source files or globs (for example `src/**/*.neoc`).
        #[arg(required = true)]
        paths: Vec<String>,
        /// Write output under this directory instead of next to the source.
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,
        /// Target language: `lua`, `js`, or `python`.
        #[arg(short, long, default_value_t = Target::Lua)]
        target: Target,
    },
    /// Parse and type-check without writing any output.
    Check {
        /// Source files or globs (for example `src/**/*.neoc`).
        #[arg(required = true)]
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
        }
    }
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
            // A module reachable from several entries is emitted once.
            if written.contains(&dest) {
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::write(&dest, contents).with_context(|| format!("writing {}", dest.display()))?;
            written.push(dest);
        }
    }

    Ok(i32::from(failed))
}

/// Parses and type-checks each resolved entry file and the modules its `use`
/// declarations reach, without emitting output.
fn check(patterns: &[String], err: &mut impl Write) -> Result<i32> {
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
    let project = transpile_project(file, Target::Lua)
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
        let got = out_path(Path::new("src/foo.neoc"), None, Target::Lua);
        assert_eq!(got, PathBuf::from("src/foo.lua"));
        let got = out_path(Path::new("src/foo.neoc"), None, Target::Js);
        assert_eq!(got, PathBuf::from("src/foo.js"));
        let got = out_path(Path::new("src/foo.neoc"), None, Target::Python);
        assert_eq!(got, PathBuf::from("src/foo.py"));
    }

    #[test]
    fn out_path_uses_output_dir_with_base_name() {
        let got = out_path(
            Path::new("src/nested/foo.neoc"),
            Some(Path::new("out")),
            Target::Lua,
        );
        assert_eq!(got, PathBuf::from("out/foo.lua"));
    }

    #[test]
    fn stage_label_has_no_host_internals() {
        assert_eq!(stage_label(Stage::Parse), "parse");
        assert_eq!(stage_label(Stage::Type), "type");
        assert_eq!(stage_label(Stage::Emit), "emit");
    }
}
