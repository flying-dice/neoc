//! Multi-file module resolution and `STATIC.md` §2 visibility enforcement.
//!
//! A `use a::b::c;` resolves module `a::b` to a sibling `.neoc` file under the
//! entry's directory and imports its `pub` item `c`. Importing a non-`pub` item
//! or a module path that resolves to no file is a diagnostic. These tests drive
//! [`transpile_project`] against both a checked-in two-file fixture and
//! temp-directory projects built per case.
//!
//! neoc emits one target, `lua`, so these tests assert against it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use neoc_compiler::{Stage, Target, transpile, transpile_project};

mod common;

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

    // The producer must be a requireable library: `pub fn add` is a `local` and
    // the module closes with a `return { … }` export table carrying `add`, so the
    // consumer's `require("util").add` resolves (`lang/DYNAMIC.md` §3.7).
    let util_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "util"))
        .map(|(_, src)| src.as_str())
        .expect("util output present");
    assert!(
        util_lua.contains("local function add("),
        "`pub fn add` should be a local in a library build:\n{util_lua}"
    );
    assert!(
        util_lua.contains("add = add"),
        "util should export `add` in its return table:\n{util_lua}"
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

    let project = transpile_project(&entry, Target::Lua).expect("read project");
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
fn conflicting_same_name_use_is_a_name_conflict() {
    // Two `use`s binding the same final segment from different modules collide
    // on a single in-scope name (`#61`). The checker should reject the second
    // `use` so the diagnostic cites the conflicting import, not the downstream
    // codegen failure (the emitted module would bind `X` twice).
    let dir = tempdir();
    write(&dir, "a.neoc", "pub fn X(n: number) -> number { n + 1 }\n");
    write(&dir, "b.neoc", "pub fn X(n: number) -> number { n + 2 }\n");
    let entry = write(
        &dir,
        "main.neoc",
        "use a::X;\nuse b::X;\n\nfn main() -> number { X(1) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.stage == Stage::Type
                && d.diagnostic.message == "`X` is already imported"),
        "expected a name-conflict diagnostic on the second `use`, got: {:#?}",
        project.diagnostics
    );
    // All-or-nothing: a project with a diagnostic emits no files.
    assert!(project.files.is_empty());
}

#[test]
fn aliased_use_avoids_conflicting_same_name_import() {
    // The escape hatch from the name conflict above: `as` rebinds the second
    // import so it no longer collides with the first.
    let dir = tempdir();
    write(&dir, "a.neoc", "pub fn X(n: number) -> number { n + 1 }\n");
    write(&dir, "b.neoc", "pub fn X(n: number) -> number { n + 2 }\n");
    let entry = write(
        &dir,
        "main.neoc",
        "use a::X;\nuse b::X as Y;\n\nfn main() -> number { X(1) + Y(2) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "aliased same-name imports should type-check, got: {:#?}",
        project.diagnostics
    );
}

#[test]
fn type_only_and_value_use_with_same_name_coexist() {
    // The conflict guard gates *per namespace*, so a type-only import and a
    // value import that share a final segment do NOT collide — they land in
    // different namespaces. `pal::Hue` is a data-only struct (type namespace);
    // `cfg::Hue` is a `const` (value namespace). Unaliased, both bind `Hue`, but
    // the struct-literal `Hue { .. }` reads the type and `r: Hue` reads the
    // value — the #58 coexistence, without aliases. An earlier guard ORed both
    // namespaces and wrongly rejected this with `Hue is already imported` plus a
    // cascading `unknown name Hue`.
    //
    // Cross-namespace is the discriminating input: every other retained conflict
    // test pairs two bindings in the *same* namespace, so a union gate and a
    // per-namespace gate agree on all of them. Without this case the whole
    // compiler suite passes against the union-gate regression.
    //
    // The struct constructor is still a runtime table (`Hue { .. }` lowers to
    // `Hue.new`), so the type-only import is not erased: it binds a fresh
    // local and the value import keeps the plain name (#91). Run the
    // emitted modules in-process on LuaJIT: `main` must read `cfg`'s `7`.
    let (main_lua, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use pal::Hue;\n\
         use cfg::Hue;\n\
         \n\
         pub fn main() -> number {\n\
         \x20   let h = Hue { r: Hue };\n\
         \x20   h.r\n\
         }\n",
    );
    assert!(
        main_lua.contains("local __type_Hue = require(\"pal\").Hue"),
        "type-only import binds a fresh local:\n{main_lua}"
    );
    assert!(
        main_lua.contains("local Hue = require(\"cfg\").Hue"),
        "value import keeps the plain name:\n{main_lua}"
    );
    assert_eq!(
        main_lua.matches("local Hue =").count(),
        1,
        "exactly one `local Hue`:\n{main_lua}"
    );
    assert!(
        main_lua.contains("__type_Hue.new({ r = Hue })"),
        "literal constructs through the type binding, field reads the value:\n{main_lua}"
    );
    assert_eq!(result, "7");
}

/// `pal`: a data-only struct `Hue` (type namespace only) with an associated fn.
const PAL_HUE: &str = "pub struct Hue {\n    r: number,\n}\n\
                       impl Hue {\n    fn make(r: number) -> Hue { Hue { r: r } }\n}\n";
/// `cfg`: a value `Hue`.
const CFG_HUE: &str = "pub const Hue: number = 7;\n";

/// Build a temp project of `deps` (dotted module path → source) plus
/// `main.neoc`, assert it type-checks, then run `main.main()` in-process on
/// `LuaJIT` under a `require` shim over the emitted chunks. Returns `main`'s Lua
/// and the printed result.
fn run_project(deps: &[(&str, &str)], main: &str) -> (String, String) {
    let dir = tempdir();
    for (module, src) in deps {
        write(&dir, &format!("{}.neoc", module.replace('.', "/")), src);
    }
    let entry = write(&dir, "main.neoc", main);

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "project should type-check, got: {:#?}",
        project.diagnostics
    );
    let lua_of = |module: &str| {
        let rel = PathBuf::from(format!("{}.lua", module.replace('.', "/")));
        project
            .files
            .iter()
            .find(|(p, _)| p.ends_with(&rel))
            .map_or_else(|| panic!("{module} output present"), |(_, src)| src.clone())
    };
    let chunks = deps
        .iter()
        .map(|(m, _)| *m)
        .chain(["main"])
        .fold(String::new(), |mut out, m| {
            let _ = writeln!(out, "[\"{m}\"] = function(...)\n{}\nend,", lua_of(m));
            out
        });
    let program = format!(
        "local __chunks = {{\n{chunks}}}\n\
         local __loaded = {{}}\n\
         function require(name)\n\
           if __loaded[name] == nil then __loaded[name] = __chunks[name]() end\n\
           return __loaded[name]\n\
         end\n\
         print(require(\"main\").main())\n"
    );
    let stdout = common::run_lua(&program)
        .unwrap_or_else(|e| panic!("lua failed:\n{e}\n--- program ---\n{program}"));
    (lua_of("main"), stdout.trim().to_string())
}

// `lang/DYNAMIC.md` §3.7: a type-only import shadowed by any value the module
// binds — value `use`, module-level `const` or `fn` — keeps its constructor
// reachable through a fresh local. Each case runs on LuaJIT.

#[test]
fn type_only_use_shadowed_by_module_const_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE)],
        "use pal::Hue;\nconst Hue: number = 7;\n\
         pub fn main() -> number { let h = Hue { r: Hue }; h.r }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn type_only_use_shadowed_by_module_fn_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE)],
        "use pal::Hue;\nfn Hue() -> number { 7 }\n\
         pub fn main() -> number { let h = Hue { r: Hue() }; h.r }\n",
    );
    assert_eq!(result, "7");
}

// `lang/DYNAMIC.md` §3.7: an imported struct is captured as readily by a local
// `let`, a parameter or a pattern binding as by a module-level value.

#[test]
fn type_only_use_shadowed_by_let_runs() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_HUE)],
        "use pal::Hue;\n\
         pub fn main() -> number { let Hue = 7; let h = Hue { r: Hue }; h.r }\n",
    );
    assert!(
        main_lua.contains("local __type_Hue = require(\"pal\").Hue"),
        "{main_lua}"
    );
    assert_eq!(result, "7");
}

#[test]
fn type_only_use_shadowed_by_parameter_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE)],
        "use pal::Hue;\nfn f(Hue: number) -> number { let h = Hue { r: Hue }; h.r }\n\
         pub fn main() -> number { f(7) }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn type_only_use_shadowed_by_closure_and_for_and_match_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE)],
        "use pal::Hue;\n\
         pub fn main() -> number {\n\
             let c = |Hue: number| { let h = Hue { r: Hue }; h.r };\n\
             let t = 0;\n\
             for Hue in 1..2 { t = t + Hue { r: Hue }.r; }\n\
             let m = match Some(5) { Some(Hue) => Hue { r: Hue }.r, None => 0 };\n\
             c(7) + t + m\n\
         }\n",
    );
    assert_eq!(result, "13");
}

#[test]
fn type_only_use_not_shadowed_keeps_its_name() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_HUE)],
        "use pal::Hue;\npub fn main() -> number { let h = Hue { r: 4 }; h.r }\n",
    );
    assert!(!main_lua.contains("__type_"), "{main_lua}");
    assert_eq!(result, "4");
}

#[test]
fn shadowed_type_local_is_not_captured_by_a_user_binding() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use pal::Hue;\nuse cfg::Hue;\n\
         pub fn main() -> number { let __type_Hue = 3; let h = Hue { r: Hue }; h.r + __type_Hue }\n",
    );
    assert!(
        main_lua.contains("local __type_Hue_1 = require(\"pal\").Hue"),
        "{main_lua}"
    );
    assert_eq!(result, "10");
}

/// `pal`: two type-only structs, `Hue` and `Hue_1`, and a fn that checks its
/// argument is a `Hue`.
const PAL_HUE_PAIR: &str = "pub struct Hue {\n    r: number,\n}\n\
                            pub struct Hue_1 {\n    r: number,\n}\n\
                            pub fn get(h: Hue) -> number { h.r }\n";
/// `cfg`: values `Hue` and `Hue_1`.
const CFG_HUE_PAIR: &str = "pub const Hue: number = 7;\npub const Hue_1: number = 100;\n";
/// `main` header shadowing both `pal` types and spelling `__type_Hue`, so
/// `Hue`'s fresh local steps to the name `Hue_1` would take first.
const USE_HUE_PAIR: &str = "use pal::Hue;\nuse pal::Hue_1;\nuse pal::get;\n\
                            use cfg::Hue;\nuse cfg::Hue_1;\n";

#[test]
fn two_shadowed_types_get_distinct_fresh_locals() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_HUE_PAIR), ("cfg", CFG_HUE_PAIR)],
        &format!(
            "{USE_HUE_PAIR}pub fn main() -> number {{ let __type_Hue = 3; let h = Hue {{ r: Hue }}; \
             let k = Hue_1 {{ r: Hue_1 }}; get(h) + k.r + __type_Hue }}\n"
        ),
    );
    assert_eq!(
        main_lua.matches("local __type_Hue_1 =").count(),
        1,
        "{main_lua}"
    );
    assert_eq!(result, "110");
}

#[test]
fn two_shadowed_types_match_their_own_struct_pattern() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE_PAIR), ("cfg", CFG_HUE_PAIR)],
        &format!(
            "{USE_HUE_PAIR}pub fn main() -> number {{ let __type_Hue = 3; let h = Hue {{ r: Hue }}; \
             let k = Hue_1 {{ r: Hue_1 }}; let a = match h {{ Hue {{ r }} => r }}; \
             let b = match k {{ Hue_1 {{ r }} => r }}; a + b + __type_Hue }}\n"
        ),
    );
    assert_eq!(result, "110");
}

#[test]
fn shadowed_type_import_struct_update_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use pal::Hue;\nuse cfg::Hue;\n\
         pub fn main() -> number { let h = Hue { r: 1 }; let g = Hue { r: Hue, ..h }; g.r }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn shadowed_type_import_associated_fn_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use pal::Hue;\nuse cfg::Hue;\npub fn main() -> number { Hue::make(Hue).r }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn shadowed_type_import_under_alias_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use pal::Hue as X;\nuse cfg::Hue as X;\n\
         pub fn main() -> number { let h = X { r: X }; h.r }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn shadowed_type_import_after_value_import_runs() {
    let (_, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use cfg::Hue;\nuse pal::Hue;\n\
         pub fn main() -> number { let h = Hue { r: Hue }; h.r }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn shadowed_type_import_from_nested_module_runs() {
    let (_, result) = run_project(
        &[("a.pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use a::pal::Hue;\nuse cfg::Hue;\n\
         pub fn main() -> number { let h = Hue { r: Hue }; h.r }\n",
    );
    assert_eq!(result, "7");
}

#[test]
fn unshadowed_alias_of_a_shadowed_type_keeps_its_name() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_HUE), ("cfg", CFG_HUE)],
        "use pal::Hue;\nuse pal::Hue as Q;\nuse cfg::Hue;\n\
         pub fn main() -> number { let h = Q { r: Hue }; h.r + 1 }\n",
    );
    assert!(
        main_lua.contains("local Q = require(\"pal\").Hue"),
        "{main_lua}"
    );
    assert!(!main_lua.contains("__type_Q"), "{main_lua}");
    assert_eq!(result, "8");
}

#[test]
fn conflicting_same_name_type_use_is_a_name_conflict() {
    // The type-namespace analogue of the value conflict above: two `use`s
    // binding the same struct name from different modules collide in the type
    // namespace. On `main` this was silent last-wins (the second `T` shadowed
    // the first, surfacing later as a spurious field error); the guard now
    // cites the conflicting `use`.
    let dir = tempdir();
    write(&dir, "a.neoc", "pub struct T {\n    n: number,\n}\n");
    write(&dir, "b.neoc", "pub struct T {\n    m: number,\n}\n");
    let entry = write(
        &dir,
        "main.neoc",
        "use a::T;\nuse b::T;\n\nfn main() -> number { let t = T { n: 1 }; t.n }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.stage == Stage::Type
                && d.diagnostic.message == "`T` is already imported"),
        "expected a name-conflict diagnostic on the second `use`, got: {:#?}",
        project.diagnostics
    );
    assert!(project.files.is_empty());
}

#[test]
fn distinct_aliases_to_the_same_name_conflict() {
    // Aliases are not a blanket escape: the *binding* is what must be unique, so
    // two different imports aliased to the same name still collide. This pins
    // the nuance that `as` sidesteps a conflict only when it rebinds to a
    // distinct name (cf. `aliased_use_avoids_conflicting_same_name_import`).
    let dir = tempdir();
    write(&dir, "a.neoc", "pub fn X(n: number) -> number { n + 1 }\n");
    write(&dir, "b.neoc", "pub fn Y(n: number) -> number { n + 2 }\n");
    let entry = write(
        &dir,
        "main.neoc",
        "use a::X as Z;\nuse b::Y as Z;\n\nfn main() -> number { Z(1) }\n",
    );

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.stage == Stage::Type
                && d.diagnostic.message == "`Z` is already imported"),
        "expected a name-conflict diagnostic on the colliding alias, got: {:#?}",
        project.diagnostics
    );
    assert!(project.files.is_empty());
}

#[test]
fn single_file_project_emits_a_requireable_library() {
    // A project build emits every module as a requireable library, so the entry
    // closes with a `return { … }` export table even with nothing `pub` (here,
    // empty) — the one divergence from single-file `transpile`, which stays a
    // directly-runnable script with no trailing return (`lang/DYNAMIC.md` §3.7).
    // Past that line, the body is the single-file `transpile` output. Maps ride
    // in-band on `source_maps`, so there is no `.map` sidecar or trailer.
    let dir = tempdir();
    let src = "fn main() -> number { 1 + 1 }\n";
    let entry = write(&dir, "solo.neoc", src);

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(project.ok());
    assert_eq!(project.files.len(), 1);
    let emitted = &project.files[0].1;
    // The script body matches single-file `transpile`; the library form adds the
    // export table (no `pub` items here, so an empty one).
    let expected = format!("{}return {{}}\n", transpile(src, Target::Lua).output);
    assert_eq!(emitted, &expected);
}

#[test]
fn lua_build_emits_module_files() {
    // A project emits a `.lua` *per module* in `files` (the artifact,
    // positionally aligned with `source_maps`). Each module is self-contained: it
    // inlines the runtime, so a build writes no sidecar of any kind. The source
    // map is real (#56): the emitter records it during emit.
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

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(
        project.ok(),
        "expected a clean project, got: {:#?}",
        project.diagnostics
    );

    // The `.lua` module is the artifact, aligned with `source_maps`.
    assert_eq!(project.files.len(), 1, "expected x.lua");
    assert_eq!(
        project.files.len(),
        project.source_maps.len(),
        "files and source_maps must stay positionally aligned"
    );
    // The emitter records a real map (#56), so `neoc run` can rewrite a runtime fault
    // to its neoc position.
    assert!(
        !project.source_maps[0].is_empty(),
        "the emitter records a source map"
    );
    let (path, x_lua) = &project.files[0];
    assert!(
        path.file_name().is_some_and(|f| f == "x.lua"),
        "expected x.lua, got {path:?}"
    );

    // x.lua is untyped Lua: the `pub fn` header carries parameter names only.
    assert!(
        x_lua.contains("function load(id)"),
        "x.lua should carry the plain `pub fn` header:\n{x_lua}"
    );
    assert!(
        !x_lua.contains("type Row"),
        "x.lua must not carry type declarations:\n{x_lua}"
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
        main_lua.contains("__tag = \"Green\""),
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
fn cross_file_use_of_extern_lowers_native_require_in_lua_consumer() {
    let dir = tempdir();
    write(&dir, "db.neoc", DB_EXT);
    let entry = write(&dir, "app.neoc", APP_USE_EXT);

    let project = transpile_project(&entry, Target::Lua).expect("read project");
    assert!(project.ok(), "expected clean: {:#?}", project.diagnostics);

    let app_lua = project
        .files
        .iter()
        .find(|(p, _)| p.file_stem().is_some_and(|s| s == "app"))
        .map(|(_, c)| c.as_str())
        .expect("app.lua emitted");

    assert!(
        app_lua.contains("require(\"bun:sqlite\")"),
        "consumer emits the native require:\n{app_lua}"
    );
    assert!(
        app_lua.contains("sqlite.Database.new(\":memory:\")"),
        "constructor lowers to the target form, qualified through the mod:\n{app_lua}"
    );
    assert!(
        app_lua.contains("d:run("),
        "method call lowers to a colon call:\n{app_lua}"
    );
    assert!(
        !app_lua.contains("require(\"db\")"),
        "consumer must not require the extern from the defining file:\n{app_lua}"
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

    let project = transpile_project(&entry, Target::Lua).expect("read project");
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

// `lang/DYNAMIC.md` §3.2: an imported method-bearing enum's variants are built
// with the defining module's metatable at every construction form, so `:method`
// dispatch resolves in the consumer. Each case runs on LuaJIT.

/// `pal`: a method-bearing enum with a unit, a tuple and a struct variant.
const PAL_SHAPE: &str = "pub enum Shape {\n    Dot,\n    Sq(number),\n    Rect { w: number },\n}\n\
                         impl Shape {\n    fn id(self) -> number { 1 }\n}\n";

#[test]
fn imported_method_enum_unit_variant_has_metatable() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_SHAPE)],
        "use pal::Shape;\npub fn main() -> number { let c = Shape::Dot; c.id() }\n",
    );
    assert!(
        main_lua.contains("setmetatable({ __enum = \"Shape\", __tag = \"Dot\" }, Shape)"),
        "{main_lua}"
    );
    assert_eq!(result, "1");
}

#[test]
fn imported_method_enum_tuple_variant_has_metatable() {
    let (_, result) = run_project(
        &[("pal", PAL_SHAPE)],
        "use pal::Shape;\npub fn main() -> number { Shape::Sq(2).id() }\n",
    );
    assert_eq!(result, "1");
}

#[test]
fn imported_method_enum_struct_variant_has_metatable() {
    let (_, result) = run_project(
        &[("pal", PAL_SHAPE)],
        "use pal::Shape;\npub fn main() -> number { Shape::Rect { w: 2 }.id() }\n",
    );
    assert_eq!(result, "1");
}

#[test]
fn imported_derive_only_enum_has_metatable() {
    let (main_lua, result) = run_project(
        &[(
            "pal",
            "#[derive(Eq)]\npub enum Mode {\n    On,\n    Off,\n}\n",
        )],
        "use pal::Mode;\npub fn main() -> number { if Mode::On == Mode::On { 1 } else { 0 } }\n",
    );
    assert!(main_lua.contains(", Mode)"), "{main_lua}");
    assert_eq!(result, "1");
}

#[test]
fn imported_plain_enum_stays_a_bare_table() {
    let (main_lua, result) = run_project(
        &[("pal", "pub enum Bit {\n    On,\n    Off,\n}\n")],
        "use pal::Bit;\npub fn main() -> number { match Bit::On { Bit::On => 1, Bit::Off => 0 } }\n",
    );
    assert!(!main_lua.contains("setmetatable"), "{main_lua}");
    assert_eq!(result, "1");
}

#[test]
fn imported_method_enum_shadowed_by_value_reads_type_local() {
    let (main_lua, result) = run_project(
        &[("pal", PAL_SHAPE)],
        "use pal::Shape;\nconst Shape: number = 7;\n\
         pub fn main() -> number { Shape::Dot.id() + Shape }\n",
    );
    assert!(
        main_lua.contains("local __type_Shape = require(\"pal\").Shape"),
        "{main_lua}"
    );
    assert!(main_lua.contains(", __type_Shape)"), "{main_lua}");
    assert_eq!(result, "8");
}

#[test]
fn local_plain_enum_sharing_a_name_with_an_unimported_method_enum_stays_bare() {
    let (main_lua, result) = run_project(
        &[(
            "pal",
            "pub enum Shape {\n    Dot,\n}\nimpl Shape {\n    fn id(self) -> number { 1 }\n}\n\
             pub enum Bit {\n    On,\n    Off,\n}\nimpl Bit {\n    fn id(self) -> number { 2 }\n}\n",
        )],
        "use pal::Shape;\nenum Bit {\n    On,\n    Off,\n}\n\
         pub fn main() -> number { match Bit::On { Bit::On => Shape::Dot.id(), Bit::Off => 0 } }\n",
    );
    assert!(
        !main_lua.contains("setmetatable({ __enum = \"Bit\""),
        "{main_lua}"
    );
    assert_eq!(result, "1");
}
