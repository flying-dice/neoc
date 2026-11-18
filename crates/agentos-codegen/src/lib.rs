//! Dev-time adapter: code generation from the domain's UI schema. Not linked into the host.
//!
//! One source of truth (the `ui` module of `agentos-domain`: Rust types plus the component catalogue tables) feeds
//! every consumer:
//!
//! | Output | For | How |
//! |---|---|---|
//! | `types/generated/agentos.ts` | the client renderer | protocol types via `ts-rs` (reads the serde attributes), component types and the runtime `COMPONENTS` table from the catalogue |
//! | `types/generated/ui.lua` | the Lua 5.1 runtime (`require("@agentos/ui")`) | the `ui.*` constructors, `ui.act`, `ui.fmt`, expression helpers (plain Lua 5.1) |
//! | `types/generated/luals/agentos.lua` | app authors (lua-language-server) | `LuaCATS`: the `app` API and its global, the core and component types, `ClientDecl` via JSON Schema (`schemars`) |
//! | `types/generated/luals/@agentos/ui.lua` | lua-language-server | `LuaCATS` for `require("@agentos/ui")`: every component's props and the `ui` functions |
//! | `types/generated/luals/@agentos/app.lua` | lua-language-server | `require("@agentos/app")` is the global `app` |
//! | `docs/reference/components.md` | everyone | the component reference |
//!
//! `bun run codegen` writes them; `bun run codegen:check` (part of `bun run lint`) fails when the committed files
//! are stale, so drift between the Rust schema and any consumer is a build failure.

mod docs;
mod lua;
mod schema_lua;
mod ts;
mod walk;

pub use lua::{
    agentos_definitions, app_module_definitions, module as ui_module, ui_module_definitions, ACTION_BUILDERS,
};
pub use schema_lua::schema_to_lua;
pub use ts::generate as typescript;

use std::{fs, io, path::Path};

/// One generated file: its path relative to the repository root and its contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: &'static str,
    pub contents: String,
}

/// Every generated file.
#[must_use]
pub fn generate() -> Vec<GeneratedFile> {
    vec![
        GeneratedFile { path: "types/generated/agentos.ts", contents: ts::generate() },
        GeneratedFile { path: "types/generated/ui.lua", contents: lua::module() },
        GeneratedFile { path: "types/generated/luals/agentos.lua", contents: lua::agentos_definitions() },
        GeneratedFile {
            path: "types/generated/luals/@agentos/ui.lua",
            contents: lua::ui_module_definitions(),
        },
        GeneratedFile {
            path: "types/generated/luals/@agentos/app.lua",
            contents: lua::app_module_definitions(),
        },
        GeneratedFile { path: "docs/reference/components.md", contents: docs::generate() },
    ]
}

/// Write every generated file under `root`.
///
/// # Errors
/// Any I/O error creating a directory or writing a file.
pub fn write_all(root: &Path) -> io::Result<()> {
    for file in generate() {
        let target = root.join(file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, file.contents)?;
    }
    Ok(())
}

/// The generated files under `root` that are missing or differ from what the generator would write now.
#[must_use]
pub fn stale(root: &Path) -> Vec<&'static str> {
    generate()
        .into_iter()
        .filter(|file| {
            fs::read_to_string(root.join(file.path)).ok().as_deref() != Some(file.contents.as_str())
        })
        .map(|file| file.path)
        .collect()
}

/// The repository root, from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap_or_else(|| Path::new("."))
}
