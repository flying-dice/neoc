//! Parse gate for runtime Lua scripts (NEOC-29).
//!
//! CI runs only a glob subset of `crates/neoc/tests` (the rest need Docker),
//! so a syntax regression in an excluded script would ship undetected. This
//! test compiles — never runs — every `.lua`/`.luau` script under
//! `crates/neoc/tests` and `crates/neoc-vm` with the embedded Luau. No
//! Docker, no network.

use std::fs;
use std::path::{Path, PathBuf};

use neoc_vm::lua::sandbox::{load_source, make_engine};

/// Lower bound on scripts found; guards against the walk silently matching nothing.
const MIN_SCRIPTS: usize = 40;

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "lua" || e == "luau") {
            out.push(path);
        }
    }
}

fn scripts() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    collect(manifest, &mut out);
    collect(&manifest.join("../neoc/tests"), &mut out);
    out.sort();
    out
}

#[test]
fn every_runtime_script_parses() {
    let scripts = scripts();
    assert!(
        scripts.len() >= MIN_SCRIPTS,
        "found only {} scripts; expected at least {MIN_SCRIPTS}",
        scripts.len()
    );
    let lua = make_engine().expect("engine builds");
    let failures: Vec<String> = scripts
        .iter()
        .filter_map(|path| {
            let src =
                fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            load_source(&lua, src.as_str())
                .set_name(format!("@{}", path.display()))
                .into_function()
                .err()
                .map(|e| format!("{}: {e}", path.display()))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "scripts failed to parse:\n{}",
        failures.join("\n")
    );
}

#[test]
fn parse_gate_rejects_invalid_syntax() {
    let lua = make_engine().expect("engine builds");
    let err = load_source(&lua, "local = = 1").into_function();
    assert!(err.is_err(), "invalid syntax must not compile");
}
