//! Executable grammar spec for `neoc-grammar`.
//!
//! The Gherkin features in `tooling/grammar/` are the behavioural contract for
//! the tree-sitter parser: valid neoc source parses cleanly to the expected
//! syntax tree, and malformed source recovers into error nodes rather than
//! aborting the parse. The step definitions drive the real FFI binding
//! (`neoc_grammar::LANGUAGE` loaded into a `tree_sitter::Parser`), mirroring the
//! crate's own `#[cfg(test)] mod tests`, so the features *are* the spec — like
//! `crates/neoc-lsp/tests/cucumber.rs` and `lang/CONFORMANCE/`.
//!
//! The grammar is the authority on syntax shape; `lang/SYNTAX.md` is canonical
//! prose for the same. Scenarios tagged `@pending` describe specified-but-
//! unimplemented behaviour; the runner filters them out so the suite stays
//! green. Drop the tag once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use tree_sitter::{Parser, Tree};

/// State threaded through one scenario: the source under test and the parse tree
/// produced by the last `When I parse with the grammar` step.
#[derive(Debug, Default, World)]
struct Grammar {
    source: String,
    tree: Option<Tree>,
}

#[given("the source:")]
fn given_source(world: &mut Grammar, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[given("an empty source")]
fn given_empty_source(world: &mut Grammar) {
    world.source.clear();
}

#[when("I parse with the grammar")]
fn parse_with_grammar(world: &mut Grammar) {
    let mut parser = Parser::new();
    parser
        .set_language(&neoc_grammar::LANGUAGE.into())
        .expect("load neoc grammar");
    world.tree = Some(parser.parse(&world.source, None).expect("parse"));
}

#[then("there are no error nodes")]
fn no_error_nodes(world: &mut Grammar) {
    let root = root(world);
    assert!(
        !root.has_error(),
        "expected a clean parse of:\n{}\n got:\n{}",
        world.source,
        root.to_sexp(),
    );
}

#[then("there are error nodes")]
fn there_are_error_nodes(world: &mut Grammar) {
    let root = root(world);
    assert!(
        root.has_error(),
        "expected the malformed source to recover into error nodes, but the parse was clean:\n{}",
        root.to_sexp(),
    );
}

#[then(regex = r#"^the root node kind is "([^"]+)"$"#)]
fn root_node_kind_is(world: &mut Grammar, expected: String) {
    let kind = root(world).kind();
    assert_eq!(
        kind, expected,
        "root node kind mismatch: expected {expected:?}, got {kind:?}",
    );
}

#[then("the syntax tree is:")]
fn syntax_tree_is(world: &mut Grammar, step: &Step) {
    let expected = docstring(step).trim();
    let actual = root(world).to_sexp();
    assert_eq!(
        actual, expected,
        "syntax tree mismatch\n  expected:\n{expected}\n  actual:\n{actual}",
    );
}

/// The root node of the last parse, panicking if no parse has run.
fn root(world: &Grammar) -> tree_sitter::Node<'_> {
    world
        .tree
        .as_ref()
        .unwrap_or_else(|| {
            panic!("no parse tree — a `When I parse with the grammar` step must run first")
        })
        .root_node()
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

fn main() {
    // Features live at the repo top (`tooling/grammar/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("grammar");
    futures::executor::block_on(Grammar::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
