//! Executable Lua-codegen spec for `neoc-codegen-lua`.
//!
//! The Gherkin features in `tooling/codegen-lua/` are the behavioural contract
//! for the AST → Lua 5.1 emitter: a `.neoc` source is parsed (`neoc_parser`)
//! and lowered (`neoc_codegen_lua::emit`), and the emitted Lua — or the
//! emit-time diagnostics — are asserted. The step definitions drive the real
//! public API, so the features *are* the spec, mirroring the hand-built and
//! source-golden `#[cfg(test)]` tests in `tests/emit.rs` and the lowering table
//! in `lang/DYNAMIC.md`. See `flying-dice/neoc#60`.
//!
//! Scenarios tagged `@pending` describe behaviour that is specified but not yet
//! implemented (or whose output currently leaks a host-language shape we won't
//! assert). The runner filters them out so the suite stays green; drop the tag
//! once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the source under test and the outcome of
/// the last emit — the emitted Lua text and the diagnostics the emitter raised.
#[derive(Debug, Default, World)]
struct Codegen {
    source: String,
    lua: Option<String>,
    diagnostics: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Codegen, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I emit Lua")]
fn emit_lua(world: &mut Codegen) {
    // The source must parse cleanly first: codegen runs on the AST the parser
    // builds, so a syntax error is a parser concern, not an emitter one.
    let parsed = neoc_parser::parse(&world.source);
    assert!(
        parsed.ok(),
        "the source did not parse; codegen features assume valid source\n  diagnostics: {:?}\n  source:\n{}",
        parsed.diagnostics,
        world.source,
    );
    let result = neoc_codegen_lua::emit(&parsed.module);
    world.diagnostics = result.diagnostics.into_iter().map(|d| d.message).collect();
    world.lua = Some(result.lua);
}

#[then(regex = r#"^the Lua contains "([^"]*)"$"#)]
fn lua_contains(world: &mut Codegen, snippet: String) {
    assert_contains(world, &snippet);
}

/// Docstring variant of `the Lua contains`, for snippets that themselves carry
/// a `"` — the inline `"…"` form can't quote those (`[^"]*` stops at the first
/// inner quote), so a `"""` block holds the literal Lua substring instead.
#[then("the Lua contains:")]
fn lua_contains_docstring(world: &mut Codegen, step: &Step) {
    let snippet = docstring(step).trim().to_string();
    assert_contains(world, &snippet);
}

fn assert_contains(world: &Codegen, snippet: &str) {
    let lua = world.lua.as_deref().expect("emit Lua before asserting it");
    assert!(
        lua.contains(snippet),
        "emitted Lua is missing the expected snippet\n  expected substring: {snippet:?}\n  actual Lua:\n{lua}",
    );
}

#[then(regex = r#"^the Lua does not contain "([^"]*)"$"#)]
fn lua_does_not_contain(world: &mut Codegen, snippet: String) {
    let lua = world.lua.as_deref().expect("emit Lua before asserting it");
    assert!(
        !lua.contains(&snippet),
        "emitted Lua unexpectedly contains {snippet:?}\n  actual Lua:\n{lua}",
    );
}

#[then("the Lua is:")]
fn lua_is(world: &mut Codegen, step: &Step) {
    let expected = docstring(step).trim();
    let lua = world.lua.as_deref().expect("emit Lua before asserting it");
    assert_eq!(
        lua.trim(),
        expected,
        "emitted Lua mismatch\n  expected:\n{expected}\n  actual:\n{}",
        lua.trim(),
    );
}

#[then("the Lua is empty")]
fn lua_is_empty(world: &mut Codegen) {
    let lua = world.lua.as_deref().expect("emit Lua before asserting it");
    assert!(
        lua.trim().is_empty(),
        "expected no emitted Lua, got:\n{lua}",
    );
}

#[then("emitting reports a diagnostic")]
fn reports_a_diagnostic(world: &mut Codegen) {
    // Asserts the *fact* of refusal; `a diagnostic reports "…"` asserts the
    // (neoc-shaped) wording.
    assert!(
        !world.diagnostics.is_empty(),
        "expected an emit diagnostic, but emission was clean\n  Lua:\n{}",
        world.lua.as_deref().unwrap_or(""),
    );
}

#[then("emitting reports no diagnostics")]
fn reports_no_diagnostics(world: &mut Codegen) {
    assert!(
        world.diagnostics.is_empty(),
        "expected clean emission, got diagnostics: {:?}",
        world.diagnostics,
    );
}

#[then(regex = r#"^a diagnostic reports "([^"]*)"$"#)]
fn diagnostic_reports(world: &mut Codegen, substring: String) {
    assert!(
        world.diagnostics.iter().any(|d| d.contains(&substring)),
        "no emit diagnostic contains {substring:?}; got: {:?}",
        world.diagnostics,
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
    // Features live at the repo top (`tooling/codegen-lua/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("codegen-lua");
    futures::executor::block_on(Codegen::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
