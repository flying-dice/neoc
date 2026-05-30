//! Executable Luau-backend spec for `neoc-codegen-luau`.
//!
//! The Gherkin features in `tooling/codegen-luau/` are the behavioural contract
//! for the typed-Luau emitter: a `.neoc` module is parsed (`neoc-parser`) and
//! lowered to a self-typed `.luau` (`neoc_codegen_luau::emit`), and the scenarios
//! assert on the emitted source. A typed `.luau` is the Lua runtime with native
//! inline Luau types woven on: a `pub fn` keeps its parameter/return types, a
//! `struct` projects to a `type` over a tagged table, an `enum` to a union
//! `type`, and `Result`/`Option` to inline type aliases. The step definitions
//! drive the real `emit` entry point, so the features *are* the spec — mirroring
//! `neoc-codegen-lua`'s harness and the crate's own goldens.
//!
//! Scenarios tagged `@pending` describe specified-but-unverified behaviour; the
//! runner filters them out so the suite stays green.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the source under test, the parse
/// diagnostics (a syntax error is held by the parser, never the infallible
/// emitter), and the emitted Luau with its emit diagnostics.
#[derive(Debug, Default, World)]
struct Codegen {
    source: String,
    parse_diagnostics: Vec<String>,
    luau: String,
    emit_diagnostics: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Codegen, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I emit Luau")]
fn emit_luau(world: &mut Codegen) {
    let parsed = neoc_parser::parse(&world.source);
    world.parse_diagnostics = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    // A single-file emit resolves no cross-module graph: no imported enums or
    // extern items — exactly how `transpile` calls `emit` for a lone source.
    let result = neoc_codegen_luau::emit(&parsed.module, &world.source, &[], &[]);
    world.luau = result.luau;
    world.emit_diagnostics = result.diagnostics.into_iter().map(|d| d.message).collect();
}

#[then(regex = r#"^the Luau contains "(.+)"$"#)]
fn luau_contains(world: &mut Codegen, snippet: String) {
    assert!(
        world.luau.contains(&snippet),
        "expected the emitted Luau to contain {snippet:?}\n  emitted Luau:\n{}",
        world.luau,
    );
}

#[then(regex = r#"^the Luau does not contain "(.+)"$"#)]
fn luau_does_not_contain(world: &mut Codegen, snippet: String) {
    assert!(
        !world.luau.contains(&snippet),
        "expected the emitted Luau to omit {snippet:?}\n  emitted Luau:\n{}",
        world.luau,
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
    // Features live at the repo top (`tooling/codegen-luau/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("codegen-luau");
    futures::executor::block_on(Codegen::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
