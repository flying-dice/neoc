//! Contract tests: drift between the Rust schema and any generated consumer is a build failure.
//!
//! * the committed generated files are exactly what the generator writes now;
//! * every component, expression function and client action is covered by the Lua and TypeScript output;
//! * the Calculator and Notes examples from `ui-system.md` §4.3 and §4.4 (checked by `bun run lint:lua`) are
//!   **run** against the real generated `ui.lua` with a `lua` interpreter (5.1 or newer), and the trees they
//!   render validate against the schema. Without `lua` installed the committed trees are validated instead.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use agentos_domain::ui::{
    components::{lookup, CATALOGUE, STACK_SUGAR},
    expr::Func,
    node::{derive_test_id, Action, Node},
    protocol::METHODS,
    source::{ClientDecl, SourceDecl},
    validate::{is_valid, validate_with, Context, Severity},
    ViewDecls,
};
use serde_json::Value;

fn root() -> &'static Path {
    agentos_codegen::repo_root()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ---- the generated files ----------------------------------------------------------------------------------

#[test]
fn the_committed_generated_files_are_up_to_date() {
    let stale = agentos_codegen::stale(root());
    assert!(stale.is_empty(), "run `bun run codegen`: stale {stale:?}");
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(agentos_codegen::generate(), agentos_codegen::generate());
}

// ---- Lua output -------------------------------------------------------------------------------------------

#[test]
fn ui_lua_has_a_constructor_for_every_component_and_sugar() {
    let lua = agentos_codegen::ui_module();
    let defs = agentos_codegen::ui_module_definitions();
    for c in CATALOGUE {
        assert!(lua.contains(&format!("function ui.{}(props)", c.name)), "{}", c.name);
        assert!(defs.contains(&format!("---@class {}Props\n", c.name)), "{}", c.name);
        assert!(defs.contains(&format!("function ui.{}(props) end", c.name)), "{}", c.name);
    }
    for (sugar, _) in STACK_SUGAR {
        assert!(lua.contains(&format!("function ui.{sugar}(props)")), "{sugar}");
        assert!(defs.contains(&format!("function ui.{sugar}(props) end")), "{sugar}");
    }
}

#[test]
fn ui_lua_has_a_helper_for_every_expression_function() {
    let lua = agentos_codegen::ui_module();
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
    let wire: Vec<&str> = agentos_codegen::ACTION_BUILDERS.iter().map(|(_, wire)| *wire).collect();
    assert_eq!(wire, Action::NAMES);
    let lua = agentos_codegen::ui_module();
    let defs = agentos_codegen::ui_module_definitions();
    for (builder, wire) in agentos_codegen::ACTION_BUILDERS {
        assert!(lua.contains(&format!("function act.{builder}(")), "act.{builder}");
        assert!(defs.contains(&format!("function act.{builder}(")), "act.{builder} definition");
        assert!(lua.contains(&format!("[\"do\"] = \"{wire}\"")), "{wire}");
    }
}

#[test]
fn ui_lua_is_plain_lua_5_1() {
    let lua = agentos_codegen::ui_module();
    assert!(!lua.contains("---@"), "the runtime module carries no annotations: they live in the definitions");
    let code: String =
        lua.lines().filter(|line| !line.trim_start().starts_with("--")).collect::<Vec<_>>().join("\n");
    for luau_only in ["export type", ":: ", " += ", " ..= ", "continue", "`"] {
        assert!(!code.contains(luau_only), "Luau-only syntax `{luau_only}` in ui.lua");
    }
}

#[test]
fn the_definitions_declare_the_app_global_and_the_client_decl() {
    let defs = agentos_codegen::agentos_definitions();
    assert!(defs.starts_with("---@meta\n"));
    assert!(defs.contains("---@class AppApi"));
    assert!(defs.contains("\napp = nil\n"), "the global `app`");
    assert!(
        defs.contains("---@alias ClientDecl {"),
        "ClientDecl comes from the JSON Schema of the Rust type"
    );
    assert!(defs.contains("---@alias Persist \"none\"|\"session\"|\"app\""));
    assert_eq!(agentos_codegen::app_module_definitions().lines().last(), Some("return app"));
}

#[test]
fn the_module_definitions_use_the_shared_core_types() {
    let core = agentos_codegen::agentos_definitions();
    let module = agentos_codegen::ui_module_definitions();
    assert!(module.starts_with("---@meta\n"));
    for ty in ["Ref<T>", "Expr<T>", "Handler<P>", "Child", "Action", "Node"] {
        assert!(core.contains(&format!("{ty} ")) || core.contains(&format!("{ty}\n")), "{ty} is defined");
    }
    for used in ["Expr<", "Handler<", "Child"] {
        assert!(module.contains(used), "{used} is used by the props");
    }
}

// ---- TypeScript output ------------------------------------------------------------------------------------

#[test]
fn typescript_declares_every_component_protocol_type_and_method() {
    let ts = agentos_codegen::typescript();
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

/// Where an example app's Lua lives: the real app under `apps/<app>` when the repo has one (the Calculator), so the
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
    fs::write(&ui, agentos_codegen::ui_module()).unwrap();
    ui
}

/// Run an example app's `main` window with a `lua` interpreter against the generated `ui.lua`. `None` when no
/// interpreter is installed.
fn run_lua(app: &str) -> Option<Value> {
    let lua = lua_binary()?;
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lua-{app}"));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let ui = write_ui(&work);
    let output = Command::new(lua)
        .current_dir(&work)
        .arg(fixtures().join("stub/harness.lua"))
        .arg(&ui)
        .arg(fixtures().join("stub/app.lua"))
        .arg(source_of(app))
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
            let child_path = agentos_domain::ui::node::join_path(path, slot, &child.key);
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
                     `UPDATE_FIXTURES=1 cargo test -p agentos-codegen` to re-bless tests/fixtures/trees/{app}.json"
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
    assert_eq!(rendered.decls.client["folder"].persist, agentos_domain::ui::source::Persist::App);
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
fn find<'a>(
    node: &'a agentos_domain::ui::node::Node,
    type_: &str,
) -> Option<&'a agentos_domain::ui::node::Node> {
    if node.type_ == type_ {
        return Some(node);
    }
    node.containers().flat_map(|(_, children)| children.iter()).find_map(|c| find(c, type_))
}

/// The first node with this key, depth first.
fn find_key<'a>(
    node: &'a agentos_domain::ui::node::Node,
    key: &str,
) -> Option<&'a agentos_domain::ui::node::Node> {
    if node.key == key {
        return Some(node);
    }
    node.containers().flat_map(|(_, children)| children.iter()).find_map(|c| find_key(c, key))
}
