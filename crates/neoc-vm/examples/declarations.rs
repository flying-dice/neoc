//! Writes the declarations of every module: `cargo run -p neoc-vm --example declarations -- <dir>`
//! (`schema.json`, `types.d.luau`, `types.d.ts`; with no directory, the Luau on stdout).

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        print!("{}", neoc_vm::declarations::luau());
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let schema = serde_json::to_string_pretty(&neoc_vm::declarations::schema())?;
    std::fs::write(dir.join("schema.json"), schema + "\n")?;
    std::fs::write(dir.join("types.d.luau"), neoc_vm::declarations::luau())?;
    std::fs::write(dir.join("types.d.ts"), neoc_vm::declarations::typescript())?;
    Ok(())
}
