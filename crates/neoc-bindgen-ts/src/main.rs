//! `neoc-bindgen-ts` — read a TypeScript declaration file and emit a
//! declarations-only `.neoc` module on stdout or to a file.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use neoc_bindgen_ts::ts_to_neoc;

/// Generate neoc `ext` declarations from a TypeScript `.d.ts` / `.ts` file.
#[derive(Parser)]
#[command(name = "neoc-bindgen-ts", about, version)]
struct Cli {
    /// TypeScript declaration source to read.
    file: PathBuf,

    /// Module name for the emitted `#[link("<name>")]` block. Defaults to the
    /// input file stem with any trailing `.d` removed.
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

    let neoc = ts_to_neoc(&source, &module)
        .with_context(|| format!("generating neoc from {}", cli.file.display()))?;

    match &cli.out {
        Some(path) => {
            fs::write(path, &neoc).with_context(|| format!("writing {}", path.display()))?;
        }
        None => print!("{neoc}"),
    }
    Ok(())
}

/// Derives the default module name from the input path: the file stem with a
/// trailing `.d` stripped (so `color.d.ts` -> `color`).
fn default_module(path: &std::path::Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    stem.strip_suffix(".d").unwrap_or(stem).to_string()
}
