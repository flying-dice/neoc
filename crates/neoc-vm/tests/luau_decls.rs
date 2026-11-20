#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Holds the generated Luau declarations (`types/neoc.d.luau`) to two things.
//!
//! 1. They are what the modules record now: a module whose surface changed without
//!    `cargo run -p neoc-vm --example luau_decls` fails here (the example's `--check` reports the same).
//! 2. luau-lsp accepts correct use and flags misuse, in strict mode. Every file of `types/fixtures/` outside
//!    `negative/` must check clean (no type error, no lint warning), and every line of
//!    `types/fixtures/negative/*.luau` marked `-- expect-error: <text>` must carry a diagnostic whose message
//!    contains `<text>`, and no other line may carry one. The negative half also fails when the declarations
//!    or the overloaded `require` do not load: `require("lib:json")` is then `any` and nothing is flagged.
//!
//! Half 2 needs `luau-lsp` (a release binary from <https://github.com/JohnnyMorganz/luau-lsp/releases>, pinned below: its messages change between releases).
//! Without it the test prints a skip notice and passes; set `NEOC_REQUIRE_LUAU_LSP=1` (the pre-PR checks do)
//! to make a missing or wrong-version tool a failure.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use neoc_vm::lua::modules::{self, LUAU_DECLS_PATH};
use neoc_vm::lua::sandbox::make_engine;

/// The luau-lsp release the fixtures are pinned to.
const LUAU_LSP_VERSION: &str = "1.70.1";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[test]
fn committed_declarations_match_the_modules() {
    let text = modules::luau_decls(&make_engine().unwrap()).unwrap();
    let current = fs::read_to_string(root().join(LUAU_DECLS_PATH)).unwrap_or_default();
    assert!(
        current == text,
        "{LUAU_DECLS_PATH} is stale: run `cargo run -p neoc-vm --example luau_decls` and commit the result"
    );
}

/// A colon is not allowed in a Windows file name: no tracked file may have one, so the repository clones there.
#[test]
fn no_file_name_in_the_repository_has_a_colon() {
    let output = Command::new("git")
        .args(["ls-files"])
        .current_dir(root())
        .output();
    let Ok(output) = output else {
        return; // not a git checkout, nothing to check
    };
    if !output.status.success() {
        return;
    }
    let listed = String::from_utf8_lossy(&output.stdout);
    let bad: Vec<&str> = listed.lines().filter(|l| l.contains(':')).collect();
    assert!(bad.is_empty(), "file names with a colon: {bad:?}");
}

#[test]
fn every_module_is_required_by_the_positive_fixture() {
    let usage = fs::read_to_string(root().join("types/fixtures/usage.luau")).unwrap();
    for key in modules::names() {
        assert!(
            usage.contains(&format!("require(\"{key}\")")),
            "types/fixtures/usage.luau never requires {key}"
        );
    }
}

/// `luau-lsp` on `PATH`, or `None`.
fn find_luau_lsp() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join("luau-lsp"))
        .find(|candidate| candidate.is_file())
}

/// One diagnostic of `luau-lsp analyze`: the 1-based line and the message (every line of it).
struct Finding {
    line: usize,
    text: String,
}

/// The `.luau` files of `dir` (`types/fixtures/...`), as paths relative to the repository root, sorted.
fn fixtures_in(dir: &str) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(root().join(dir))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "luau"))
        .map(|e| format!("{dir}/{}", e.file_name().to_string_lossy()))
        .collect();
    found.sort();
    found
}

/// luau-lsp's findings by file, from `analyze --formatter=plain` over `files` with the declarations loaded.
///
/// The plain format is `path:line:col-col: (W0) Kind: message`, a message of several lines continuing on the
/// lines after it. Any other output (`[ERROR]` when the definitions file does not load, a syntax error in it)
/// fails the run: a broken declarations file must not look like a clean one.
fn analyze(luau_lsp: &Path, files: &[String]) -> Vec<(String, Vec<Finding>)> {
    let output = Command::new(luau_lsp)
        .current_dir(root())
        .args(["analyze", "--formatter=plain", "--definitions"])
        .arg(LUAU_DECLS_PATH)
        .args(files)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found: Vec<(String, Vec<Finding>)> = Vec::new();
    for line in stderr
        .lines()
        .chain(String::from_utf8_lossy(&output.stdout).lines())
    {
        if line.starts_with("[INFO]") {
            continue;
        }
        assert!(
            !line.starts_with("[ERROR]") && !line.contains("neoc.d.luau"),
            "luau-lsp could not use {LUAU_DECLS_PATH}: {line}"
        );
        let diagnostic = files.iter().find_map(|file| {
            let rest = line.strip_prefix(file.as_str())?.strip_prefix(':')?;
            let (number, rest) = rest.split_once(':')?;
            let message = rest.split_once(") ")?.1;
            Some((file, number.parse::<usize>().ok()?, message))
        });
        if let Some((file, number, message)) = diagnostic {
            if !found.iter().any(|(name, _)| name == file) {
                found.push((file.clone(), Vec::new()));
            }
            let (_, findings) = found.iter_mut().find(|(name, _)| name == file).unwrap();
            findings.push(Finding {
                line: number,
                text: message.to_owned(),
            });
        } else if let Some((_, findings)) = found.last_mut() {
            // the rest of the message above
            let last = findings.last_mut().expect("a finding opens a file");
            last.text.push('\n');
            last.text.push_str(line);
        } else {
            panic!("unexpected luau-lsp output: {line}");
        }
    }
    found
}

/// The lines of `source` marked `-- expect-error: <text>`, as `(1-based line, text)`.
fn expected_errors(name: &str, source: &str) -> Vec<(usize, String)> {
    source
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let marker = line.split("-- expect-error").nth(1)?;
            let text = marker.strip_prefix(':').map(str::trim);
            Some((
                i + 1,
                text.filter(|t| !t.is_empty())
                    .unwrap_or_else(|| {
                        panic!("{name}:{}: `-- expect-error` needs a `: <text>`", i + 1)
                    })
                    .to_owned(),
            ))
        })
        .collect()
}

#[test]
fn fixtures_check_against_the_generated_declarations() {
    let required = env::var("NEOC_REQUIRE_LUAU_LSP").is_ok_and(|v| !v.is_empty() && v != "0");
    let Some(luau_lsp) = find_luau_lsp() else {
        assert!(
            !required,
            "luau-lsp {LUAU_LSP_VERSION} not found on PATH (release binaries: https://github.com/JohnnyMorganz/luau-lsp/releases)"
        );
        eprintln!("SKIPPED: luau-lsp not found; the fixtures were not checked");
        return;
    };
    let version = Command::new(&luau_lsp).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if version != LUAU_LSP_VERSION {
        assert!(
            !required,
            "{} is {version:?}, the fixtures are pinned to {LUAU_LSP_VERSION}",
            luau_lsp.display()
        );
        eprintln!(
            "SKIPPED: luau-lsp is {version:?}, the fixtures are pinned to {LUAU_LSP_VERSION}"
        );
        return;
    }

    let positives = fixtures_in("types/fixtures");
    let negatives = fixtures_in("types/fixtures/negative");
    assert!(
        positives.len() >= 2 && negatives.len() >= 2,
        "fixtures missing: {positives:?} {negatives:?}"
    );
    let all: Vec<String> = positives.iter().chain(&negatives).cloned().collect();
    let found = analyze(&luau_lsp, &all);
    let mut problems = Vec::new();

    // positive: every fixture outside `negative/` checks clean
    for file in &positives {
        let source = fs::read_to_string(root().join(file)).unwrap();
        assert!(
            source.starts_with("--!strict\n"),
            "{file} must opt in to strict mode"
        );
        if let Some((_, findings)) = found.iter().find(|(name, _)| name == file) {
            for f in findings {
                problems.push(format!("{file}:{}: {}", f.line, f.text));
            }
        }
    }

    // negative: exactly the lines marked `-- expect-error` are flagged, with the text that was marked
    let mut marked = 0;
    for file in &negatives {
        let source = fs::read_to_string(root().join(file)).unwrap();
        assert!(
            source.starts_with("--!strict\n"),
            "{file} must opt in to strict mode"
        );
        let expected = expected_errors(file, &source);
        assert!(!expected.is_empty(), "{file} expects no errors");
        let findings = found
            .iter()
            .find(|(name, _)| name == file)
            .map_or(&[][..], |(_, findings)| findings.as_slice());
        for (line, text) in &expected {
            let on_line: Vec<&Finding> = findings.iter().filter(|f| f.line == *line).collect();
            if on_line.is_empty() {
                problems.push(format!(
                    "{file}:{line}: expected `{text}`, luau-lsp accepted it"
                ));
            } else if !on_line.iter().any(|f| f.text.contains(text.as_str())) {
                let got: Vec<&str> = on_line.iter().map(|f| f.text.as_str()).collect();
                problems.push(format!("{file}:{line}: expected `{text}`, got {got:?}"));
            }
            marked += 1;
        }
        for f in findings
            .iter()
            .filter(|f| !expected.iter().any(|(line, _)| *line == f.line))
        {
            problems.push(format!(
                "{file}:{}: {} (a line not marked expect-error)",
                f.line, f.text
            ));
        }
    }
    assert!(
        marked >= 25,
        "only {marked} expected errors in the negative fixtures"
    );
    assert!(
        problems.is_empty(),
        "luau-lsp {version}:\n{}",
        problems.join("\n")
    );
}
