#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Holds the generated `LuaCATS` definitions (`types/luals/<key>.lua`) to two things.
//!
//! 1. They are what the modules record now: a module whose surface changed without
//!    `cargo run -p neoc-vm --example luals` fails here (the example's `--check` is the same test).
//! 2. lua-language-server accepts correct use and flags misuse. `types/fixtures/usage.lua` must check clean and
//!    every line of `types/fixtures/negative/*.lua` marked `-- expect-error` must be flagged, and no other
//!    line. The negative half also fails when the definitions do not load: `require("lib:json")` is then `any`
//!    and nothing is flagged.
//!
//! Half 2 needs `lua-language-server` (`brew install lua-language-server`, pinned below: its diagnostics
//! change between releases). Without it the test prints a skip notice and passes; set `NEOC_REQUIRE_LUALS=1`
//! (the pre-PR checks do) to make a missing or wrong-version tool a failure.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use neoc_vm::lua::modules;
use neoc_vm::lua::sandbox::make_engine;

/// The lua-language-server release the fixtures are pinned to.
const LUALS_VERSION: &str = "3.19.2";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// `(registry key, definition text)` for every module.
fn definitions() -> Vec<(String, String)> {
    modules::luals_metas(&make_engine().unwrap()).unwrap()
}

#[test]
fn committed_definitions_match_the_modules() {
    let dir = root().join("types/luals");
    for (key, text) in definitions() {
        let file = dir.join(format!("{key}.lua"));
        let on_disk = fs::read_to_string(&file).unwrap_or_default();
        assert!(
            on_disk == text,
            "{} is stale: run `cargo run -p neoc-vm --example luals` and commit the result",
            file.display()
        );
    }
}

#[test]
fn every_module_is_required_by_the_positive_fixture() {
    let usage = fs::read_to_string(root().join("types/fixtures/usage.lua")).unwrap();
    for (key, _) in definitions() {
        assert!(
            usage.contains(&format!("require(\"{key}\")")),
            "types/fixtures/usage.lua never requires {key}"
        );
    }
}

/// `lua-language-server` on `PATH`, or `None`.
fn find_luals() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join("lua-language-server"))
        .find(|candidate| candidate.is_file())
}

/// One diagnostic of `--check`: the 1-based line and a one-line description.
struct Finding {
    line: usize,
    text: String,
}

/// LuaLS's findings by file (a path under `types/fixtures`), from `--check` over the fixtures folder.
fn check(luals: &Path) -> Vec<(String, Vec<Finding>)> {
    let fixtures = root().join("types/fixtures");
    let scratch = env::temp_dir().join(format!("neoc-luals-{}", std::process::id()));
    let output = Command::new(luals)
        .arg("--check")
        .arg(&fixtures)
        .arg("--configpath")
        .arg(fixtures.join(".luarc.json"))
        .args(["--checklevel", "Hint", "--check_format", "json"])
        .arg("--logpath")
        .arg(scratch.join("log"))
        .arg("--metapath")
        .arg(scratch.join("meta"))
        .output()
        .unwrap();
    let report = scratch.join("log/check.json");
    // LuaLS writes the report only when it found something.
    let found = match fs::read_to_string(&report) {
        Ok(json) => serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        Err(_) => {
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.status.success() && said.contains("no problems found"),
                "lua-language-server produced no report:\n{said}"
            );
            serde_json::Value::Object(serde_json::Map::new())
        }
    };
    let _ = fs::remove_dir_all(&scratch);
    // an empty report is `[]`, a report with findings is an object keyed by file URI
    let found = found.as_object().cloned().unwrap_or_default();
    found
        .iter()
        .map(|(uri, diagnostics)| {
            let file = uri.split("types/fixtures/").last().unwrap().to_owned();
            let findings = diagnostics
                .as_array()
                .unwrap()
                .iter()
                .map(|d| Finding {
                    line: usize::try_from(d["range"]["start"]["line"].as_u64().unwrap()).unwrap()
                        + 1,
                    text: format!(
                        "[{}] {}",
                        d["code"].as_str().unwrap_or("-"),
                        d["message"].as_str().unwrap().lines().next().unwrap()
                    ),
                })
                .collect();
            (file, findings)
        })
        .collect()
}

#[test]
fn fixtures_check_against_the_generated_definitions() {
    let required = env::var_os("NEOC_REQUIRE_LUALS").is_some();
    let Some(luals) = find_luals() else {
        assert!(
            !required,
            "lua-language-server {LUALS_VERSION} not found on PATH (brew install lua-language-server)"
        );
        eprintln!("SKIPPED: lua-language-server not found; the fixtures were not checked");
        return;
    };
    let version = Command::new(&luals).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if version != LUALS_VERSION {
        assert!(
            !required,
            "{} is {version:?}, the fixtures are pinned to {LUALS_VERSION}",
            luals.display()
        );
        eprintln!(
            "SKIPPED: lua-language-server is {version:?}, the fixtures are pinned to {LUALS_VERSION}"
        );
        return;
    }

    let found = check(&luals);
    let mut problems = Vec::new();

    // positive: everything outside `negative/` checks clean
    for (file, findings) in found
        .iter()
        .filter(|(file, _)| !file.starts_with("negative/"))
    {
        for f in findings {
            problems.push(format!("{file}:{}: {}", f.line, f.text));
        }
    }

    // negative: exactly the lines marked `-- expect-error` are flagged
    let negatives = fs::read_dir(root().join("types/fixtures/negative")).unwrap();
    let mut checked = 0;
    for entry in negatives.flatten() {
        let name = format!("negative/{}", entry.file_name().to_string_lossy());
        let source = fs::read_to_string(entry.path()).unwrap();
        let expected: Vec<usize> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("-- expect-error"))
            .map(|(i, _)| i + 1)
            .collect();
        assert!(!expected.is_empty(), "{name} expects no errors");
        let findings = found
            .iter()
            .find(|(file, _)| *file == name)
            .map_or(&[][..], |(_, findings)| findings.as_slice());
        for line in &expected {
            if !findings.iter().any(|f| f.line == *line) {
                problems.push(format!(
                    "{name}:{line}: expected a diagnostic, lua-language-server accepted it"
                ));
            }
        }
        for f in findings.iter().filter(|f| !expected.contains(&f.line)) {
            problems.push(format!(
                "{name}:{}: {} (a line not marked expect-error)",
                f.line, f.text
            ));
        }
        checked += 1;
    }
    assert!(checked > 0, "no negative fixture found");
    assert!(
        problems.is_empty(),
        "lua-language-server {version}:\n{}",
        problems.join("\n")
    );
}
