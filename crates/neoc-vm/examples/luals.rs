//! Writes the `LuaCATS` definition file of every `std:`, `lib:` and `vnd:` module to
//! `types/luals/<ns>/<name>.lua` (`lib/json.lua` for `lib:json`, ...), or with `--check` fails when they are
//! stale.
//!
//! ```sh
//! cargo run -p neoc-vm --example luals            # regenerate
//! cargo run -p neoc-vm --example luals -- --check # fail on drift (a pre-PR check)
//! ```
//!
//! A colon is not allowed in a Windows file name, so the files are not named after the registry key; the
//! plugin `types/luals-plugin.lua` (`runtime.plugin` in `.luarc.json`) maps `require("lib:json")` to its file.
//! The definitions come from each module's recorded surface (`neoc_vm::lua::facade`).

use std::path::Path;
use std::process::ExitCode;
use std::{fs, io};

use neoc_vm::lua::modules;
use neoc_vm::lua::sandbox::make_engine;

/// Where the definitions go, relative to the repository root.
const OUT_DIR: &str = "types/luals";

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| Path::new("."));
    let out = root.join(OUT_DIR);
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(other) => {
            eprintln!("usage: luals [--check] (got `{other}`)");
            return ExitCode::from(2);
        }
    };
    let files = match definitions() {
        Ok(files) => files,
        Err(error) => {
            eprintln!("luals: {error}");
            return ExitCode::FAILURE;
        }
    };
    if check {
        let drifted = stale(&out, &files);
        if drifted.is_empty() {
            println!(
                "luals --check OK ({} module definition files up to date)",
                files.len()
            );
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "luals --check FAILED: a module's surface changed but these definition files were not regenerated:"
        );
        for path in drifted {
            eprintln!("  {OUT_DIR}/{path}");
        }
        eprintln!("run `cargo run -p neoc-vm --example luals` and commit the result");
        return ExitCode::FAILURE;
    }
    match write(&out, &files) {
        Ok(()) => {
            println!("wrote {} module definition files to {OUT_DIR}", files.len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("luals: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `(path relative to OUT_DIR, contents)` for every module.
fn definitions() -> mlua::Result<Vec<(String, String)>> {
    let lua = make_engine()?;
    let metas = modules::luals_metas(&lua)?;
    Ok(metas
        .into_iter()
        .map(|(key, text)| (modules::luals_file(&key), text))
        .collect())
}

fn write(out: &Path, files: &[(String, String)]) -> io::Result<()> {
    for path in orphans(out, files) {
        fs::remove_file(out.join(path))?;
    }
    for (name, text) in files {
        let path = out.join(name);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}

/// The file names that are missing or differ from `files`, and the module definition files that no
/// module writes any more.
fn stale(out: &Path, files: &[(String, String)]) -> Vec<String> {
    let mut stale: Vec<String> = files
        .iter()
        .filter(|(name, text)| {
            fs::read_to_string(out.join(name)).ok().as_deref() != Some(text.as_str())
        })
        .map(|(name, _)| name.clone())
        .collect();
    stale.extend(
        orphans(out, files)
            .into_iter()
            .map(|name| format!("{name} (orphaned: no module writes it; delete it)")),
    );
    stale
}

/// Every file under `out` (as a path relative to it, with `/`) that `files` does not list: `out` holds
/// generated definitions and nothing else.
fn orphans(out: &Path, files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    collect(out, "", &mut found);
    found.retain(|name| !files.iter().any(|(written, _)| written == name));
    found.sort();
    found
}

fn collect(dir: &Path, prefix: &str, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            collect(&entry.path(), &format!("{name}/"), found);
        } else {
            found.push(name);
        }
    }
}
