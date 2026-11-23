//! Dev-time adapter: the generated type files. Not linked into the host.
//!
//! | Output | For | How |
//! |---|---|---|
//! | `types/generated/vos.ts` | the shell | the wire types of the `/rpc` socket via `ts-rs` (reads the serde attributes of `vos-domain`'s `protocol` and `api`), the version constants and the method table |
//! | `types/generated/vos.d.luau` | app authors (luau-lsp) | Luau declarations: the `std:`/`lib:`/`vnd:` modules an app is offered (from `types/neoc.ast.json`, the tree neoc-vm records), the `app` API and its global, and the one `declare require` |
//! | `types/generated/vos.d.ts` | app authors (TypeScript, compiled to Luau by `TypeScriptToLua`) | the same declarations in TypeScript, written from the same tree, so the two say the same |
//!
//! `bun run codegen` writes them; `bun run codegen:check` (part of `bun run lint`) fails when the committed files
//! are stale, so drift between the Rust types and a consumer is a build failure.

mod luau;
mod ts;
mod tsdecl;

pub use luau::defs::{definitions as luau_definitions, require_overloads};
pub use luau::neoc::read as neoc_modules;
pub use ts::generate as typescript;
pub use tsdecl::declarations as typescript_declarations;

use std::{fs, io, path::Path};

/// Where the tree of neoc's modules is: what neoc-vm recorded, and what the app's declarations are written from.
pub const NEOC_AST: &str = "types/neoc.ast.json";

/// One generated file: its path relative to the repository root and its contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: &'static str,
    pub contents: String,
}

/// Every generated file. `root` is the repository root, where [`NEOC_AST`] is read.
///
/// # Errors
/// neoc's modules are missing, or are not in the shape the generator reads.
pub fn generate(root: &Path) -> Result<Vec<GeneratedFile>, String> {
    let json = fs::read_to_string(root.join(NEOC_AST)).map_err(|e| format!("cannot read {NEOC_AST}: {e}"))?;
    let modules = luau::neoc::read(&json)?;
    let luau = luau::defs::definitions(&modules)?;
    let typescript = tsdecl::declarations(&modules).map_err(|e| format!("types/generated/vos.d.ts: {e}"))?;
    Ok(vec![
        GeneratedFile { path: "types/generated/vos.ts", contents: ts::generate() },
        GeneratedFile { path: "types/generated/vos.d.luau", contents: luau },
        GeneratedFile { path: "types/generated/vos.d.ts", contents: typescript },
    ])
}

/// Write every generated file under `root`.
///
/// # Errors
/// Any I/O error creating a directory or writing a file, or [`generate`] failing.
pub fn write_all(root: &Path) -> io::Result<()> {
    for file in generate(root).map_err(io::Error::other)? {
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

/// The generated files under `root` that are missing or differ from what the generator would write now, and the
/// files under `types/generated` that no generator writes any more.
#[must_use]
pub fn stale(root: &Path) -> Vec<String> {
    let files = match generate(root) {
        Ok(files) => files,
        Err(error) => return vec![format!("every generated file ({error})")],
    };
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
                !files.iter().any(|f| f.path == path) && !WRITTEN_ELSEWHERE.contains(&path.as_str())
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

    /// A copy of the repository's generated files and neoc's declarations, which the tests may edit.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vos-codegen-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("types")).unwrap();
        fs::copy(repo_root().join(NEOC_AST), dir.join(NEOC_AST)).unwrap();
        write_all(&dir).unwrap();
        dir
    }

    /// A generated file that nothing writes any more (`ui.lua` since the host stopped drawing apps' windows, the
    /// `luals/` definition files after the move to Luau declarations) kept being read by whatever globbed the
    /// directory, and `codegen:check` was green with it there.
    #[test]
    fn a_file_no_generator_writes_any_more_is_stale() {
        let dir = scratch("orphans");
        assert!(stale(&dir).is_empty(), "{:?}", stale(&dir));
        fs::write(dir.join("types/generated/ui.lua"), "-- old output").unwrap();
        fs::create_dir_all(dir.join("types/generated/luals/old")).unwrap();
        fs::write(dir.join("types/generated/luals/old/extra.lua"), "-- old output").unwrap();
        // the prelude is another generator's (scripts/codegen-lua.ts)
        fs::write(dir.join("types/generated/prelude.lua"), "-- copy").unwrap();
        // the `luals/` definitions this generator used to write
        fs::write(dir.join("types/generated/luals/vos.lua"), "---@meta").unwrap();
        let flagged = stale(&dir);
        assert_eq!(flagged.len(), 3, "{flagged:?}");
        assert!(flagged.iter().any(|p| p.starts_with("types/generated/luals/vos.lua")), "{flagged:?}");
        assert!(flagged.iter().any(|p| p.starts_with("types/generated/ui.lua")), "{flagged:?}");
        assert!(flagged.iter().any(|p| p.starts_with("types/generated/luals/old/extra.lua")), "{flagged:?}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_or_edited_file_is_stale() {
        let dir = scratch("drift");
        fs::write(dir.join("types/generated/vos.d.luau"), "-- edited").unwrap();
        fs::remove_file(dir.join("types/generated/vos.ts")).unwrap();
        assert_eq!(stale(&dir), ["types/generated/vos.ts", "types/generated/vos.d.luau"]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// The app definitions (Luau and TypeScript) are written from neoc's tree: when that changes, the committed
    /// definitions are stale (`bun run codegen:check` then asks for a regeneration), and a file of another shape is
    /// an error.
    #[test]
    fn the_definitions_follow_neoc_declarations() {
        let dir = scratch("neoc");
        let neoc = dir.join(NEOC_AST);
        let text = fs::read_to_string(&neoc).unwrap();
        fs::write(&neoc, text.replace("Read a key.", "Read a key (changed).")).unwrap();
        // both are written from the same tree
        assert_eq!(stale(&dir), ["types/generated/vos.d.luau", "types/generated/vos.d.ts"]);
        fs::write(&neoc, "-- nothing\n").unwrap();
        assert!(stale(&dir)[0].contains("is not the tree of neoc's modules"), "{:?}", stale(&dir));
        fs::remove_dir_all(dir).unwrap();
    }
}
