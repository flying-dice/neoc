//! `agentos-codegen`: writes the generated type files, or with `--check` fails when they are stale.
//!
//! ```sh
//! cargo run -p agentos-codegen          # bun run codegen
//! cargo run -p agentos-codegen -- --check   # bun run codegen:check
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let root = agentos_codegen::repo_root();
    match std::env::args().nth(1).as_deref() {
        None => match agentos_codegen::write_all(root) {
            Ok(()) => {
                for file in agentos_codegen::generate() {
                    println!("wrote {}", file.path);
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("codegen: {error}");
                ExitCode::FAILURE
            }
        },
        Some("--check") => {
            let stale = agentos_codegen::stale(root);
            if stale.is_empty() {
                println!(
                    "codegen:check OK ({} generated files up to date)",
                    agentos_codegen::generate().len()
                );
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "codegen:check FAILED: the Rust schema changed but these files were not regenerated:"
                );
                for path in stale {
                    eprintln!("  {path}");
                }
                eprintln!("run `bun run codegen` and commit the result");
                ExitCode::FAILURE
            }
        }
        Some(other) => {
            eprintln!("usage: agentos-codegen [--check] (got `{other}`)");
            ExitCode::from(2)
        }
    }
}
