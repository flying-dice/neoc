//! Executable TypeScript-backend spec for `neoc-codegen-ts`.
//!
//! The Gherkin features in `tooling/codegen-ts/` are the behavioural contract
//! for the typed-TypeScript emitter: a `.neoc` module is parsed (`neoc-parser`)
//! and lowered to a self-typed `.ts` (`neoc_codegen_ts::emit`), and the
//! scenarios assert on the emitted source. A typed `.ts` is the JS runtime with
//! TypeScript types woven on: `pub fn` keeps its parameter/return types,
//! `pub struct` projects to an `interface`, `pub enum` to a discriminated-union
//! `type`, and a `use` to a typed, extensionless sibling import. The step
//! definitions drive the real `emit` entry point, so the features *are* the
//! spec — mirroring `neoc-codegen-js`'s harness and the crate's own goldens.
//!
//! Scenarios tagged `@pending` describe specified-but-unverified behaviour; the
//! runner filters them out so the suite stays green.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashSet;
use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the source under test, the parse
/// diagnostics (a syntax error is held by the parser, never the infallible
/// emitter), and the emitted TypeScript with its emit diagnostics.
#[derive(Debug, Default, World)]
struct Codegen {
    source: String,
    parse_diagnostics: Vec<String>,
    ts: String,
    emit_diagnostics: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Codegen, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I emit TypeScript")]
fn emit_typescript(world: &mut Codegen) {
    let parsed = neoc_parser::parse(&world.source);
    world.parse_diagnostics = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    // A single-file emit resolves no cross-module graph: the import/binding sets
    // are empty, the importer directory is the root, and there are no imported
    // extern items or enums — exactly how `transpile` calls `emit` for a lone
    // source.
    let result = neoc_codegen_ts::emit(
        &parsed.module,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &[],
        &[],
    );
    world.ts = result.ts;
    world.emit_diagnostics = result.diagnostics.into_iter().map(|d| d.message).collect();
}

#[then(regex = r#"^the TS contains "(.+)"$"#)]
fn ts_contains(world: &mut Codegen, snippet: String) {
    assert!(
        world.ts.contains(&snippet),
        "expected the emitted TS to contain {snippet:?}\n  emitted TS:\n{}",
        world.ts,
    );
}

#[then(regex = r#"^the TS does not contain "(.+)"$"#)]
fn ts_does_not_contain(world: &mut Codegen, snippet: String) {
    assert!(
        !world.ts.contains(&snippet),
        "expected the emitted TS to omit {snippet:?}\n  emitted TS:\n{}",
        world.ts,
    );
}

#[then("the emit reports no problems")]
fn emit_reports_no_problems(world: &mut Codegen) {
    assert!(
        world.emit_diagnostics.is_empty(),
        "expected a clean emit, got diagnostics: {:?}",
        world.emit_diagnostics,
    );
}

#[then("the source is rejected")]
fn source_is_rejected(world: &mut Codegen) {
    // A syntax error is caught at the parse stage — the emitter is infallible.
    assert!(
        !world.parse_diagnostics.is_empty(),
        "expected the source to be rejected as a syntax error, but it parsed cleanly",
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
    // Features live at the repo top (`tooling/codegen-ts/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("codegen-ts");
    futures::executor::block_on(Codegen::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
