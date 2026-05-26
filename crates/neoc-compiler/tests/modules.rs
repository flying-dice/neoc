//! Multi-file module resolution and `STATIC.md` §2 visibility enforcement.
//!
//! A `use a::b::c;` resolves module `a::b` to a sibling `.neoc` file under the
//! entry's directory and imports its `pub` item `c`. Importing a non-`pub` item
//! or a module path that resolves to no file is a diagnostic. These tests drive
//! [`transpile_project`] against both a checked-in two-file fixture and
//! temp-directory projects built per case.

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

/// Whether `node` is on PATH, so the ESM run trial can drive it. When absent
/// the trial skips the run and asserts only the emitted import shape.
fn node_present() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A cross-module project that exports a `pub fn`, a `pub const`, and a
/// data-only `pub` struct. The `pub fn` and `pub const` carry a runtime binding;
/// the data-only struct (no `impl`, no `#[derive]`) emits nothing in JS, so its
/// name is erased. The importing module must `import` only the binding-bearing
/// names — emitting an `import { Point }` against a module that exports nothing
/// under that name is an unmatched named import, an ESM `SyntaxError` at link
/// (#58). The emitted modules are written to disk and run under `node` as ESM to
/// prove they load and execute.
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
         ext fn println(n: number);\n\
         \n\
         pub fn main() {\n\
         \x20   let p = Point { x: ORIGIN_X, y: 2 };\n\
         \x20   println(add(p.x, p.y));\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Js).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_js = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // The `pub fn` and `pub const` carry a binding, so each lowers to an import.
    assert!(
        main_js.contains("import { add } from \"./lib/geom.js\";"),
        "main should import the `pub fn`:\n{main_js}"
    );
    assert!(
        main_js.contains("import { ORIGIN_X } from \"./lib/geom.js\";"),
        "main should import the `pub const`:\n{main_js}"
    );
    // The data-only `pub` struct emits no binding, so no import — its name is
    // erased and the construction inlines to a plain tagged object.
    assert!(
        !main_js.contains("Point } from") && !main_js.contains("{ Point }"),
        "main must NOT import the data-only struct `Point`:\n{main_js}"
    );
    assert!(
        main_js.contains("_struct: \"Point\""),
        "the `Point` construction should inline to a tagged object:\n{main_js}"
    );

    if !node_present() {
        return;
    }

    // Write the emitted modules to disk under their relative paths and run the
    // entry as ESM. A runner module supplies the `println` host global, imports
    // `main`, and calls it. node loading without a `SyntaxError` proves no
    // import dangles against a missing export.
    let out = tempdir();
    // Project file paths are canonicalised against the entry's directory, so
    // strip against the canonical source root to recover each module's relative
    // layout (`main.js`, `lib/geom.js`).
    let src_root = std::fs::canonicalize(&dir).expect("canonicalise source root");
    for (abs, contents) in &project.files {
        let rel = abs.strip_prefix(&src_root).expect("file under source root");
        let dest = out.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("create out dir");
        }
        std::fs::write(&dest, contents).expect("write emitted module");
    }
    let runner = out.join("run.mjs");
    std::fs::write(
        &runner,
        "globalThis.println = (x) => console.log(x);\n\
         import { main } from \"./main.js\";\n\
         main();\n",
    )
    .expect("write runner");

    let output = Command::new("node")
        .arg(&runner)
        .output()
        .expect("spawn node");
    assert!(
        output.status.success(),
        "node failed to run the emitted ESM:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "12",
        "main should print add(10, 2) = 12"
    );
}

/// Two modules share a final segment `Hue`: `pal` declares a type-only data-only
/// `pub struct Hue` (no `impl`, no `#[derive]`, so JS erases its name and its
/// module exports nothing for it), while `cfg` exports a binding `pub const Hue`.
/// The entry `use`s both under aliases. The keep/suppression decision must be
/// keyed per resolved import (`module::name`), not on the bare final segment:
/// keyed on the bare name, `cfg`'s binding `Hue` would leak its keep decision
/// onto `pal`'s type-only `Hue`, re-emitting `import { Hue } from "./pal.js"`
/// that dangles against an empty export — an ESM `SyntaxError` at link (#58).
/// The entry must import ONLY the binding `Hue` from `cfg`, and load + run under
/// `node` as ESM with no `SyntaxError`.
#[test]
fn binding_and_type_only_name_collision_loads_as_esm() {
    let dir = tempdir();
    // `pal::Hue` — a data-only `pub` struct: no binding, name erased in JS.
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
         ext fn println(n: number);\n\
         \n\
         pub fn main() {\n\
         \x20   let h = PalHue { r: CfgHue };\n\
         \x20   println(h.r);\n\
         }\n",
    );

    let project = transpile_project(&entry, Target::Js).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_js = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    // The binding `cfg::Hue` is kept (aliased to `CfgHue`).
    assert!(
        main_js.contains("import { Hue as CfgHue } from \"./cfg.js\";"),
        "main should import the binding `Hue` from cfg:\n{main_js}"
    );
    // The type-only `pal::Hue` is suppressed: `pal.js` exports nothing for it.
    assert!(
        !main_js.contains("from \"./pal.js\""),
        "main must NOT import the type-only `Hue` from pal:\n{main_js}"
    );
    // Its construction inlines to a tagged object — the erased name never appears
    // as an import binding.
    assert!(
        main_js.contains("_struct: \"PalHue\"") || main_js.contains("_struct: \"Hue\""),
        "the `PalHue` construction should inline to a tagged object:\n{main_js}"
    );

    if !node_present() {
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
    let runner = out.join("run.mjs");
    std::fs::write(
        &runner,
        "globalThis.println = (x) => console.log(x);\n\
         import { main } from \"./main.js\";\n\
         main();\n",
    )
    .expect("write runner");

    let output = Command::new("node")
        .arg(&runner)
        .output()
        .expect("spawn node");
    assert!(
        output.status.success(),
        "node failed to run the emitted ESM:\nstdout: {}\nstderr: {}",
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
    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    assert!(
        main_lua.contains("tag == \"Red\"") && main_lua.contains("tag == \"Green\""),
        "match on an imported enum should test the variant tag:\n{main_lua}"
    );
    assert!(
        main_lua.contains("tag = \"Green\""),
        "constructing an imported variant should build the tagged table:\n{main_lua}"
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
    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    assert!(
        main_lua.contains("Counter.zero()"),
        "an imported associated call should keep the `Counter.` qualifier:\n{main_lua}"
    );
    assert!(
        !main_lua.lines().any(|l| l.trim() == "return zero()"),
        "a bare `zero()` would reference an unbound global:\n{main_lua}"
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
    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    let main_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");

    assert!(
        main_lua.contains(".."),
        "a string `+` across modules should lower to Lua `..`:\n{main_lua}"
    );
    assert!(
        !main_lua.contains("hello() +"),
        "the `+` must not stay arithmetic for a string concatenation:\n{main_lua}"
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
    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "field access on a transitively-imported type should resolve, got: {:#?}",
        project.diagnostics
    );

    let main_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "main"))
        .map(|(_, src)| src.as_str())
        .expect("main output present");
    assert!(
        main_lua.contains("c.level"),
        "the field access should lower to a Lua field read:\n{main_lua}"
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
    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        !project.ok(),
        "constructing an unimported `Hidden` should not type-check:\n{:#?}",
        project.diagnostics
    );
}
