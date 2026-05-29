//! Executable formatter spec for `neoc-fmt`.
//!
//! The Gherkin features in `tooling/fmt/` are the behavioural contract for the
//! source formatter: messy-but-valid source is rewritten to neoc's canonical
//! form, already-canonical source is left unchanged (formatting is idempotent),
//! and a source with a syntax error is refused rather than mangled. The step
//! definitions drive `neoc_fmt::format` directly, so the features *are* the
//! spec — mirroring `crates/neoc-lsp/tests/cucumber.rs` and `lang/CONFORMANCE/`.
//!
//! Scenarios tagged `@pending` describe specified-but-unimplemented behaviour;
//! the runner filters them out so the suite stays green. Drop the tag once the
//! behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use neoc_fmt::format;

/// State threaded through one scenario: the source under test and the outcome of
/// the last format — either the rewritten text or the error's neoc-flavoured
/// message.
#[derive(Debug, Default, World)]
struct Fmt {
    source: String,
    formatted: Option<String>,
    error: Option<String>,
}

#[given("the source:")]
fn given_source(world: &mut Fmt, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I format it")]
fn format_it(world: &mut Fmt) {
    match format(&world.source) {
        Ok(text) => {
            world.formatted = Some(text);
            world.error = None;
        }
        Err(err) => {
            world.error = Some(err.to_string());
            world.formatted = None;
        }
    }
}

#[then("it becomes:")]
fn it_becomes(world: &mut Fmt, step: &Step) {
    let expected = docstring(step).trim();
    let actual = world
        .formatted
        .as_deref()
        .unwrap_or_else(|| panic!("expected formatted output, got error {:?}", world.error));
    assert_eq!(
        actual.trim(),
        expected,
        "formatted source mismatch\n  expected:\n{expected}\n  actual:\n{}",
        actual.trim(),
    );
}

#[then("it is left unchanged")]
fn left_unchanged(world: &mut Fmt) {
    let actual = world
        .formatted
        .as_deref()
        .unwrap_or_else(|| panic!("expected unchanged output, got error {:?}", world.error));
    assert_eq!(
        actual.trim(),
        world.source.trim(),
        "expected the already-canonical source to be returned unchanged",
    );
}

#[then("formatting is refused")]
fn formatting_refused(world: &mut Fmt) {
    assert!(
        world.error.is_some(),
        "expected formatting to be refused (syntax error), but it produced output: {:?}",
        world.formatted,
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
    // Features live at the repo top (`tooling/fmt/`), resolved from this crate's
    // manifest directory — a test's working directory is the crate, not the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("fmt");
    futures::executor::block_on(Fmt::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
