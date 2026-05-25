//! The `neocc` command-line compiler. Parses arguments and dispatches into the
//! command logic in the library crate.

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;
use neoc_cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let stderr = io::stderr();
    let mut err = stderr.lock();
    match cli.run(&mut err) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            let _ = writeln!(err, "neocc: {e:#}");
            ExitCode::FAILURE
        }
    }
}
