//! Multi-file module resolution and `STATIC.md` §2 visibility enforcement.
//!
//! A `use a::b::c;` resolves module `a::b` to a sibling `.neoc` file under the
//! entry's directory and imports its `pub` item `c`. Importing a non-`pub` item
//! or a module path that resolves to no file is a diagnostic. These tests drive
//! [`transpile_project`] against both a checked-in two-file fixture and
//! temp-directory projects built per case.
//!
//! neoc emits the typed supersets: `luau` (downgrades to Lua) and `ts`
//! (downgrades to JavaScript). The cross-module lowering is shared, so these
//! tests assert against `luau` and `ts`.

use std::path::{Path, PathBuf};
use std::process::Command;
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
    let project = transpile_project(&entry, Target::Luau).expect("read project");

    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    // Both modules are emitted, each to its own `.luau` destination.
    assert_eq!(project.files.len(), 2, "expected main + util outputs");
    let main_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // `use util::add;` lowers to a Luau module import bringing `add` into scope.
    assert!(
        main_luau.contains("require(\"util\")") && main_luau.contains("add"),
        "main should import `add` via require:\n{main_luau}"
    );
    assert!(
        main_luau.contains("add(2, 3)"),
        "main should call the imported function:\n{main_luau}"
    );

    // The producer must be a requireable library: `pub fn add` is a `local` and
    // the module closes with a `return { … }` export table carrying `add`, so the
    // consumer's `require("util").add` resolves (`lang/DYNAMIC.md` §3.7).
    let util_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "util"))
        .map(|(_, src)| src.as_str())
        .expect("util output present");
    assert!(
        util_luau.contains("local function add("),
        "`pub fn add` should be a local in a library build:\n{util_luau}"
    );
    assert!(
        util_luau.contains("add = add"),
        "util should export `add` in its return table:\n{util_luau}"
    );
}

#[test]
fn two_file_project_emits_esm_import_and_export_for_ts() {
    let dir = fixtures();
    let entry = dir.join("main.neoc");
    let project = transpile_project(&entry, Target::Ts).expect("read project");

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

    // `use util::add;` lowers to a relative, extensionless ESM import — the form
    // a bundler/`bun` resolves natively, and the `.js` downgrade keeps working.
    let main_ts = find("main");
    assert!(
        main_ts.contains("import { add } from \"./util\";"),
        "main should import `add` as an ES module:\n{main_ts}"
    );
    assert!(
        main_ts.contains("add(2, 3)"),
        "main should call the imported function:\n{main_ts}"
    );

    // The `pub fn add` becomes an exported binding; the private `secret` does not.
    let util_ts = find("util");
    assert!(
        util_ts.contains("export function add("),
        "`pub fn add` should be exported:\n{util_ts}"
    );
    assert!(
        !util_ts.contains("export function secret(") && util_ts.contains("function secret("),
        "private `secret` should stay module-local:\n{util_ts}"
    );
}

#[test]
fn subdirectory_module_imports_a_sibling_relative_to_itself_in_ts() {
    // Regression: a module inside a subdirectory importing a sibling in the SAME
    // subdirectory must emit an ESM specifier relative to the *importer*
    // (`./b`), not relative to the source root (`./sub/b`). The rooted form
    // resolves to `sub/sub/b` from inside `sub/` and fails to load — the bug the
    // convention-driven generators' `autosql/` output first exposed.
    let dir = tempdir();
    write(&dir, "sub/b.neoc", "pub fn b() -> number { 7 }\n");
    write(
        &dir,
        "sub/a.neoc",
        "use sub::b::b;\n\npub fn a() -> number { b() }\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use sub::a::a;\n\nfn main() -> number { a() }\n",
    );

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let a_ts = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "a"))
        .map_or_else(|| panic!("sub/a output present"), |(_, src)| src.as_str());

    assert!(
        a_ts.contains("import { b } from \"./b\";"),
        "a subdirectory module should import its sibling relative to itself:\n{a_ts}"
    );
    assert!(
        !a_ts.contains("./sub/b"),
        "the import must not be rooted at the source root:\n{a_ts}"
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

    let project = transpile_project(&entry, Target::Luau).expect("read project");
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

    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.message
                == "unresolved module `missing` (no file at `missing.neoc`)"),
        "expected an unresolved-module diagnostic, got: {:#?}",
        project.diagnostics
    );
}

#[test]
fn unresolved_nested_module_names_the_expected_file() {
    // A nested module path points the author at the source-root-relative file —
    // `autosql::ports` → `autosql/ports.neoc` — so the fixed layout is
    // discoverable from the diagnostic alone.
    let dir = tempdir();
    let entry = write(
        &dir,
        "main.neoc",
        "use autosql::ports::SqlExecutor;\n\nfn main() -> number { 1 }\n",
    );

    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.diagnostics.iter().any(|d| d.diagnostic.message
            == "unresolved module `autosql::ports` (no file at `autosql/ports.neoc`)"),
        "expected the nested module's file path in the diagnostic, got: {:#?}",
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

    let project = transpile_project(&entry, Target::Luau).expect("read project");
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

    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.ok(),
        "alias import should type-check, got: {:#?}",
        project.diagnostics
    );
}

#[test]
fn single_file_project_emits_a_requireable_library() {
    // A project build emits every module as a requireable library, so the entry
    // closes with a `return { … }` export table even with nothing `pub` (here,
    // empty) — the one divergence from single-file `transpile`, which stays a
    // directly-runnable script with no trailing return (`lang/DYNAMIC.md` §3.7).
    // Past that line, the body is the single-file `transpile` output. No backend
    // emits a source map yet, so there is no `.map` sidecar or trailer.
    let dir = tempdir();
    let src = "fn main() -> number { 1 + 1 }\n";
    let entry = write(&dir, "solo.neoc", src);

    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(project.ok());
    assert_eq!(project.files.len(), 1);
    let emitted = &project.files[0].1;
    // The script body matches single-file `transpile`; the library form adds the
    // export table (no `pub` items here, so an empty one).
    let expected = format!("{}return {{}}\n", transpile(src, Target::Luau).output);
    assert_eq!(emitted, &expected);
    // No source map yet, so no aux sidecars.
    assert!(
        project.aux_files.is_empty(),
        "Luau emits no aux (self-contained), got: {:?}",
        project.aux_files
    );
}

#[test]
fn ts_build_emits_dts_via_self_typed_modules_and_shared_neoc_lib() {
    // A TS-target project emits a self-typed `.ts` per module in `files`
    // (projecting each module's `pub` exports inline) and, once at the resolution
    // root, the shared `neoc-lib.js` + `neoc-lib.d.ts` prelude. The prelude rides
    // in `aux_files`, not `files`, so the `files`↔`source_maps` positional
    // contract is unaffected.
    let dir = tempdir();
    write(&dir, "y.neoc", "pub fn helper() -> number { 7 }\n");
    let entry = write(
        &dir,
        "x.neoc",
        "use y::helper;\n\
         \n\
         pub struct Row { id: number, body: string }\n\
         \n\
         pub fn load(id: number) -> Result<Row, string> {\n\
         \x20   Ok(Row { id: id + helper(), body: \"ok\" })\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    // One `.ts` per module, aligned with `source_maps`.
    assert_eq!(project.files.len(), 2, "expected x.ts + y.ts");
    assert_eq!(
        project.files.len(),
        project.source_maps.len(),
        "files and source_maps must stay positionally aligned"
    );

    let module = |name: &str| {
        project
            .files
            .iter()
            .find(|(p, _)| p.file_name().is_some_and(|f| f == name))
            .map_or_else(|| panic!("{name} not in files"), |(_, c)| c.as_str())
    };

    // x.ts is self-typed: the `pub struct` is an exported interface, the `pub fn`
    // returning `Result` carries an inline signature.
    let x_ts = module("x.ts");
    assert!(
        x_ts.contains("export interface Row { id: number; body: string; }"),
        "x.ts should project the `pub struct`:\n{x_ts}"
    );
    assert!(
        x_ts.contains("export function load(id: number): Result<Row, string> {"),
        "x.ts should carry the inline-typed `pub fn` returning Result:\n{x_ts}"
    );
    assert!(
        x_ts.contains("import type { Result, Option } from \"./neoc-lib\";"),
        "x.ts should import Result/Option from the shared neoc-lib:\n{x_ts}"
    );

    let aux = |name: &str| {
        project
            .aux_files
            .iter()
            .find(|(p, _)| p.file_name().is_some_and(|f| f == name))
            .map_or_else(|| panic!("{name} not in aux_files"), |(_, c)| c.as_str())
    };

    // The shared prelude is emitted once, at the resolution root.
    assert!(
        aux("neoc-lib.js").contains("export { Ok, Err, Some, None, Option$, Result$ };"),
        "neoc-lib.js should export the Result/Option runtime"
    );
    assert!(
        aux("neoc-lib.d.ts").contains("export declare function Ok"),
        "neoc-lib.d.ts should declare the prelude types"
    );
    let lib_at_root = project
        .aux_files
        .iter()
        .filter(|(p, _)| p.file_name().is_some_and(|f| f == "neoc-lib.js"))
        .count();
    assert_eq!(lib_at_root, 1, "neoc-lib.js emitted exactly once");
}

#[test]
fn luau_build_emits_self_typed_module_files_with_no_aux() {
    // A Luau-target project emits a self-typed `.luau` *per module* in `files`
    // (the artifact, positionally aligned with `source_maps`). Each module is
    // self-contained: it inlines the runtime and its own `Result`/`Option` type
    // aliases, so there are **no** aux files — no `neoc-lib`, no `.d.lua`, no
    // `.map`. The source map is empty (none yet for Luau).
    let dir = tempdir();
    let entry = write(
        &dir,
        "x.neoc",
        "pub struct Row { id: number, body: string }\n\
         \n\
         pub fn load(id: number) -> Result<Row, string> {\n\
         \x20   Ok(Row { id: id, body: \"ok\" })\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    // The `.luau` module is the artifact, aligned with `source_maps`.
    assert_eq!(project.files.len(), 1, "expected x.luau");
    assert_eq!(
        project.files.len(),
        project.source_maps.len(),
        "files and source_maps must stay positionally aligned"
    );
    let (path, x_luau) = &project.files[0];
    assert!(
        path.file_name().is_some_and(|f| f == "x.luau"),
        "expected x.luau, got {path:?}"
    );

    // x.luau is self-typed: the `pub struct` is an inline `type`, the `pub fn`
    // returning `Result` carries an inline-typed signature.
    assert!(
        x_luau.contains("type Row = {"),
        "x.luau should carry the inline `type` for the `pub struct`:\n{x_luau}"
    );
    assert!(
        x_luau.contains("): Result<"),
        "x.luau should carry the inline-typed `pub fn` returning Result:\n{x_luau}"
    );

    // Self-contained: no aux files at all.
    assert!(
        project.aux_files.is_empty(),
        "Luau emits no aux (self-contained), got: {:?}",
        project.aux_files
    );
}

#[test]
fn python_build_emits_pyi_siblings_and_shared_neoc_lib() {
    // A Python-target project emits the module artifact `x.py` in `files` and,
    // in `aux_files`, a typed stub `x.pyi` beside it plus — once at the
    // resolution root — the shared runtime `neoc_lib.py` and its stub
    // `neoc_lib.pyi`. (Python has no native `Result`, so the prelude carries the
    // runtime, not just typings.) These ride in `aux_files`, not `files`, so the
    // `files`↔`source_maps` positional contract is unaffected. No source map yet.
    let dir = tempdir();
    let entry = write(
        &dir,
        "x.neoc",
        "pub struct Row { id: number, body: string }\n\
         \n\
         pub fn load(id: number) -> Result<Row, string> {\n\
         \x20   Ok(Row { id: id, body: \"ok\" })\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Python).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    // The module artifact is `x.py`, aligned with `source_maps`.
    assert_eq!(project.files.len(), 1, "expected x.py");
    assert_eq!(
        project.files.len(),
        project.source_maps.len(),
        "files and source_maps must stay positionally aligned"
    );
    let (path, _) = &project.files[0];
    assert!(
        path.file_name().is_some_and(|f| f == "x.py"),
        "expected x.py, got {path:?}"
    );
    // No source map yet for Python.
    assert!(
        project.source_maps[0].is_empty(),
        "Python has no source map yet"
    );

    let aux = |name: &str| {
        project
            .aux_files
            .iter()
            .find(|(p, _)| p.file_name().is_some_and(|f| f == name))
            .map_or_else(|| panic!("{name} not in aux_files"), |(_, c)| c.as_str())
    };

    // The per-module `.pyi` typed stub sits beside `x.py`.
    let x_pyi = aux("x.pyi");
    assert!(
        x_pyi.contains("class Row"),
        "x.pyi should project the `pub struct`:\n{x_pyi}"
    );

    // The shared runtime prelude pair is emitted once, at the resolution root.
    assert!(
        aux("neoc_lib.py").contains("class Ok"),
        "neoc_lib.py should carry the Result/Option runtime"
    );
    assert!(
        aux("neoc_lib.pyi").contains("class Ok"),
        "neoc_lib.pyi should declare the prelude types"
    );
    let lib_at_root = project
        .aux_files
        .iter()
        .filter(|(p, _)| p.file_name().is_some_and(|f| f == "neoc_lib.py"))
        .count();
    assert_eq!(lib_at_root, 1, "neoc_lib.py emitted exactly once");
}

/// Whether `bun` is on PATH, so the TS run trial can drive it. When absent the
/// trial skips the run and asserts only the emitted import shape.
fn bun_present() -> bool {
    Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A cross-module project that exports a `pub fn`, a `pub const`, and a
/// data-only `pub` struct. The `pub fn` and `pub const` carry a runtime binding;
/// the data-only struct (no `impl`, no `#[derive]`) emits nothing runtime, so its
/// name is erased. The importing module must `import` only the binding-bearing
/// names — emitting an `import { Point }` against a module that exports nothing
/// under that name is an unmatched named import, an ESM link error (#58). The
/// emitted modules are written to disk and run under `bun` as ESM to prove they
/// load and execute.
#[test]
fn non_fn_cross_module_export_loads_as_esm() {
    let dir = tempdir();
    write(
        &dir,
        "lib/geom.neoc",
        "pub fn add(a: number, b: number) -> number { a + b }\n\
         \n\
         pub const ORIGIN_X: number = 10;\n\
         \n\
         pub struct Point {\n    x: number,\n    y: number,\n}\n",
    );
    // `main` constructs the data-only `Point` across the module boundary, uses
    // the `pub const`, and calls the `pub fn`.
    let entry = write(
        &dir,
        "main.neoc",
        "use lib::geom::add;\n\
         use lib::geom::ORIGIN_X;\n\
         use lib::geom::Point;\n\
         \n\
         extern { fn println(n: number); }\n\
         \n\
         pub fn main() {\n\
         \x20   let p = Point { x: ORIGIN_X, y: 2 };\n\
         \x20   println(add(p.x, p.y));\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_ts = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // The `pub fn` and `pub const` carry a binding, so each lowers to an import.
    assert!(
        main_ts.contains("import { add } from \"./lib/geom\";"),
        "main should import the `pub fn`:\n{main_ts}"
    );
    assert!(
        main_ts.contains("import { ORIGIN_X } from \"./lib/geom\";"),
        "main should import the `pub const`:\n{main_ts}"
    );
    // The data-only `pub` struct emits no runtime binding, so its name is not a
    // runtime import; the construction inlines to a plain tagged object.
    assert!(
        main_ts.contains("__struct: \"Point\""),
        "the `Point` construction should inline to a tagged object:\n{main_ts}"
    );

    if !bun_present() {
        return;
    }

    // Write the emitted modules to disk under their relative paths and run the
    // entry under `bun` (which strips TS types and resolves extensionless
    // imports). A runner module supplies the `println` host global, imports
    // `main`, and calls it. Loading without a link error proves no import dangles.
    let out = tempdir();
    let src_root = std::fs::canonicalize(&dir).expect("canonicalise source root");
    for (abs, contents) in &project.files {
        let rel = abs.strip_prefix(&src_root).expect("file under source root");
        let dest = out.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("create out dir");
        }
        std::fs::write(&dest, contents).expect("write emitted module");
    }
    let runner = out.join("run.ts");
    std::fs::write(
        &runner,
        "globalThis.println = (x) => console.log(x);\n\
         import { main } from \"./main\";\n\
         main();\n",
    )
    .expect("write runner");

    let output = Command::new("bun")
        .arg(&runner)
        .output()
        .expect("spawn bun");
    assert!(
        output.status.success(),
        "bun failed to run the emitted ESM:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "12",
        "main should print add(10, 2) = 12"
    );
}

/// A method called on a `use`-imported, method-bearing type dispatches across
/// the module boundary. Dispatch is prototype-backed: the value's `__proto__`
/// link is made in the defining module (where its prototype `Thing$` lives), so
/// it carries dispatch wherever it travels (`lang/DYNAMIC.md` §3.7, §7.1). The
/// importer needs no per-type wiring — a plain `import` and a native `t.doubled()`
/// call suffice. Asserted on the emitted source, then proven under `bun` where present.
#[test]
fn method_on_use_imported_type_dispatches_across_modules() {
    let dir = tempdir();
    write(
        &dir,
        "lib.neoc",
        "pub struct Thing {\n    n: number,\n}\n\
         \n\
         impl Thing {\n\
         \x20   fn doubled(self) -> number {\n\
         \x20       self.n + self.n\n\
         \x20   }\n\
         }\n\
         \n\
         pub fn make(v: number) -> Thing {\n    Thing { n: v }\n}\n",
    );
    // `main` imports the type and its constructor, then calls a method on the
    // value returned across the boundary.
    let entry = write(
        &dir,
        "main.neoc",
        "use lib::Thing;\n\
         use lib::make;\n\
         \n\
         extern { fn println(n: number); }\n\
         \n\
         pub fn main() {\n\
         \x20   let t = make(21);\n\
         \x20   println(t.doubled());\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_ts = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // The method-bearing type's runtime binding is its prototype `Thing$`; the
    // `use` imports that. No per-type registry wiring — the value carries its
    // prototype across the boundary.
    assert!(
        main_ts.contains("import { Thing$ } from \"./lib\";"),
        "main should import the method-bearing type's prototype:\n{main_ts}"
    );
    assert!(
        !main_ts.contains("__m") && !main_ts.contains("__call"),
        "main should emit no method registry — dispatch is prototype-backed:\n{main_ts}"
    );
    // The method call is a native property call resolving up the prototype chain.
    assert!(
        main_ts.contains("t.doubled()"),
        "the method call should be a native property call:\n{main_ts}"
    );

    if !bun_present() {
        return;
    }

    let out = tempdir();
    let src_root = std::fs::canonicalize(&dir).expect("canonicalise source root");
    for (abs, contents) in &project.files {
        let rel = abs.strip_prefix(&src_root).expect("file under source root");
        let dest = out.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("create out dir");
        }
        std::fs::write(&dest, contents).expect("write emitted module");
    }
    let runner = out.join("run.ts");
    std::fs::write(
        &runner,
        "globalThis.println = (x) => console.log(x);\n\
         import { main } from \"./main\";\n\
         main();\n",
    )
    .expect("write runner");

    let output = Command::new("bun")
        .arg(&runner)
        .output()
        .expect("spawn bun");
    assert!(
        output.status.success(),
        "bun failed to run the emitted ESM (the #66 crash if unfixed):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "42",
        "main should print Thing::doubled(21) = 42"
    );
}

/// Two modules share a final segment `Hue`: `pal` declares a type-only data-only
/// `pub struct Hue` (no `impl`, no `#[derive]`, so its name is erased and its
/// module exports nothing for it), while `cfg` exports a binding `pub const Hue`.
/// The entry `use`s both under aliases. The keep/suppression decision must be
/// keyed per resolved import (`module::name`), not on the bare final segment:
/// keyed on the bare name, `cfg`'s binding `Hue` would leak its keep decision
/// onto `pal`'s type-only `Hue`, re-emitting `import { Hue } from "./pal"` that
/// dangles against an empty export — an ESM link error (#58). The entry must
/// import ONLY the binding `Hue` from `cfg`, and load + run under `bun` with no
/// link error.
#[test]
fn binding_and_type_only_name_collision_loads_as_esm() {
    let dir = tempdir();
    // `pal::Hue` — a data-only `pub` struct: no binding, name erased.
    write(&dir, "pal.neoc", "pub struct Hue {\n    r: number,\n}\n");
    // `cfg::Hue` — a `pub const` binding under the same final segment.
    write(&dir, "cfg.neoc", "pub const Hue: number = 7;\n");
    // The entry imports both under aliases: it constructs the type-only `PalHue`
    // and reads the binding `CfgHue`.
    let entry = write(
        &dir,
        "main.neoc",
        "use pal::Hue as PalHue;\n\
         use cfg::Hue as CfgHue;\n\
         \n\
         extern { fn println(n: number); }\n\
         \n\
         pub fn main() {\n\
         \x20   let h = PalHue { r: CfgHue };\n\
         \x20   println(h.r);\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_ts = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // The binding `cfg::Hue` is kept (aliased to `CfgHue`).
    assert!(
        main_ts.contains("import { Hue as CfgHue } from \"./cfg\";"),
        "main should import the binding `Hue` from cfg:\n{main_ts}"
    );
    // The type-only `pal::Hue` is suppressed: `pal` exports nothing for it.
    assert!(
        !main_ts.contains("from \"./pal\""),
        "main must NOT import the type-only `Hue` from pal:\n{main_ts}"
    );
    // Its construction inlines to a tagged object — the erased name never appears
    // as an import binding.
    assert!(
        main_ts.contains("__struct: \"PalHue\"") || main_ts.contains("__struct: \"Hue\""),
        "the `PalHue` construction should inline to a tagged object:\n{main_ts}"
    );

    if !bun_present() {
        return;
    }

    let out = tempdir();
    let src_root = std::fs::canonicalize(&dir).expect("canonicalise source root");
    for (abs, contents) in &project.files {
        let rel = abs.strip_prefix(&src_root).expect("file under source root");
        let dest = out.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("create out dir");
        }
        std::fs::write(&dest, contents).expect("write emitted module");
    }
    let runner = out.join("run.ts");
    std::fs::write(
        &runner,
        "globalThis.println = (x) => console.log(x);\n\
         import { main } from \"./main\";\n\
         main();\n",
    )
    .expect("write runner");

    let output = Command::new("bun")
        .arg(&runner)
        .output()
        .expect("spawn bun");
    assert!(
        output.status.success(),
        "bun failed to run the emitted ESM:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "7",
        "main should print CfgHue = 7"
    );
}

#[test]
fn cross_module_enum_match_and_construction_lower_correctly() {
    // An enum imported via `use` must lower the same as a local one: `match`
    // tests the variant tag, and construction builds the tagged table. The
    // emitter learns the imported enum's variants through the module graph.
    let dir = tempdir();
    write(
        &dir,
        "palette.neoc",
        "pub enum Color {\n  Red,\n  Green,\n}\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use palette::Color;\n\nfn name(c: Color) -> string {\n  match c {\n    Color::Red => \"red\",\n    Color::Green => \"green\",\n  }\n}\n\nfn main() -> string {\n  let c = Color::Green;\n  name(c)\n}\n",
    );
    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    assert!(
        main_luau.contains("tag == \"Red\"") && main_luau.contains("tag == \"Green\""),
        "match on an imported enum should test the variant tag:\n{main_luau}"
    );
    assert!(
        main_luau.contains("__tag = \"Green\""),
        "constructing an imported variant should build the tagged table:\n{main_luau}"
    );
}

#[test]
fn cross_module_associated_fn_call_keeps_the_type_qualifier() {
    // `Counter::zero()` on a `use`d type must emit `Counter.zero()`, reading the
    // imported type's table. Dropping to a bare `zero()` would reference an
    // unbound global. The `use` binding marks `Counter` as a qualified carrier.
    let dir = tempdir();
    write(
        &dir,
        "counter.neoc",
        "pub struct Counter {\n  n: number,\n}\n\nimpl Counter {\n  fn zero() -> Counter {\n    Counter { n: 0 }\n  }\n}\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use counter::Counter;\n\nfn main() -> Counter {\n  Counter::zero()\n}\n",
    );
    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    assert!(
        main_luau.contains("Counter.zero()"),
        "an imported associated call should keep the `Counter.` qualifier:\n{main_luau}"
    );
    assert!(
        !main_luau.lines().any(|l| l.trim() == "return zero()"),
        "a bare `zero()` would reference an unbound global:\n{main_luau}"
    );
}

#[test]
fn cross_module_string_plus_lowers_to_concatenation() {
    // `+` is overloaded: `string + string` is concatenation (`..`), `number +
    // number` is arithmetic (`+`). The choice is type-directed, so when an
    // operand's type resolves through a `use` import the elaborator must see the
    // imported signature — otherwise it cannot tell the `+` is on strings and
    // leaves arithmetic `+`, which is wrong Lua for a concatenation.
    let dir = tempdir();
    write(
        &dir,
        "greet.neoc",
        "pub fn hello() -> string {\n  \"hello\"\n}\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use greet::hello;\n\nfn main() -> string {\n  hello() + \" world\"\n}\n",
    );
    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    assert!(
        main_luau.contains(".."),
        "a string `+` across modules should lower to Lua `..`:\n{main_luau}"
    );
    assert!(
        !main_luau.contains("hello() +"),
        "the `+` must not stay arithmetic for a string concatenation:\n{main_luau}"
    );
}

#[test]
fn field_access_on_a_transitively_imported_type_resolves() {
    // `main` imports only `factory::make`; `make` returns `Config`, declared in
    // a third module `types` that `main` never names in a `use`. Accessing
    // `c.level` needs `Config`'s shape, which the driver pulls in transitively
    // from the imported signature — without it the checker rejects the field.
    let dir = tempdir();
    write(
        &dir,
        "types.neoc",
        "pub struct Config {\n  level: number,\n}\n",
    );
    write(
        &dir,
        "factory.neoc",
        "use types::Config;\n\npub fn make() -> Config {\n  Config { level: 1 }\n}\n",
    );
    let entry = write(
        &dir,
        "main.neoc",
        "use factory::make;\n\nfn main() -> number {\n  let c = make();\n  c.level\n}\n",
    );
    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        project.ok(),
        "field access on a transitively-imported type should resolve, got: {:#?}",
        project.diagnostics
    );

    let main_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");
    assert!(
        main_luau.contains("c.level"),
        "the field access should lower to a Lua field read:\n{main_luau}"
    );
}

#[test]
fn an_unrelated_pub_type_is_not_silently_in_scope() {
    // The transitive pull is scoped to types an import's signature reaches. A
    // `pub` type in a module nothing imports must not leak into another module's
    // scope: naming it without a `use` is still a resolution error.
    let dir = tempdir();
    write(
        &dir,
        "secret.neoc",
        "pub struct Hidden {\n  v: number,\n}\n",
    );
    write(&dir, "util.neoc", "pub fn noop() -> number {\n  0\n}\n");
    let entry = write(
        &dir,
        "main.neoc",
        "use util::noop;\n\nfn main() -> number {\n  let h = Hidden { v: 1 };\n  noop()\n}\n",
    );
    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(
        !project.ok(),
        "constructing an unimported `Hidden` should not type-check:\n{:#?}",
        project.diagnostics
    );
}

// ---- cross-file `use` of an extern item (`lang/EXTERN.md` §Lowering) --------

/// A `pub` extern struct + `#[extern(constructor)]`/method declared in one file,
/// `use`d and exercised in another. The defining file (`db.neoc`) emits no
/// runtime value for the extern; the consumer (`app.neoc`) reconstructs the
/// **native** import (`bun:sqlite`) and the construction/method lowering from the
/// imported `#[extern(...)]` metadata, never importing `Database` from
/// `./db` / `require("db")`.
const DB_EXT: &str = "extern {\n  \
    #[extern(path = \"bun:sqlite\")]\n  \
    pub mod sqlite {\n    \
    pub struct Database;\n    \
    impl Database {\n      \
    #[extern(constructor)] pub fn new(filename: string);\n      \
    pub fn run(self, sql: string);\n    \
    }\n  \
    }\n}\n";

const APP_USE_EXT: &str = "use db::Database;\n\n\
    pub fn main() {\n  \
    let d = Database::new(\":memory:\");\n  \
    d.run(\"CREATE TABLE t (id)\");\n}\n";

#[test]
fn cross_file_use_of_extern_lowers_native_import_in_ts_consumer() {
    let dir = tempdir();
    write(&dir, "db.neoc", DB_EXT);
    let entry = write(&dir, "app.neoc", APP_USE_EXT);

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(project.ok(), "expected clean: {:#?}", project.diagnostics);

    let app_ts = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "app"))
        .map(|(_, c)| c.as_str())
        .expect("app.ts emitted");

    // The consumer reconstructs the native import and the constructor form.
    assert!(
        app_ts.contains("import * as sqlite from \"bun:sqlite\";"),
        "consumer emits the native import:\n{app_ts}"
    );
    assert!(
        app_ts.contains("new sqlite.Database(\":memory:\")"),
        "constructor lowers to the target form, qualified through the mod:\n{app_ts}"
    );
    assert!(
        app_ts.contains("d.run("),
        "method call lowers native on the foreign receiver:\n{app_ts}"
    );
    // No import of `Database` from the defining file — an extern emits no runtime
    // value there.
    assert!(
        !app_ts.contains("./db"),
        "consumer must not import the extern from the defining file:\n{app_ts}"
    );
}

#[test]
fn cross_file_use_of_extern_lowers_native_require_in_luau_consumer() {
    let dir = tempdir();
    write(&dir, "db.neoc", DB_EXT);
    let entry = write(&dir, "app.neoc", APP_USE_EXT);

    let project = transpile_project(&entry, Target::Luau).expect("read project");
    assert!(project.ok(), "expected clean: {:#?}", project.diagnostics);

    let app_luau = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "app"))
        .map(|(_, c)| c.as_str())
        .expect("app.luau emitted");

    assert!(
        app_luau.contains("require(\"bun:sqlite\")"),
        "consumer emits the native require:\n{app_luau}"
    );
    assert!(
        app_luau.contains("sqlite.Database.new(\":memory:\")"),
        "constructor lowers to the target form, qualified through the mod:\n{app_luau}"
    );
    assert!(
        app_luau.contains("d:run("),
        "method call lowers to a colon call:\n{app_luau}"
    );
    assert!(
        !app_luau.contains("require(\"db\")"),
        "consumer must not require the extern from the defining file:\n{app_luau}"
    );
}

#[test]
fn cross_file_use_of_a_private_extern_is_a_visibility_error() {
    // The extern struct is *not* `pub`: a cross-file `use` of it must fail with
    // the same private-item diagnostic a plain non-`pub` item gets.
    let dir = tempdir();
    write(
        &dir,
        "db.neoc",
        "extern {\n  #[extern(path = \"bun:sqlite\")]\n  pub mod sqlite {\n    \
         struct Database;\n  }\n}\n",
    );
    let entry = write(
        &dir,
        "app.neoc",
        "use db::Database;\npub fn main() { let _ = Database; }\n",
    );

    let project = transpile_project(&entry, Target::Ts).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.stage == Stage::Type
                && d.diagnostic.message.contains("`Database` is private")),
        "a non-`pub` extern struct must report a private-item error:\n{:#?}",
        project.diagnostics
    );
}

// ---- in-memory project diagnostics (check_module_in_project, the LSP path) --

mod project_check {
    use neoc_compiler::check_module_in_project;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn project(files: &[(&str, &str)]) -> (PathBuf, HashMap<PathBuf, String>) {
        let root = PathBuf::from("/proj");
        let sources = files
            .iter()
            .map(|(name, src)| (root.join(name), (*src).to_string()))
            .collect();
        (root, sources)
    }

    fn messages(d: &[neoc_compiler::Diagnostic]) -> Vec<String> {
        d.iter().map(|x| x.message.clone()).collect()
    }

    #[test]
    fn a_non_entry_importer_resolves_its_imports() {
        // The confirmed bug: a module that `use`s a sibling must not report a
        // phantom `unresolved module` when checked in the project.
        let (root, sources) = project(&[
            (
                "util.neoc",
                "pub fn add(a: number, b: number) -> number { a + b }\n",
            ),
            (
                "main.neoc",
                "use util::add;\nfn go() -> number { add(1, 2) }\n",
            ),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("main.neoc"));
        assert!(
            diags.is_empty(),
            "imported `add` resolves: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_real_type_error_in_the_focus_is_reported() {
        let (root, sources) = project(&[
            (
                "util.neoc",
                "pub fn add(a: number, b: number) -> number { a + b }\n",
            ),
            (
                "main.neoc",
                "use util::add;\nfn go() -> string { add(1, 2) }\n",
            ),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("main.neoc"));
        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("expects `string`")),
            "the return mismatch surfaces: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_missing_imported_module_is_reported() {
        let (root, sources) = project(&[("main.neoc", "use gone::x;\nfn go() -> number { 1 }\n")]);
        let diags = check_module_in_project(&root, &sources, &root.join("main.neoc"));
        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("unresolved module")),
            "a genuinely missing module is a real error: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_focus_absent_from_sources_yields_nothing() {
        let (root, sources) = project(&[("a.neoc", "fn a() -> number { 1 }\n")]);
        let diags = check_module_in_project(&root, &sources, &root.join("missing.neoc"));
        assert!(diags.is_empty(), "{:?}", messages(&diags));
    }

    #[test]
    fn a_directly_referenced_sibling_type_without_use_is_unresolved() {
        // The editor path must match the single-file and build paths:
        // constructing a sibling module's `pub struct` with no `use` is a
        // resolution error, even though every workspace module is in the
        // universe. The universe is for *transitive* shapes, not for satisfying
        // a name the module references directly. Single-file `transpile` reports
        // `unknown struct \`Database\`` for the same source; the editor must too.
        let (root, sources) = project(&[
            ("model.neoc", "pub struct Database {\n  path: string,\n}\n"),
            (
                "main.neoc",
                "fn go() -> number {\n  let d = Database { path: \"x\" };\n  1\n}\n",
            ),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("main.neoc"));
        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("unknown struct `Database`")),
            "a directly-referenced un-`use`d `Database` must stay unresolved: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_directly_referenced_sibling_method_without_use_is_unresolved() {
        // Same invariant for an associated-function call: `Database::open()` on
        // a sibling's type the focus never `use`s must not silently resolve.
        // Single-file `transpile` reports `unresolved path \`Database::open\``;
        // the editor must match.
        let (root, sources) = project(&[
            (
                "model.neoc",
                "pub struct Database {\n  path: string,\n}\nimpl Database {\n  pub fn open() -> number { 1 }\n}\n",
            ),
            ("main.neoc", "fn go() -> number {\n  Database::open()\n}\n"),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("main.neoc"));
        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("unresolved path `Database::open`")),
            "a directly-referenced un-`use`d `Database::open` must stay unresolved: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_cross_file_use_of_a_pub_extern_resolves_in_the_editor() {
        // The editor path (`check_module_in_project`) must agree with the build:
        // a `use`d `pub` extern struct + its constructor resolve, no error.
        let (root, sources) = project(&[
            (
                "db.neoc",
                "extern {\n  #[extern(path = \"bun:sqlite\")]\n  pub mod sqlite {\n    \
                 pub struct Database;\n    impl Database {\n      \
                 #[extern(constructor)] pub fn new(filename: string);\n    }\n  }\n}\n",
            ),
            (
                "app.neoc",
                "use db::Database;\npub fn main() {\n  \
                 let d = Database::new(\":memory:\");\n  let _ = d;\n}\n",
            ),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("app.neoc"));
        assert!(
            diags.is_empty(),
            "a `use`d `pub` extern must resolve in the editor: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_cross_file_use_of_a_private_extern_is_a_visibility_error_in_the_editor() {
        let (root, sources) = project(&[
            (
                "db.neoc",
                "extern {\n  #[extern(path = \"bun:sqlite\")]\n  pub mod sqlite {\n    \
                 struct Database;\n  }\n}\n",
            ),
            (
                "app.neoc",
                "use db::Database;\npub fn main() { let _ = Database; }\n",
            ),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("app.neoc"));
        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("`Database` is private")),
            "a non-`pub` extern struct must be a private-item error in the editor: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn a_transitively_imported_shape_still_resolves_in_the_editor() {
        // The legitimate feature survives the fix: `main` imports only
        // `factory::make`, whose return type `Config` it never names in a `use`.
        // Field access on the returned value needs `Config`'s shape, pulled in
        // transitively through the imported signature — so no diagnostic.
        let (root, sources) = project(&[
            ("types.neoc", "pub struct Config {\n  level: number,\n}\n"),
            (
                "factory.neoc",
                "use types::Config;\n\npub fn make() -> Config {\n  Config { level: 1 }\n}\n",
            ),
            (
                "main.neoc",
                "use factory::make;\n\nfn go() -> number {\n  let c = make();\n  c.level\n}\n",
            ),
        ]);
        let diags = check_module_in_project(&root, &sources, &root.join("main.neoc"));
        assert!(
            diags.is_empty(),
            "transitive-shape access must keep resolving in the editor: {:?}",
            messages(&diags)
        );
    }
}
