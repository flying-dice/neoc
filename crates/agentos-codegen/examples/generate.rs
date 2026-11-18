//! `cargo run -p agentos-codegen --example generate` writes generated/{types.ts,ui.luau,schemas.json}.
#![allow(clippy::unwrap_used, clippy::cast_precision_loss)] // dev tool, not shipped
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("generated");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("types.ts"), agentos_codegen::typescript()).unwrap();
    std::fs::write(dir.join("ui.luau"), agentos_codegen::luau()).unwrap();
    let schemas: serde_json::Map<_, _> =
        agentos_codegen::schemas().into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    std::fs::write(dir.join("schemas.json"), serde_json::to_string_pretty(&schemas).unwrap()).unwrap();
}
