//! Writes the Luau declarations of every `std:`, `lib:` and `vnd:` module to `types/neoc.d.luau`, or with
//! `--check` fails when the committed file is stale.
//!
//! ```sh
//! cargo run -p neoc-vm --example luau_decls            # regenerate
//! cargo run -p neoc-vm --example luau_decls -- --check # fail on drift (a pre-PR check)
//! ```
//!
//! One definitions file for luau-lsp: a section of `export type` aliases per module and an overloaded `require`
//! that maps `require("lib:json")` to its module type, so no file is named after a registry key (a colon is
//! not allowed in a Windows file name). The declarations come from each module's recorded surface
//! (`neoc_vm::lua::facade`).

use std::path::Path;
use std::process::ExitCode;
use std::{fs, io};

use neoc_vm::lua::modules::{self, LUAU_DECLS_PATH};
use neoc_vm::lua::sandbox::make_engine;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| Path::new("."));
    let path = root.join(LUAU_DECLS_PATH);
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(other) => {
            eprintln!("usage: luau_decls [--check] (got `{other}`)");
            return ExitCode::from(2);
        }
    };
    let text = match declarations() {
        Ok(text) => text,
        Err(error) => {
            eprintln!("luau_decls: {error}");
            return ExitCode::FAILURE;
        }
    };
    if check {
        if fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
            println!(
                "luau_decls --check OK ({LUAU_DECLS_PATH} up to date, {} modules)",
                modules::names().count()
            );
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "luau_decls --check FAILED: a module's surface changed but {LUAU_DECLS_PATH} was not regenerated"
        );
        eprintln!("run `cargo run -p neoc-vm --example luau_decls` and commit the result");
        return ExitCode::FAILURE;
    }
    match write(&path, &text) {
        Ok(()) => {
            println!(
                "wrote the declarations of {} modules to {LUAU_DECLS_PATH}",
                modules::names().count()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("luau_decls: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The text of the definitions file.
fn declarations() -> mlua::Result<String> {
    modules::luau_decls(&make_engine()?)
}

fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, text)
}
