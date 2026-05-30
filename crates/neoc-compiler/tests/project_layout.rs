//! Project-manifest integration: a `neoc.toml` rooting a real
//! `src/main/neoc/` tree is discovered, its `use` graph resolves against the
//! source root, and `build` emits to `src/auto/<target>/` preserving paths
//! (`lang/PROJECT.md` §3–§5).
//!
//! Everything runs in a tempdir; nothing is written into the repo tree.

use std::fs;
use std::path::{Path, PathBuf};

use neoc_compiler::{Target, discover_project, resolution_root, transpile_project};

/// A unique temp directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("neoc-project-layout-{tag}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Lays out a minimal Lua project: a manifest, an entry that `use`s a sibling,
/// and the sibling itself. Returns the project root.
fn scaffold(root: &Path) {
    fs::write(
        root.join("neoc.toml"),
        "[package]\nname = \"demo\"\ntarget = \"lua\"\n",
    )
    .unwrap();
    let src = root.join("src").join("main").join("neoc");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.neoc"),
        "use greet::hello;\n\nfn main() {\n    hello();\n}\n",
    )
    .unwrap();
    fs::write(src.join("greet.neoc"), "pub fn hello() {\n}\n").unwrap();
}

#[test]
fn build_discovers_root_resolves_sibling_and_preserves_paths() {
    let tmp = TempDir::new("build");
    let root = tmp.path();
    scaffold(root);

    let project = discover_project(root).unwrap().expect("found project");
    assert_eq!(project.name, "demo");
    assert_eq!(project.target, Target::Lua);

    // The entry resolves its `use greet::hello` against the source root.
    assert_eq!(resolution_root(&project.entry), project.source_root);

    let compiled = transpile_project(&project.entry, project.target).expect("read project");
    assert!(
        compiled.diagnostics.is_empty(),
        "expected clean build: {:?}",
        compiled.diagnostics
    );
    // Both the entry and the resolved sibling are emitted.
    assert_eq!(compiled.files.len(), 2, "{:?}", compiled.files);

    // Mirror what `neoc build` writes: each module under `src/auto/<target>/`,
    // preserving its path relative to the canonical source root.
    let source_root = fs::canonicalize(&project.source_root).unwrap();
    let out_dir = project.output_dir(project.target);
    for (src_dest, contents) in &compiled.files {
        let name = src_dest.with_extension(project.target.extension());
        let rel = name.strip_prefix(&source_root).expect("under source root");
        let dest = out_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&dest, contents).unwrap();
    }

    // Output landed in src/auto/<target>/, one file per module, preserving names.
    assert!(out_dir.join("main.lua").is_file(), "main.lua missing");
    assert!(out_dir.join("greet.lua").is_file(), "greet.lua missing");
    // The source tree stays clean — nothing emitted into src/main/neoc.
    assert!(!project.source_root.join("main.lua").exists());
}

#[test]
fn nested_module_path_is_preserved_into_output() {
    let tmp = TempDir::new("nested");
    let root = tmp.path();
    fs::write(
        root.join("neoc.toml"),
        "[package]\nname = \"demo\"\ntarget = \"lua\"\n",
    )
    .unwrap();
    let src = root.join("src").join("main").join("neoc");
    let nested = src.join("a");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        src.join("main.neoc"),
        "use a::b::hello;\n\nfn main() {\n    hello();\n}\n",
    )
    .unwrap();
    fs::write(nested.join("b.neoc"), "pub fn hello() {\n}\n").unwrap();

    let project = discover_project(root).unwrap().unwrap();
    let compiled = transpile_project(&project.entry, project.target).expect("read project");
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );

    let source_root = fs::canonicalize(&project.source_root).unwrap();
    let rels: Vec<PathBuf> = compiled
        .files
        .iter()
        .map(|(p, _)| {
            p.with_extension(project.target.extension())
                .strip_prefix(&source_root)
                .unwrap()
                .to_path_buf()
        })
        .collect();
    assert!(rels.contains(&PathBuf::from("main.lua")), "{rels:?}");
    assert!(rels.contains(&PathBuf::from("a/b.lua")), "{rels:?}");
}
