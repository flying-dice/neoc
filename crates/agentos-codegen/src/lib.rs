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

/// The directory the generators own. Anything in it that is not listed in [`generate`] or [`WRITTEN_ELSEWHERE`] is
/// left over from a version that wrote more (a renamed or dropped output), and would go on being read as if it were
/// current.
const GENERATED_DIR: &str = "types/generated";

/// Files under [`GENERATED_DIR`] that another generator writes (`bun run codegen` runs all of them).
const WRITTEN_ELSEWHERE: &[&str] = &["types/generated/prelude.lua"];

/// Where `agentos-luavm`'s `luals` example writes the definitions of the `std:`, `lib:` and `vnd:` modules, one
/// file per module named after its registry key (`lib:json.lua`). That example checks its own files; this crate
/// depends on no VM crate, so it only knows not to call them orphans.
const MODULE_DEFINITIONS_DIR: &str = "types/generated/luals/";

/// Whether `path` is a module definition file written by the `luals` example.
fn is_module_definition(path: &str) -> bool {
    path.strip_prefix(MODULE_DEFINITIONS_DIR)
        .is_some_and(|name| ["std:", "lib:", "vnd:"].iter().any(|namespace| name.starts_with(namespace)))
}

/// The generated files under `root` that are missing or differ from what the generator would write now, and the
/// files under `types/generated` that no generator writes any more.
#[must_use]
pub fn stale(root: &Path) -> Vec<String> {
    let files = generate();
    let mut stale: Vec<String> = files
        .iter()
        .filter(|file| {
            fs::read_to_string(root.join(file.path)).ok().as_deref() != Some(file.contents.as_str())
        })
        .map(|file| file.path.to_owned())
        .collect();
    let mut found = Vec::new();
    list_files(root, &root.join(GENERATED_DIR), &mut found);
    found.sort();
    stale.extend(
        found
            .into_iter()
            .filter(|path| {
                !files.iter().any(|f| f.path == path)
                    && !WRITTEN_ELSEWHERE.contains(&path.as_str())
                    && !is_module_definition(path)
            })
            .map(|path| format!("{path} (orphaned: no generator writes it; delete it)")),
    );
    stale
}

/// Every file under `dir`, as a `/`-separated path relative to `root`.
fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            list_files(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(
                relative.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
            );
        }
    }
}

/// The repository root, from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap_or_else(|| Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentos-codegen-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write_all(&dir).unwrap();
        dir
    }

    /// A generated file that nothing writes any more (`ui.luau` after the move to Lua 5.1) kept being read by
    /// whatever globbed the directory, and `codegen:check` was green with it there.
    #[test]
    fn a_file_no_generator_writes_any_more_is_stale() {
        let dir = scratch("orphans");
        assert!(stale(&dir).is_empty(), "{:?}", stale(&dir));
        fs::write(dir.join("types/generated/ui.luau"), "-- old output").unwrap();
        fs::create_dir_all(dir.join("types/generated/luals/old")).unwrap();
        fs::write(dir.join("types/generated/luals/old/extra.lua"), "-- old output").unwrap();
        // the prelude is another generator's (scripts/codegen-lua.ts), and so are the module definitions
        // (`agentos-luavm`'s `luals` example, which checks them itself)
        fs::write(dir.join("types/generated/prelude.lua"), "-- copy").unwrap();
        fs::write(dir.join("types/generated/luals/lib:json.lua"), "---@meta").unwrap();
        let flagged = stale(&dir);
        assert_eq!(flagged.len(), 2, "{flagged:?}");
        assert!(flagged.iter().any(|p| p.starts_with("types/generated/ui.luau")), "{flagged:?}");
        assert!(flagged.iter().any(|p| p.starts_with("types/generated/luals/old/extra.lua")), "{flagged:?}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_or_edited_file_is_stale() {
        let dir = scratch("drift");
        fs::write(dir.join("types/generated/ui.lua"), "-- edited").unwrap();
        fs::remove_file(dir.join("docs/reference/components.md")).unwrap();
        assert_eq!(stale(&dir), ["types/generated/ui.lua", "docs/reference/components.md"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
