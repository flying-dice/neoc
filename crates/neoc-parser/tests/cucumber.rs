//! Executable parser spec for `neoc-parser`.
//!
//! The Gherkin features in `tooling/parser/` are the behavioural contract for
//! the CST → AST adapter: well-formed neoc source parses cleanly (no
//! diagnostics), and malformed source is reported with a diagnostic rather than
//! silently accepted. The step definitions drive `neoc_parser::parse` directly,
//! so the features *are* the spec — mirroring `crates/neoc-fmt/tests/cucumber.rs`
//! and `lang/CONFORMANCE/`.
//!
//! Scenarios tagged `@pending` describe behaviour that is specified but not yet
//! caught by the parser (tree-sitter recovers and accepts some malformed
//! source); the runner filters them out so the suite stays green. Drop the tag
//! once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use neoc_parser::parse;

/// State threaded through one scenario: the source under test and, once parsed,
/// whether the parse was clean plus every diagnostic's neoc-flavoured message.
#[derive(Debug, Default, World)]
struct Parser {
    source: String,
    parsed: bool,
    ok: bool,
    messages: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Parser, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[given("an empty source")]
fn given_empty_source(world: &mut Parser) {
    world.source.clear();
}

#[when("I parse it")]
fn parse_it(world: &mut Parser) {
    let result = parse(&world.source);
    world.ok = result.ok();
    world.messages = result
        .diagnostics
        .iter()
        .map(|diag| diag.message.clone())
        .collect();
    world.parsed = true;
}

#[then("it parses cleanly")]
fn parses_cleanly(world: &mut Parser) {
    assert!(world.parsed, "the source was never parsed");
    assert!(
        world.ok,
        "expected a clean parse, got diagnostics: {:?}\nsource:\n{}",
        world.messages, world.source,
    );
}

#[then("parsing reports an error")]
fn reports_an_error(world: &mut Parser) {
    assert!(world.parsed, "the source was never parsed");
    assert!(
        !world.ok && !world.messages.is_empty(),
        "expected parsing to report an error, but the parse was clean\nsource:\n{}",
        world.source,
    );
}

#[then(regex = r#"^a parse error mentions "([^"]*)"$"#)]
fn error_mentions(world: &mut Parser, substring: String) {
    assert!(world.parsed, "the source was never parsed");
    assert!(
        world.messages.iter().any(|msg| msg.contains(&substring)),
        "no parse error mentions {substring:?}; got {:?}",
        world.messages,
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
    // Features live at the repo top (`tooling/parser/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("parser");
    futures::executor::block_on(Parser::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
