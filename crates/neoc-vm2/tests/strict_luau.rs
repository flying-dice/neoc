//! Every `conformance/<module>.vm2.luau` is written in Luau's strict mode and type-checks against the declarations
//! neoc-vm2 generates: what a script may write is what the declarations say, so a declaration that goes wrong (or a
//! script that stops being careful) fails here.
//!
//! luau-lsp does the checking. It is looked for on `PATH`, then at `~/.local/bin/luau-lsp`, as
//! `packages/vite-plugin/src/luau-tools.ts` does. Where it is not installed the test says it was skipped and passes,
//! so `cargo test` needs nothing installed; with `NEOC_REQUIRE_LUAU_LSP` set a missing luau-lsp is a failure (a
//! skipped check is not a pass).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The first line of every script.
const STRICT: &str = "--!strict";

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance")
}

/// The names of the vm2 scripts, sorted.
fn scripts() -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(conformance())
        .expect("the conformance directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".vm2.luau"))
        .collect();
    found.sort();
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
fn not_strict(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| {
            let text = std::fs::read_to_string(conformance().join(name)).expect("a script");
            // a checkout on Windows may end its lines with `\r\n`
            text.lines().next().map(str::trim_end) != Some(STRICT)
        })
        .cloned()
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
fn every_vm2_script_starts_with_strict() {
    let names = scripts();
    assert!(!names.is_empty(), "no conformance/*.vm2.luau was found");
    let loose = not_strict(&names);
    assert!(loose.is_empty(), "the first line of these scripts is not `{STRICT}`: {}", loose.join(", "));
}

#[test]
fn every_vm2_script_type_checks_in_strict_mode() {
    let Some(tool) = luau_lsp() else {
        let message = "luau-lsp is not installed (looked on PATH and at ~/.local/bin/luau-lsp)";
        assert!(
            std::env::var_os("NEOC_REQUIRE_LUAU_LSP").is_none(),
            "{message}, and NEOC_REQUIRE_LUAU_LSP is set: a skipped check is not a pass"
        );
        // written to the stream itself: the harness captures `println!` of a test that passes, and a skip is said
        let said = format!("strict_luau SKIPPED: {message}; the conformance scripts were not type-checked\n");
        std::io::Write::write_all(&mut std::io::stderr(), said.as_bytes()).expect("stderr is written");
        return;
    };
    let names = scripts();
    assert!(!names.is_empty(), "no conformance/*.vm2.luau was found");

    let scratch = tempfile::tempdir().expect("a temporary directory");
    let definitions = scratch.path().join("vm2.d.luau");
    std::fs::write(&definitions, neoc_vm2::declarations::luau()).expect("the declarations are written");

    let run = Command::new(&tool)
        .current_dir(conformance())
        .args(["analyze", "--formatter=plain", "--platform=standard"])
        .arg(format!("--definitions:@vm2={}", definitions.display()))
        .args(&names)
        .output()
        .unwrap_or_else(|error| panic!("{} did not start: {error}", tool.display()));
    let output = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let found = errors(&output);
    assert!(
        found.is_empty(),
        "{} error(s) in strict mode against neoc-vm2's declarations:\n{}",
        found.len(),
        found.join("\n")
    );
    // luau-lsp ends with 0 when it found nothing; anything else with no finding is luau-lsp itself failing
    assert!(run.status.success() || output.contains(": ("), "luau-lsp failed ({}):\n{output}", run.status);
}

#[test]
fn only_type_and_syntax_errors_are_kept_with_their_continuation() {
    let output = "[WARN] a note\n\
                  a.vm2.luau:3:1-4: (W0) TypeError: Expected this to be\n\
                  \t'string'\n\
                  but got\n\
                  \t'number'\n\
                  a.vm2.luau:9:1-4: (W1) LocalUnused: Variable 'x' is never used\n\
                  \tnot part of an error\n\
                  b.vm2.luau:1:1: (E0) SyntaxError: Expected identifier\n";
    let found = errors(output);
    assert_eq!(found.len(), 2);
    assert_eq!(
        found[0],
        "a.vm2.luau:3:1-4: (W0) TypeError: Expected this to be\n\t'string'\nbut got\n\t'number'"
    );
    assert_eq!(found[1], "b.vm2.luau:1:1: (E0) SyntaxError: Expected identifier");
}
