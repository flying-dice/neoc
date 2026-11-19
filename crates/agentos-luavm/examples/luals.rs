//! Writes the `LuaCATS` definition file of every `std:`, `lib:` and `vnd:` module to
//! `types/generated/luals/<key>.lua` (`lib:json.lua`, ...), or with `--check` fails when they are stale.
//!
//! ```sh
//! cargo run -p agentos-luavm --example luals            # part of `bun run codegen`
//! cargo run -p agentos-luavm --example luals -- --check # part of `bun run codegen:check`
//! ```
//!
//! lua-language-server resolves `require("lib:json")` to the file of that name in a `workspace.library`
//! directory (`.luarc.json`), so the file name is the registry key. The definitions come from each module's
//! recorded surface (`agentos_luavm::lua::facade`); this lives here, and not in `agentos-codegen`, because
//! that crate depends on no adapter or VM crate.

use std::path::Path;
use std::process::ExitCode;
use std::{fs, io};

use agentos_luavm::lua::modules;
use agentos_luavm::lua::sandbox::make_engine;

/// Where the definitions go, relative to the repository root.
const OUT_DIR: &str = "types/generated/luals";

/// The namespaces a module key can start with; a file in [`OUT_DIR`] that starts with one and is not generated
/// is left over from a module that was renamed or dropped.
const NAMESPACES: [&str; 3] = ["std:", "lib:", "vnd:"];

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap_or_else(|| Path::new("."));
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
            println!("codegen:check OK ({} module definition files up to date)", files.len());
            return ExitCode::SUCCESS;
        }
        eprintln!("codegen:check FAILED: a module's surface changed but these definition files were not regenerated:");
        for path in drifted {
            eprintln!("  {OUT_DIR}/{path}");
        }
        eprintln!("run `bun run codegen` and commit the result");
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

/// `(file name, contents)` for every module.
fn definitions() -> mlua::Result<Vec<(String, String)>> {
    let lua = make_engine()?;
    let metas = modules::luals_metas(&lua)?;
    Ok(metas.into_iter().map(|(key, text)| (format!("{key}.lua"), text)).collect())
}

fn write(out: &Path, files: &[(String, String)]) -> io::Result<()> {
    fs::create_dir_all(out)?;
    for path in orphans(out, files) {
        fs::remove_file(out.join(path))?;
    }
    for (name, text) in files {
        fs::write(out.join(name), text)?;
    }
    Ok(())
}

/// The file names that are missing or differ from `files`, and the module definition files that no module
/// writes any more.
fn stale(out: &Path, files: &[(String, String)]) -> Vec<String> {
    let mut stale: Vec<String> = files
        .iter()
        .filter(|(name, text)| fs::read_to_string(out.join(name)).ok().as_deref() != Some(text.as_str()))
        .map(|(name, _)| name.clone())
        .collect();
    stale.extend(
        orphans(out, files)
            .into_iter()
            .map(|name| format!("{name} (orphaned: no module writes it; delete it)")),
    );
    stale
}

/// The module-named files in `out` that `files` does not list.
fn orphans(out: &Path, files: &[(String, String)]) -> Vec<String> {
    let Ok(entries) = fs::read_dir(out) else { return Vec::new() };
    let mut found: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| NAMESPACES.iter().any(|ns| name.starts_with(ns)))
        .filter(|name| !files.iter().any(|(written, _)| written == name))
        .collect();
    found.sort();
    found
}
