//! Executable structural spec for `neoc-ast`.
//!
//! The Gherkin features in `tooling/ast/` are the behavioural contract for the
//! AST's query functions over `extern` / `ext` blocks — the seam every backend
//! reads to wire a target's surface (`lang/EXTERN.md`). The step definitions
//! drive the real query functions (`extern_imports`, `extern_struct_names`)
//! against a `Module` built by `neoc_parser::parse`, so the features *are* the
//! spec — mirroring `crates/neoc-fmt/tests/cucumber.rs` and
//! `lang/CONFORMANCE/`.
//!
//! Scenarios tagged `@pending` describe behaviour that is specified but not yet
//! implemented (or whose outcome the harness cannot verify against today's
//! output). The runner filters them out so the suite stays green; drop the tag
//! once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashSet;
use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use neoc_ast::{Ident, Module, extern_imports, extern_struct_names};

/// State threaded through one scenario: the parsed module under test and the
/// outcome of the last query — the extern-mod imports, the extern struct names,
/// and whether the source parsed cleanly.
#[derive(Debug, Default, World)]
struct Ast {
    module: Module,
    parsed_clean: bool,
    imports: Vec<(String, String, bool)>,
    struct_names: HashSet<Ident>,
}

#[given("the source:")]
fn given_source(world: &mut Ast, step: &Step) {
    let source = docstring(step).trim();
    let result = neoc_parser::parse(source);
    world.parsed_clean = result.ok();
    world.module = result.module;
}

#[when("I collect extern imports")]
fn collect_extern_imports(world: &mut Ast) {
    world.imports = extern_imports(&world.module)
        .into_iter()
        .map(|import| (import.local_name, import.path, import.ambient))
        .collect();
}

#[when("I collect extern struct names")]
fn collect_extern_struct_names(world: &mut Ast) {
    world.struct_names = extern_struct_names(&world.module);
}

#[then(regex = r#"^the imports include "([^"]+)"$"#)]
fn imports_include(world: &mut Ast, local_name: String) {
    assert!(
        world.imports.iter().any(|(name, _, _)| name == &local_name),
        "expected an import bound as {local_name:?}; got {:?}",
        world.imports,
    );
}

#[then(regex = r#"^the import "([^"]+)" has path "([^"]+)"$"#)]
fn import_has_path(world: &mut Ast, local_name: String, path: String) {
    let import = world
        .imports
        .iter()
        .find(|(name, _, _)| name == &local_name)
        .unwrap_or_else(|| panic!("no import bound as {local_name:?}; got {:?}", world.imports));
    assert_eq!(
        import.1, path,
        "import {local_name:?} has the wrong target path",
    );
}

#[then(regex = r#"^the import "([^"]+)" is ambient$"#)]
fn import_is_ambient(world: &mut Ast, local_name: String) {
    let import = world
        .imports
        .iter()
        .find(|(name, _, _)| name == &local_name)
        .unwrap_or_else(|| panic!("no import bound as {local_name:?}; got {:?}", world.imports));
    assert!(import.2, "expected import {local_name:?} to be ambient");
}

#[then(regex = r#"^the imports exclude "([^"]+)"$"#)]
fn imports_exclude(world: &mut Ast, local_name: String) {
    assert!(
        !world.imports.iter().any(|(name, _, _)| name == &local_name),
        "expected no import bound as {local_name:?}; got {:?}",
        world.imports,
    );
}

#[then("there are no imports")]
fn no_imports(world: &mut Ast) {
    assert!(
        world.imports.is_empty(),
        "expected no extern imports, got {:?}",
        world.imports,
    );
}

#[then(regex = r#"^the struct names include "([^"]+)"$"#)]
fn struct_names_include(world: &mut Ast, name: String) {
    assert!(
        world.struct_names.contains(&name),
        "expected extern struct {name:?}; got {:?}",
        world.struct_names,
    );
}

#[then(regex = r#"^the struct names exclude "([^"]+)"$"#)]
fn struct_names_exclude(world: &mut Ast, name: String) {
    assert!(
        !world.struct_names.contains(&name),
        "expected no extern struct {name:?}; got {:?}",
        world.struct_names,
    );
}

#[then("the source parsed cleanly")]
fn parsed_cleanly(world: &mut Ast) {
    assert!(
        world.parsed_clean,
        "expected the source to parse without diagnostics",
    );
}

#[then("the source did not parse cleanly")]
fn parsed_with_errors(world: &mut Ast) {
    assert!(
        !world.parsed_clean,
        "expected the source to carry parse diagnostics",
    );
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

fn main() {
    // Features live at the repo top (`tooling/ast/`), resolved from this crate's
    // manifest directory — a test's working directory is the crate, not the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("ast");
    futures::executor::block_on(Ast::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
