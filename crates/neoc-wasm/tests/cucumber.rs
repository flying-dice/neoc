//! Intended browser-entry spec for `neoc-wasm`.
//!
//! The Gherkin features in `tooling/wasm/` document the *contract* for the
//! in-page transpiler: a wasm-bindgen wrapper over `neoc-compiler` that takes
//! `.neoc` source text and a target (`lua` / `js`) and returns the emitted
//! output, for transpilation inside a browser page. Mirrors
//! `crates/neoc-lsp/tests/cucumber.rs` and `crates/neoc-fmt/tests/cucumber.rs`:
//! the spec lives at the repo top, executed by a crate test.
//!
//! Today `neoc-wasm` is a stub — a doc comment over the future wrapper, with no
//! exported function and no wasm-bindgen surface (that lands once the compiler
//! graph is wasm-clean). A native cucumber test cannot drive a wasm-only entry,
//! and there is nothing else to call, so every scenario is tagged
//! `@chaos @pending`. The runner filters `@pending` out, so zero scenarios run
//! and the suite stays green. The step definitions below give the features a
//! place to bind once the wasm-bindgen export exists; until then they panic if
//! ever reached, which the filter guarantees they are not.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the source under test, the chosen
/// target, and the transpiled output once the browser entry produces one. All
/// `@pending` today — the fields are the shape the live wrapper will fill.
#[derive(Debug, Default, World)]
struct Wasm {
    source: String,
    target: String,
    output: Option<String>,
}

#[given("the source:")]
fn given_source(world: &mut Wasm, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[given(regex = r#"^the target is "([^"]+)"$"#)]
fn given_target(world: &mut Wasm, target: String) {
    world.target = target;
}

#[when("I transpile it in the browser")]
fn transpile_in_browser(_world: &mut Wasm) {
    // No wasm-bindgen entry exists yet, so there is nothing to call natively.
    // Every scenario reaching this step is `@pending` and filtered out before
    // it runs; this guard documents that and fails loudly if the filter is ever
    // dropped without wiring the real wrapper.
    unimplemented!(
        "neoc-wasm has no wasm-bindgen entry yet; this scenario must stay @pending \
         until the in-page transpiler over neoc-compiler::transpile lands"
    );
}

#[then("it transpiles to:")]
fn it_transpiles_to(world: &mut Wasm, step: &Step) {
    let expected = docstring(step).trim();
    let actual = world
        .output
        .as_deref()
        .unwrap_or_else(|| panic!("expected transpiled output, got none"));
    assert_eq!(actual.trim(), expected, "transpiled output mismatch");
}

#[then("transpilation is refused")]
fn transpilation_refused(world: &mut Wasm) {
    assert!(
        world.output.is_none(),
        "expected transpilation to be refused, but it produced output: {:?}",
        world.output,
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
    // Features live at the repo top (`tooling/wasm/`), resolved from this crate's
    // manifest directory — a test's working directory is the crate, not the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("wasm");
    futures::executor::block_on(Wasm::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
