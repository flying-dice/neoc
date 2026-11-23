//! Writes the Luau declarations of every `std:`, `lib:` and `vnd:` module to `types/neoc.d.luau`, or with
//! `--check` fails when the committed file is stale.
//!
//! ```sh
//! cargo run -p neoc-vm --example luau_decls            # regenerate
//! cargo run -p neoc-vm --example luau_decls -- --check # fail on drift (a pre-PR check)
//! ```
//!
//! `--ast` writes (or checks) the same declarations as a tree, `types/neoc.ast.json`: what a generator for another
//! language reads. `--out <file>` writes (or checks) another file than the committed one. Exit codes: 0 done, 1 stale,
//! missing, not writable or a record the emitter refuses, 2 bad arguments. `tests/luau_decls.rs` runs the example for
//! 0, for 1 (stale, missing, not writable) and for 2; a record the emitter refuses is unit-tested in
//! `facade/luau_decls.rs` (no real module has one).
//!
//! One definitions file for luau-lsp: a section of `export type` aliases per module and an overloaded `require`
//! that maps `require("lib:json")` to its module type, so no file is named after a registry key (a colon is
//! not allowed in a Windows file name). The declarations come from each module's recorded surface
//! (`neoc_vm::lua::facade`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{fs, io};

use neoc_vm::lua::modules::{self, AST_PATH, LUAU_DECLS_PATH};
use neoc_vm::lua::sandbox::make_engine;

const USAGE: &str = "usage: luau_decls [--ast] [--check] [--out <file>]";

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| Path::new("."));
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(&args, root)
}

/// What the arguments ask for.
struct Asked {
    /// Compare instead of writing.
    check: bool,
    /// The tree (`--ast`) and not the Luau text.
    ast: bool,
    /// Another file than the committed one (the tests use it).
    out: Option<PathBuf>,
}

/// What the arguments say. `Err` is a usage error.
fn parse(args: &[String]) -> Result<Asked, String> {
    let mut check = false;
    let mut ast = false;
    let mut out = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--ast" => ast = true,
            "--out" => {
                let file = args.next().ok_or("`--out` needs a file")?;
                out = Some(PathBuf::from(file));
            }
            other => return Err(format!("got `{other}`")),
        }
    }
    Ok(Asked { check, ast, out })
}

/// Writes (or with `--check` compares) the declarations: exit 0 on success, 1 when the file is stale or
/// missing, cannot be written or a record cannot be emitted, 2 on a usage error.
fn run(args: &[String], root: &Path) -> ExitCode {
    let Asked { check, ast, out } = match parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("luau_decls: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let committed = root.join(if ast { AST_PATH } else { LUAU_DECLS_PATH });
    let path = out.as_deref().unwrap_or(&committed);
    let shown = path.display();
    let flag = if ast { " -- --ast" } else { "" };
    let text = match declarations(ast) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("luau_decls: {error}");
            return ExitCode::FAILURE;
        }
    };
    if check {
        if fs::read_to_string(path).ok().as_deref() == Some(text.as_str()) {
            println!(
                "luau_decls --check OK ({shown} up to date, {} modules)",
                modules::names().count()
            );
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "luau_decls --check FAILED: {shown} is missing or does not match what the modules record"
        );
        eprintln!("run `cargo run -p neoc-vm --example luau_decls{flag}` and commit the result");
        return ExitCode::FAILURE;
    }
    match write(path, &text) {
        Ok(()) => {
            println!(
                "wrote the declarations of {} modules to {shown}",
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

/// The text of the definitions file, or of the tree.
fn declarations(ast: bool) -> mlua::Result<String> {
    let lua = make_engine()?;
    if ast {
        modules::ast_json(&lua)
    } else {
        modules::luau_decls(&lua)
    }
}

fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, text)
}
