//! `neoc-bindgen-lua` — read a Luau source file and emit a declarations-only
//! `.neoc` header on stdout or to a file.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use neoc_bindgen_lua::luau_to_neoc;

/// Generate neoc `ext` declarations from a Luau source file.
#[derive(Parser)]
#[command(name = "neoc-bindgen-lua", about, version)]
struct Cli {
    /// Luau source to read.
    file: PathBuf,

    /// Module name for the emitted `#[link("<name>")]` block. Defaults to the
    /// input file stem.
    #[arg(long)]
    module: Option<String>,

    /// Write output here instead of stdout.
    #[arg(short = 'o', long)]
    out: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let source =
        fs::read_to_string(&cli.file).with_context(|| format!("reading {}", cli.file.display()))?;

    let module = cli.module.unwrap_or_else(|| default_module(&cli.file));

    let neoc = luau_to_neoc(&source, &module)
        .with_context(|| format!("generating neoc from {}", cli.file.display()))?;

    match &cli.out {
        Some(path) => {
            fs::write(path, &neoc).with_context(|| format!("writing {}", path.display()))?;
        }
        None => print!("{neoc}"),
    }
    Ok(())
}

/// Derives the default module name from the input path: the file stem.
fn default_module(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}
