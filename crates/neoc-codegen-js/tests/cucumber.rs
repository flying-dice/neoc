//! Executable JavaScript-backend spec for `neoc-codegen-js`.
//!
//! The Gherkin features in `tooling/codegen-js/` are the behavioural contract
//! for the ES2022 emitter: a `.neoc` module is parsed (`neoc-parser`) and lowered
//! to a JavaScript ES module (`neoc_codegen_js::emit`), and the scenarios assert
//! on the emitted source. `pub` items become `export`s, a `use` becomes a
//! relative `.js` `import`, and neoc's expression-oriented constructs lower to
//! the documented JS shapes (`lang/DYNAMIC.md`). The step definitions drive the
//! real `emit` entry point, so the features *are* the spec — mirroring the
//! crate's own `#[cfg(test)]` goldens and `lang/CONFORMANCE/`.
//!
//! Scenarios tagged `@pending` describe specified-but-unverified behaviour; the
//! runner filters them out so the suite stays green. Drop the tag once the
//! behaviour is verified.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashSet;
use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the source under test, the parse
/// diagnostics (a syntax error is held by the parser, never the infallible
/// emitter), and the emitted JavaScript with its emit diagnostics.
#[derive(Debug, Default, World)]
struct Codegen {
    source: String,
    parse_diagnostics: Vec<String>,
    js: String,
    emit_diagnostics: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Codegen, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I emit JavaScript")]
fn emit_javascript(world: &mut Codegen) {
    let parsed = neoc_parser::parse(&world.source);
    world.parse_diagnostics = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    // A single-file emit resolves no cross-module graph: the import/binding sets
    // are empty, the importer directory is the root, and there are no imported
    // extern items or enums. This is exactly how `transpile` calls `emit` for a
    // lone source (`neoc-compiler`'s `emit` seam).
    let result = neoc_codegen_js::emit(
        &parsed.module,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &[],
        &[],
    );
    world.js = result.js;
    world.emit_diagnostics = result.diagnostics.into_iter().map(|d| d.message).collect();
}

#[then(regex = r#"^the JS contains "(.+)"$"#)]
fn js_contains(world: &mut Codegen, snippet: String) {
    assert!(
        world.js.contains(&snippet),
        "expected the emitted JS to contain {snippet:?}\n  emitted JS:\n{}",
        world.js,
    );
}

#[then(regex = r#"^the JS does not contain "(.+)"$"#)]
fn js_does_not_contain(world: &mut Codegen, snippet: String) {
    assert!(
        !world.js.contains(&snippet),
        "expected the emitted JS to omit {snippet:?}\n  emitted JS:\n{}",
        world.js,
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

#[then(regex = r#"^the emit reports "(.+)"$"#)]
fn emit_reports(world: &mut Codegen, substring: String) {
    assert!(
        world
            .emit_diagnostics
            .iter()
            .any(|d| d.contains(&substring)),
        "no emit diagnostic contains {substring:?}; got: {:?}",
        world.emit_diagnostics,
    );
}

#[then("the source is rejected")]
fn source_is_rejected(world: &mut Codegen) {
    // A syntax error is caught at the parse stage — the emitter is infallible and
    // would only see a degraded tree. Asserting on parse diagnostics keeps the
    // refusal at the stage that owns it.
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
    // Features live at the repo top (`tooling/codegen-js/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("codegen-js");
    futures::executor::block_on(Codegen::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
