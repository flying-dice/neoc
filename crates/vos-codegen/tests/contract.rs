//! Contract tests: drift between the Rust schema and any generated consumer is a build failure.
//!
//! * the committed generated files are exactly what the generator writes now;
//! * every component, expression function and client action is covered by the Luau declarations, the runtime
//!   `ui.lua` and the TypeScript output; the app definitions offer exactly the modules the host does (no more, no
//!   refused one) and carry one `declare require`;
//! * the Calculator and Notes examples (typed Luau, checked by `bun run lint:luau`), built as the host runs them,
//!   are **run** against the real generated `ui.lua` with a `lua` interpreter (5.1 or newer), and the trees they
//!   render validate against the schema. Without `lua` installed the committed trees are validated instead.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use vos_domain::{GLOBALS_NOT_FOR_APPS, NOT_FOR_APPS};

use serde_json::Value;
use vos_domain::ui::{
    components::{lookup, CATALOGUE, STACK_SUGAR},
    expr::Func,
    node::{derive_test_id, Action, Node},
    protocol::METHODS,
    source::{ClientDecl, SourceDecl},
    validate::{is_valid, validate_with, Context, Severity},
    ViewDecls,
};

fn root() -> &'static Path {
    vos_codegen::repo_root()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The app definitions the generator writes now, from neoc's committed declarations.
fn definitions() -> String {
    let neoc = fs::read_to_string(root().join(vos_codegen::NEOC_DECLARATIONS)).unwrap();
    vos_codegen::luau_definitions(&neoc).unwrap()
}

// ---- the generated files ----------------------------------------------------------------------------------

#[test]
fn the_committed_generated_files_are_up_to_date() {
    let stale = vos_codegen::stale(root());
    assert!(stale.is_empty(), "run `bun run codegen`: stale {stale:?}");
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(vos_codegen::generate(root()), vos_codegen::generate(root()));
}

/// A colon is not allowed in a Windows file name: no generated file may have one (the module `vos:app` is
/// declared inside one file, not as a file of its own).
#[test]
fn no_generated_file_has_a_colon_in_its_name() {
    for file in vos_codegen::generate(root()).unwrap() {
        assert!(!file.path.contains(':'), "{}", file.path);
    }
}

// ---- Lua output -------------------------------------------------------------------------------------------

#[test]
fn ui_lua_has_a_constructor_for_every_component_and_sugar() {
    let lua = vos_codegen::ui_module();
    let defs = definitions();
    for c in CATALOGUE {
        assert!(lua.contains(&format!("function ui.{}(props)", c.name)), "{}", c.name);
        assert!(defs.contains(&format!("export type vos_{}Props = {{\n", c.name)), "{}", c.name);
        assert!(
            defs.contains(&format!("\t{}: (props: vos_{}Props) -> vos_Node,", c.name, c.name)),
            "{}",
            c.name
        );
    }
    for (sugar, _) in STACK_SUGAR {
        assert!(lua.contains(&format!("function ui.{sugar}(props)")), "{sugar}");
        assert!(defs.contains(&format!("\t{sugar}: (props: vos_StackProps) -> vos_Node,")), "{sugar}");
    }
}

#[test]
fn ui_lua_has_a_helper_for_every_expression_function() {
    let lua = vos_codegen::ui_module();
    for f in Func::ALL {
        let wire = format!("call(\"{}\"", f.name());
        assert!(lua.contains(&wire), "no ui helper calls `{}`", f.name());
    }
    for keyword_safe in ["ui.all", "ui.any", "ui.negate", "ui.when", "ui.concat", "ui.isEmpty", "ui.coalesce"]
    {
        assert!(lua.contains(&format!("function {keyword_safe}(")), "{keyword_safe}");
    }
    assert!(lua.contains("function fmt.relative("));
}

#[test]
fn ui_act_covers_exactly_the_domain_actions() {
    let wire: Vec<&str> = vos_codegen::ACTION_BUILDERS.iter().map(|(_, wire)| *wire).collect();
    assert_eq!(wire, Action::NAMES);
    let lua = vos_codegen::ui_module();
    let defs = definitions();
    for (builder, wire) in vos_codegen::ACTION_BUILDERS {
        assert!(lua.contains(&format!("function act.{builder}(")), "act.{builder}");
        assert!(defs.contains(&format!("\t{builder}: (")), "act.{builder} definition");
        assert!(lua.contains(&format!("[\"do\"] = \"{wire}\"")), "{wire}");
    }
}

/// The runtime module is plain Lua source (the host runs it as Luau; the contract tests below also run it with a native
/// `lua`): the types of its constructors live in the declarations.
#[test]
fn ui_lua_is_plain_lua() {
    let lua = vos_codegen::ui_module();
    assert!(
        !lua.contains("---@"),
        "the runtime module carries no annotations: they live in the declarations"
    );
    let code: String =
        lua.lines().filter(|line| !line.trim_start().starts_with("--")).collect::<Vec<_>>().join("\n");
    for luau_only in ["export type", ":: ", " += ", " ..= ", "continue", "`"] {
        assert!(!code.contains(luau_only), "Luau-only syntax `{luau_only}` in ui.lua");
    }
}

#[test]
fn the_definitions_declare_the_app_global_and_the_client_decl() {
    let defs = definitions();
    assert!(defs.starts_with("-- GENERATED by vos-codegen"));
    assert!(defs.contains("export type vos_AppApi = {"));
    assert!(defs.contains("\ndeclare app: vos_AppApi\n"), "the global `app`");
    assert!(
        defs.contains("export type vos_ClientDecl = {"),
        "ClientDecl comes from the JSON Schema of the Rust type"
    );
    assert!(defs.contains("export type vos_Persist = \"none\" | \"session\" | \"app\""));
    for ty in ["vos_Ref<T>", "vos_Expr<T>", "vos_Handler<P>", "vos_Child", "vos_Action", "vos_Node"] {
        assert!(defs.contains(&format!("export type {ty} = ")), "{ty} is defined");
    }
}

/// Two `declare require` do not merge: the file has one, and it is the last thing in it.
#[test]
fn the_definitions_have_one_declare_require_with_every_module() {
    let defs = definitions();
    assert_eq!(defs.lines().filter(|l| l.starts_with("declare require")).count(), 1);
    let overloads = vos_codegen::require_overloads(&defs);
    let names: Vec<&str> = overloads.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"vos:app") && names.contains(&"vos:ui"), "{names:?}");
    assert!(defs.trim_end().ends_with("& ((path: string) -> any)"), "a bundled module of the app is any");
    // `bun run lint:luau` cuts the last section out for its second pass: it must be the last marker of the file
    let last = defs.lines().rfind(|l| l.starts_with("-- ===== ")).unwrap();
    assert_eq!(last, "-- ===== require =====");
    // typed: the app API and the constructors are the types the file declares
    assert!(overloads.contains(&("vos:app".to_owned(), "vos_AppApi".to_owned())));
    assert!(overloads.contains(&("vos:ui".to_owned(), "vos_ui".to_owned())));
}

#[test]
fn the_modules_an_app_is_not_offered_are_unusable_and_the_others_are_neocs() {
    let defs = definitions();
    let overloads = vos_codegen::require_overloads(&defs);
    for refused in NOT_FOR_APPS {
        let (_, ty) =
            overloads.iter().find(|(name, _)| name == refused).unwrap_or_else(|| panic!("{refused}"));
        assert!(ty.starts_with("vos_NotForApps_"), "{refused} is typed unusable, not as neoc's module: {ty}");
        let section = format!("-- ===== {refused} =====");
        assert!(!defs.contains(&section), "neoc's section of {refused} is left out");
        assert!(defs.contains(&format!("export type {ty} = {{ __not_for_apps: never }}")), "{ty}");
    }
    // every other overload is neoc's own type for the module (`lib:json` -> `lib_json`)
    let neoc = fs::read_to_string(root().join(vos_codegen::NEOC_DECLARATIONS)).unwrap();
    let theirs = vos_codegen::require_overloads(&neoc);
    assert!(theirs.len() > NOT_FOR_APPS.len() + 20);
    for (name, ty) in &theirs {
        if NOT_FOR_APPS.contains(&name.as_str()) {
            continue;
        }
        assert!(overloads.contains(&(name.clone(), ty.clone())), "{name} keeps neoc's type {ty}");
        assert!(defs.contains(&format!("-- ===== {name} =====\n")), "{name} keeps its section");
    }
    assert_eq!(overloads.len(), theirs.len() + 2, "neoc's modules, `vos:app` and `vos:ui`");
}

#[test]
fn the_globals_an_app_does_not_have_are_declared_unusable() {
    let defs = definitions();
    for global in GLOBALS_NOT_FOR_APPS {
        assert!(defs.contains(&format!("\ndeclare {global}: vos_NotForApps_{global}\n")), "{global}");
    }
    // a function global is unusable by any call (`print("x")`, `pcall(print, "x")`), a library by any member access
    assert!(defs.contains("export type vos_NotForApps_print = (__not_for_apps: never) -> never\n"));
    assert!(defs.contains("export type vos_NotForApps_loadstring = (__not_for_apps: never) -> never\n"));
    assert!(defs.contains("export type vos_NotForApps_os = { __not_for_apps: never }\n"));
    // and none of them is a global the app is meant to use
    for needed in ["app", "typeof", "utf8", "table", "string", "math", "bit32", "coroutine"] {
        assert!(!GLOBALS_NOT_FOR_APPS.contains(&needed), "{needed}");
    }
}

// ---- TypeScript output ------------------------------------------------------------------------------------

#[test]
fn typescript_declares_every_component_protocol_type_and_method() {
    let ts = vos_codegen::typescript();
    for c in CATALOGUE {
        assert!(ts.contains(&format!("export type {}Props", c.name)), "{}", c.name);
        assert!(ts.contains(&format!("export type {}Node = {{", c.name)), "{}", c.name);
        assert!(ts.contains(&format!("  {}: {{ tier: {}", c.name, c.tier as u8)), "{} in COMPONENTS", c.name);
    }
    for ty in [
        "Node",
        "Handler",
        "Action",
        "Op",
        "SourceDecl",
        "ClientDecl",
        "SourcePatch",
        "ViewSnapshot",
        "ViewOps",
        "ViewTree",
        "DataPatch",
        "ViewEvent",
        "WriteAck",
        "SessionHello",
        "Command",
    ] {
        assert!(ts.contains(&format!("export type {ty} =")), "{ty}");
    }
    for m in METHODS {
        assert!(ts.contains(&format!("'{}': {{ kind:", m.name)), "{}", m.name);
    }
    for f in Func::ALL {
        assert!(ts.contains(&format!("'{}': {{ min:", f.name())), "{}", f.name());
    }
}

#[test]
fn the_reference_documents_every_component() {
    let docs = fs::read_to_string(root().join("docs/reference/components.md")).unwrap();
    for c in CATALOGUE {
        assert!(docs.contains(&format!("### {}\n", c.name)), "{}", c.name);
    }
    assert!(docs.contains("Accessible name required"));
}

// ---- the example apps -------------------------------------------------------------------------------------

/// What the stub host sends for `view.open`, minus the snapshot envelope.
struct Rendered {
    tree: Node,
    decls: ViewDecls,
}

fn parse(json: &Value) -> Rendered {
    let tree: Node = serde_json::from_value(json["tree"].clone()).unwrap_or_else(|e| panic!("tree: {e}"));
    let mut decls = ViewDecls::default();
    for (name, decl) in json["client"].as_object().unwrap() {
        let decl: ClientDecl =
            serde_json::from_value(decl.clone()).unwrap_or_else(|e| panic!("client {name}: {e}"));
        decls.client.insert(name.clone(), decl);
    }
    for (id, decl) in json["sources"].as_object().unwrap() {
        let decl: SourceDecl =
            serde_json::from_value(decl.clone()).unwrap_or_else(|e| panic!("source {id}: {e}"));
        decls.sources.insert(id.clone(), decl);
    }
    Rendered { tree, decls }
}

fn committed(app: &str) -> Value {
    let path = fixtures().join("trees").join(format!("{app}.json"));
    serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .unwrap()
}

/// A Lua interpreter (5.1 or newer) on PATH, or `None`.
fn lua_binary() -> Option<&'static str> {
    ["lua5.1", "lua", "lua5.4", "lua5.3", "luajit"]
        .into_iter()
        .find(|name| Command::new(name).arg("-v").output().is_ok_and(|out| out.status.success()))
}

/// Where an example app's Luau lives: the real app under `apps/<app>` when the repo has one (the Calculator), so the
/// contract is checked against what ships, else a fixture (Notes, which exists only as an example).
fn source_of(app: &str) -> std::path::PathBuf {
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps").join(app);
    if shipped.join("app.json").exists() {
        shipped
    } else {
        fixtures().join(app)
    }
}

/// The generated `ui.lua`, written where the interpreter can read it.
fn write_ui(work: &Path) -> PathBuf {
    let ui = work.join("ui.lua");
    fs::write(&ui, vos_codegen::ui_module()).unwrap();
    ui
}

/// The app as the host runs it: the one file the product's own pipeline builds (`scripts/luau-build.ts`: darklua,
/// with the types stripped). Needs bun and darklua, as the other crates' tests do.
fn build(app: &str, work: &Path) -> PathBuf {
    let built = work.join("app.luau");
    let run = Command::new("bun")
        .arg("scripts/luau-build.ts")
        .arg(source_of(app).join("app.luau"))
        .arg(&built)
        .current_dir(root())
        .output()
        .expect("bun is needed to build the apps these tests run (https://bun.sh)");
    assert!(
        run.status.success(),
        "building {app} failed (is darklua 0.18.0 installed? see scripts/luau.ts):\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    built
}

/// Run an example app's `main` window, built as the host runs it, with a `lua` interpreter against the generated
/// `ui.lua`. `None` when no interpreter is installed.
fn run_lua(app: &str) -> Option<Value> {
    let lua = lua_binary()?;
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lua-{app}"));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let ui = write_ui(&work);
    let built = build(app, &work);
    let output = Command::new(lua)
        .current_dir(&work)
        .arg(fixtures().join("stub/harness.lua"))
        .arg(&ui)
        .arg(fixtures().join("stub/app.lua"))
        .arg(&built)
        .output()
        .ok()?;
    assert!(
        output.status.success(),
        "{lua} failed for {app}:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some(
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{app}: {e}\n{}", String::from_utf8_lossy(&output.stdout))),
    )
}

fn check_tree(app: &str, rendered: &Rendered) {
    let context = Context::from_decls(&rendered.decls);
    let findings = validate_with(&rendered.tree, &context);
    let errors: Vec<String> =
        findings.iter().filter(|v| v.severity == Severity::Error).map(ToString::to_string).collect();
    assert!(errors.is_empty(), "{app} renders an invalid tree:\n{}", errors.join("\n"));
    assert!(is_valid(&findings));
    assert!(rendered.tree.count() > 5, "{app} should render a real tree");
}

fn ids(app: &str, node: &Node, path: &str, out: &mut Vec<String>) {
    if let Some(id) = derive_test_id(app, path, None) {
        out.push(id);
    }
    for (slot, children) in node.containers() {
        for child in children {
            let child_path = vos_domain::ui::node::join_path(path, slot, &child.key);
            ids(app, child, &child_path, out);
        }
    }
}

#[test]
fn the_examples_run_against_the_generated_constructors_and_render_valid_trees() {
    let bless = std::env::var_os("UPDATE_FIXTURES").is_some();
    let mut ran = 0;
    for app in ["calculator", "notes"] {
        if let Some(fresh) = run_lua(app) {
            ran += 1;
            check_tree(app, &parse(&fresh));
            let path = fixtures().join("trees").join(format!("{app}.json"));
            if bless {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, serde_json::to_string_pretty(&fresh).unwrap() + "\n").unwrap();
            } else {
                assert_eq!(
                    fresh,
                    committed(app),
                    "{app}: the tree the Lua example renders changed; review it, then run \
                     `UPDATE_FIXTURES=1 cargo test -p vos-codegen` to re-bless tests/fixtures/trees/{app}.json"
                );
            }
        }
    }
    if ran == 0 {
        eprintln!(
            "skipped running the examples: no `lua` interpreter is installed (the committed trees are validated below)"
        );
    }
}

#[test]
fn the_constructors_behave_as_documented() {
    let Some(lua) = lua_binary() else {
        eprintln!("skipped: no `lua` interpreter is installed");
        return;
    };
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lua-constructors");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let ui = write_ui(&work);
    let output = Command::new(lua)
        .current_dir(&work)
        .arg(fixtures().join("stub/constructors.lua"))
        .arg(&ui)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.trim() == "ok",
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_committed_example_trees_validate() {
    for app in ["calculator", "notes"] {
        check_tree(app, &parse(&committed(app)));
    }
}

#[test]
fn the_calculator_tree_has_the_documented_shape_and_ids() {
    let rendered = parse(&committed("calculator"));
    let mut found = Vec::new();
    ids("calculator", &rendered.tree, &rendered.tree.key, &mut found);
    for expected in [
        "calculator-key-7",
        "calculator-key-clear",
        "calculator-display",
        "calculator-caption",
        "calculator-backspace",
    ] {
        assert!(found.iter().any(|id| id == expected), "{expected} in {found:?}");
    }
    // the keys themselves (`calculator-key-7`), not ids inside one (the wide 0 key's `calculator-key-0-label`)
    let keys = found
        .iter()
        .filter(|id| id.strip_prefix("calculator-key-").is_some_and(|k| !k.contains('-')))
        .count();
    assert_eq!(keys, 19, "the keypad");
    // the keys are grid children with a column span, the display is a live region
    // (found by key, not position: the layout around them is the app's to change)
    let grid = find(&rendered.tree, "Grid").expect("a keypad grid");
    assert_eq!(grid.children.len(), 19);
    let zero = grid.children.iter().find(|k| k.key == "key-0").unwrap();
    assert_eq!(zero.props["span"], 2);
    let display = find_key(&rendered.tree, "display").expect("a display");
    assert_eq!(display.props["live"], "polite");
    assert!(lookup(&display.type_).is_some());
}

#[test]
fn the_notes_tree_has_the_documented_sources_bindings_and_ids() {
    let rendered = parse(&committed("notes"));
    // three sources, declared once; the SQL never reaches the tree
    assert_eq!(rendered.decls.sources.len(), 3);
    assert!(
        matches!(rendered.decls.sources["s1"], SourceDecl::Row { .. }),
        "`note` is declared first in render"
    );
    assert!(matches!(rendered.decls.sources["s3"], SourceDecl::Rows { page: Some(60), .. }));
    assert!(serde_json::to_string(&rendered.tree).unwrap().find("select ").is_none(), "no SQL in the tree");
    // persisted client state
    assert_eq!(rendered.decls.client["folder"].persist, vos_domain::ui::source::Persist::App);
    // the two-way text binding, and the ids the spec lists (§4.5)
    let text = serde_json::to_string(&rendered.tree).unwrap();
    assert!(text.contains(r#""text":{"$ref":"src.s1.body"}"#), "TextEditor is bound to the row source field");
    let mut found = Vec::new();
    ids("notes", &rendered.tree, &rendered.tree.key, &mut found);
    for expected in
        ["notes-folders", "notes-notes", "notes-body", "notes-confirm-delete-confirm", "notes-new-note"]
    {
        assert!(found.iter().any(|id| id == expected), "{expected} in {found:?}");
    }
}

/// The first node of this type, depth first.
fn find<'a>(node: &'a vos_domain::ui::node::Node, type_: &str) -> Option<&'a vos_domain::ui::node::Node> {
    if node.type_ == type_ {
        return Some(node);
    }
    node.containers().flat_map(|(_, children)| children.iter()).find_map(|c| find(c, type_))
}

/// The first node with this key, depth first.
fn find_key<'a>(node: &'a vos_domain::ui::node::Node, key: &str) -> Option<&'a vos_domain::ui::node::Node> {
    if node.key == key {
        return Some(node);
    }
    node.containers().flat_map(|(_, children)| children.iter()).find_map(|c| find_key(c, key))
}
