//! Multi-file module resolution and `STATIC.md` §2 visibility enforcement.
//!
//! A `use a::b::c;` resolves module `a::b` to a sibling `.neoc` file under the
//! entry's directory and imports its `pub` item `c`. Importing a non-`pub` item
//! or a module path that resolves to no file is a diagnostic. These tests drive
//! [`transpile_project`] against both a checked-in two-file fixture and
//! temp-directory projects built per case.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use neoc_compiler::{Stage, Target, transpile, transpile_project};

/// The checked-in `tests/fixtures/modules/` directory.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("modules")
}

/// Create a uniquely named temp directory for a per-case module project.
fn tempdir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("neoc_modules_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Write `contents` to `<dir>/<name>` and return the path.
fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, contents).expect("write fixture");
    path
}

#[test]
fn two_file_project_resolves_pub_item_and_emits_require() {
    let dir = fixtures();
    let entry = dir.join("main.neoc");
    let project = transpile_project(&entry, Target::Lua).expect("read project");

    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    // Both modules are emitted, each to its own `.lua` destination.
    assert_eq!(project.files.len(), 2, "expected main + util outputs");
    let main_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // `use util::add;` lowers to a Lua module import bringing `add` into scope.
    assert!(
        main_lua.contains("require(\"util\")") && main_lua.contains("add"),
        "main should import `add` via require:\n{main_lua}"
    );
    assert!(
        main_lua.contains("add(2, 3)"),
        "main should call the imported function:\n{main_lua}"
    );
}

#[test]
fn two_file_project_emits_esm_import_and_export_for_js() {
    let dir = fixtures();
    let entry = dir.join("main.neoc");
    let project = transpile_project(&entry, Target::Js).expect("read project");

    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );
    assert_eq!(project.files.len(), 2, "expected main + util outputs");

    let find = |stem: &str| {
        project
            .files
            .iter()
            .find(|(p, _)| p.file_stem().is_some_and(|s| s == stem))
            .map_or_else(|| panic!("{stem} output present"), |(_, src)| src.as_str())
    };

    // `use util::add;` lowers to a relative, `.js`-suffixed ESM import — the form
    // a browser or bundler resolves natively.
    let main_js = find("main");
    assert!(
        main_js.contains("import { add } from \"./util.js\";"),
        "main should import `add` as an ES module:\n{main_js}"
    );
    assert!(
        main_js.contains("add(2, 3)"),
        "main should call the imported function:\n{main_js}"
    );

    // The `pub fn add` becomes an exported binding; the private `secret` does not.
    let util_js = find("util");
    assert!(
        util_js.contains("export function add("),
        "`pub fn add` should be exported:\n{util_js}"
    );
    assert!(
        !util_js.contains("export function secret(") && util_js.contains("function secret("),
        "private `secret` should stay module-local:\n{util_js}"
    );
}

#[test]
fn using_a_private_item_is_a_visibility_error() {
    let dir = tempdir();
    write(&dir, "util.neoc", "fn secret(x: number) -> number { x }\n");
    let entry = write(
        &dir,
        "main.neoc",
        "use util::secret;\n\nfn main() -> number { secret(1) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.stage == Stage::Type
                && d.diagnostic.message == "`secret` is private in module `util`"),
        "expected a private-item diagnostic, got: {:#?}",
        project.diagnostics
    );
    // All-or-nothing: a project with a diagnostic emits no files.
    assert!(project.files.is_empty());
}

#[test]
fn unresolved_module_path_is_an_error() {
    let dir = tempdir();
    let entry = write(
        &dir,
        "main.neoc",
        "use missing::thing;\n\nfn main() -> number { thing(1) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.message == "unresolved module `missing`"),
        "expected an unresolved-module diagnostic, got: {:#?}",
        project.diagnostics
    );
}

#[test]
fn nested_module_path_resolves_under_a_subdirectory() {
    let dir = tempdir();
    write(
        &dir,
        "math/vec.neoc",
        "pub fn dot(a: number, b: number) -> number { a * b }\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use math::vec::dot;\n\nfn main() -> number { dot(2, 4) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "expected `math::vec::dot` to resolve, got: {:#?}",
        project.diagnostics
    );
}

#[test]
fn use_alias_rebinds_the_imported_name() {
    let dir = tempdir();
    write(
        &dir,
        "util.neoc",
        "pub fn add(a: number, b: number) -> number { a + b }\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use util::add as plus;\n\nfn main() -> number { plus(1, 2) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "alias import should type-check, got: {:#?}",
        project.diagnostics
    );
}

#[test]
fn single_file_project_matches_single_file_transpile() {
    // A no-`use` entry produces exactly the single-file `transpile` output, so
    // the conformance corpus path is unchanged.
    let dir = tempdir();
    let src = "fn main() -> number { 1 + 1 }\n";
    let entry = write(&dir, "solo.neoc", src);

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(project.ok());
    assert_eq!(project.files.len(), 1);
    assert_eq!(project.files[0].1, transpile(src, Target::Lua).output);
}
