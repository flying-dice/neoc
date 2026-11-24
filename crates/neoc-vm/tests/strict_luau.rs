//! Every `tests/scripts/<module>.luau` is written in Luau's strict mode and type-checks against the declarations
//! this crate generates: what a script may write is what the declarations say, so a declaration that goes wrong (or
//! a script that stops being careful) fails here. So does `types/fixtures/syntax.luau`, the typed script
//! `crates/neoc/tests/cli.rs` runs with `neoc run`.
//!
//! luau-lsp does the checking. It is looked for on `PATH`, then at `~/.local/bin/luau-lsp`, as
//! `packages/vite-plugin/src/luau-tools.ts` does. Where it is not installed the test says it was skipped and passes,
//! so `cargo test` needs nothing installed; with `NEOC_REQUIRE_LUAU_LSP` set a missing luau-lsp is a failure (a
//! skipped check is not a pass).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The first line of every script.
const STRICT: &str = "--!strict";

fn scripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("scripts")
}

/// The typed script the `neoc` CLI's tests run.
fn syntax_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../types/fixtures/syntax.luau")
}

/// Every script: the module scripts, sorted, and the syntax fixture.
fn scripts() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(scripts_dir())
        .expect("the scripts directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "luau"))
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no tests/scripts/*.luau was found");
    found.push(syntax_fixture());
    found
}

/// The program `name` in `folder`: on Windows a program's file has an ending (`luau-lsp.exe`).
fn program_in(folder: &Path, name: &str) -> Option<PathBuf> {
    let endings: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    endings.iter().map(|ending| folder.join(format!("{name}{ending}"))).find(|candidate| candidate.is_file())
}

/// luau-lsp: the first on `PATH`, else `~/.local/bin/luau-lsp`.
fn luau_lsp() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .and_then(|path| std::env::split_paths(&path).find_map(|folder| program_in(&folder, "luau-lsp")));
    on_path.or_else(|| {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        program_in(&Path::new(&home).join(".local").join("bin"), "luau-lsp")
    })
}

/// The scripts whose first line is not `--!strict`.
fn not_strict(scripts: &[PathBuf]) -> Vec<String> {
    scripts
        .iter()
        .filter(|script| {
            let text = std::fs::read_to_string(script).expect("a script");
            // a checkout on Windows may end its lines with `\r\n`
            text.lines().next().map(str::trim_end) != Some(STRICT)
        })
        .map(|script| script.display().to_string())
        .collect()
}

/// The type and syntax errors in luau-lsp's plain output (`path:line:col: (W0) TypeError: message`, a message
/// continuing on the lines after), each with its continuation lines.
fn errors(output: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut in_error = false;
    for line in output.lines() {
        let starts_finding = line.contains(": (") && !line.starts_with(char::is_whitespace);
        if starts_finding {
            in_error = line.contains("TypeError") || line.contains("SyntaxError");
            if in_error {
                found.push(line.to_owned());
            }
        } else if in_error
            && !line.starts_with('[')
            && let Some(last) = found.last_mut()
        {
            last.push('\n');
            last.push_str(line);
        }
    }
    found
}

#[test]
fn every_script_starts_with_strict() {
    let loose = not_strict(&scripts());
    assert!(loose.is_empty(), "the first line of these scripts is not `{STRICT}`: {}", loose.join(", "));
}

#[test]
fn every_script_type_checks_in_strict_mode() {
    let Some(tool) = luau_lsp() else {
        let message = "luau-lsp is not installed (looked on PATH and at ~/.local/bin/luau-lsp)";
        assert!(
            std::env::var_os("NEOC_REQUIRE_LUAU_LSP").is_none(),
            "{message}, and NEOC_REQUIRE_LUAU_LSP is set: a skipped check is not a pass"
        );
        // written to the stream itself: the harness captures `println!` of a test that passes, and a skip is said
        let said = format!("strict_luau SKIPPED: {message}; the scripts were not type-checked\n");
        std::io::Write::write_all(&mut std::io::stderr(), said.as_bytes()).expect("stderr is written");
        return;
    };

    let scratch = tempfile::tempdir().expect("a temporary directory");
    let definitions = scratch.path().join("neoc.d.luau");
    std::fs::write(&definitions, neoc_vm::declarations::luau()).expect("the declarations are written");

    let run = Command::new(&tool)
        .current_dir(scripts_dir())
        .args(["analyze", "--formatter=plain", "--platform=standard"])
        .arg(format!("--definitions:@neoc={}", definitions.display()))
        .args(scripts())
        .output()
        .unwrap_or_else(|error| panic!("{} did not start: {error}", tool.display()));
    let output = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let found = errors(&output);
    assert!(
        found.is_empty(),
        "{} error(s) in strict mode against the generated declarations:\n{}",
        found.len(),
        found.join("\n")
    );
    // luau-lsp ends with 0 when it found nothing; anything else with no finding is luau-lsp itself failing
    assert!(run.status.success() || output.contains(": ("), "luau-lsp failed ({}):\n{output}", run.status);
}

#[test]
fn only_type_and_syntax_errors_are_kept_with_their_continuation() {
    let output = "[WARN] a note\n\
                  a.luau:3:1-4: (W0) TypeError: Expected this to be\n\
                  \t'string'\n\
                  but got\n\
                  \t'number'\n\
                  a.luau:9:1-4: (W1) LocalUnused: Variable 'x' is never used\n\
                  \tnot part of an error\n\
                  b.luau:1:1: (E0) SyntaxError: Expected identifier\n";
    let found = errors(output);
    assert_eq!(found.len(), 2);
    assert_eq!(
        found[0],
        "a.luau:3:1-4: (W0) TypeError: Expected this to be\n\t'string'\nbut got\n\t'number'"
    );
    assert_eq!(found[1], "b.luau:1:1: (E0) SyntaxError: Expected identifier");
}
