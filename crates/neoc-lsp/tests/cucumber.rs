//! Executable IDE-UX spec for `neoc-lsp`.
//!
//! The Gherkin features in `tooling/ide/` are the behavioural contract for the
//! language server's editor interactions — one feature per interaction (hover,
//! completion, goto, symbols). The step definitions below drive them against
//! the real handlers (`neoc_lsp::hover`, …) over source text, so the features
//! *are* the spec; there is no separate prose document. This mirrors
//! `lang/CONFORMANCE/`: the spec lives at the repo top, executed by a crate
//! test. See `flying-dice/neoc#60`.
//!
//! Scenarios tagged `@pending` describe behaviour that is specified but not yet
//! implemented (e.g. call-site type instantiation). The runner filters them out
//! so the suite stays green; drop the tag once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
// That is the framework's contract, not a needless move.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the document under test and the hover
/// markdown produced by the last `When` step.
#[derive(Debug, Default, World)]
struct Ide {
    source: String,
    hover: Option<String>,
}

#[given("the source:")]
fn given_source(world: &mut Ide, step: &Step) {
    docstring(step).clone_into(&mut world.source);
}

#[when(regex = r#"^I hover on "([^"]+)" at its declaration$"#)]
fn hover_at_declaration(world: &mut Ide, ident: String) {
    // The declaration is the first occurrence of the name in the source.
    world.hover = hover_at(&world.source, &ident, 1);
}

#[when(regex = r#"^I hover on "([^"]+)" at the call site$"#)]
fn hover_at_call_site(world: &mut Ide, ident: String) {
    // In these specs the call follows the declaration, so the call site is the
    // second occurrence of the name.
    world.hover = hover_at(&world.source, &ident, 2);
}

#[then("the hover shows:")]
fn hover_shows(world: &mut Ide, step: &Step) {
    let expected = docstring(step).trim();
    let Some(actual) = world.hover.as_deref() else {
        panic!("expected a hover, got none\nsource:\n{}", world.source);
    };
    assert!(
        actual.contains(expected),
        "hover missing the expected signature\n  expected substring: {expected:?}\n  actual hover: {actual:?}",
    );
}

/// The hover markdown for the `occurrence`-th appearance of `ident` in `source`,
/// with the cursor inside the name — where an editor's caret would sit. Anchored
/// at the name's midpoint so the offset lands strictly within the identifier for
/// any length, including single-character names (`+ 1` would overshoot to the
/// exclusive end of a 1-char range and miss the declaration).
fn hover_at(source: &str, ident: &str, occurrence: usize) -> Option<String> {
    let byte = nth_occurrence(source, ident, occurrence) + ident.len() / 2;
    neoc_lsp::hover(source, neoc_lsp::byte_to_position(source, byte))
}

/// Byte offset of the 1-based `occurrence`-th occurrence of `needle` in `src`.
fn nth_occurrence(src: &str, needle: &str, occurrence: usize) -> usize {
    let (offset, _) = src
        .match_indices(needle)
        .nth(occurrence - 1)
        .unwrap_or_else(|| panic!("{needle:?} does not occur {occurrence} time(s) in the source"));
    offset
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Features live at the repo top (`tooling/ide/`), resolved from this crate's
    // manifest directory — a test's working directory is the crate, not the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("ide");
    Ide::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    })
    .await;
}
