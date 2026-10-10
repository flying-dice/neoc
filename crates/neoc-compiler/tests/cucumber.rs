//! Executable transpiler spec for `neoc-compiler`.
//!
//! The Gherkin features in `tooling/compiler/` are the behavioural contract for
//! the public `transpile` seam: neoc source goes in, emitted target source and
//! stage-tagged diagnostics come out. The step definitions drive
//! `neoc_compiler::transpile` directly — source in, [`CompileOutput`] out — so
//! the features *are* the spec, mirroring `lang/CONFORMANCE/` and
//! `crates/neoc-lsp/tests/cucumber.rs`.
//!
//! Scenarios tagged `@pending` describe behaviour that is specified but not yet
//! verifiable here; the runner filters them out so the suite stays green. Drop
//! the tag once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use neoc_compiler::{CompileOutput, Stage, Target, transpile};

/// State threaded through one scenario: the source under test and the result of
/// the last transpile — the emitted target text plus its stage-tagged
/// diagnostics.
#[derive(Debug, Default, World)]
struct Compiler {
    source: String,
    output: Option<CompileOutput>,
}

#[given("the source:")]
fn given_source(world: &mut Compiler, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I transpile to lua")]
fn transpile_to_lua(world: &mut Compiler) {
    world.output = Some(transpile(&world.source, Target::Lua));
}

#[then("it succeeds")]
fn it_succeeds(world: &mut Compiler) {
    let out = output(world);
    assert!(
        out.ok(),
        "expected success, but transpile reported diagnostics:\n{}",
        render_diagnostics(out),
    );
}

#[then("it fails")]
fn it_fails(world: &mut Compiler) {
    let out = output(world);
    assert!(
        !out.ok(),
        "expected failure, but transpile reported no diagnostics; output was:\n{}",
        out.output,
    );
}

#[then(regex = r#"^a diagnostic reports "([^"]*)"$"#)]
fn diagnostic_reports(world: &mut Compiler, substring: String) {
    let out = output(world);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.message.contains(&substring)),
        "no diagnostic message contains {substring:?}; got:\n{}",
        render_diagnostics(out),
    );
}

#[then(regex = r#"^a (parse|type|emit) diagnostic reports "([^"]*)"$"#)]
fn staged_diagnostic_reports(world: &mut Compiler, stage: String, substring: String) {
    let want = match stage.as_str() {
        "parse" => Stage::Parse,
        "type" => Stage::Type,
        "emit" => Stage::Emit,
        other => panic!("unknown stage {other:?}"),
    };
    let out = output(world);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.stage == want && d.message.contains(&substring)),
        "no {stage} diagnostic contains {substring:?}; got:\n{}",
        render_diagnostics(out),
    );
}

#[then(regex = r#"^the output contains "([^"]*)"$"#)]
fn output_contains(world: &mut Compiler, snippet: String) {
    assert_output_contains(world, &snippet);
}

/// Docstring form of `the output contains`, for a snippet that itself holds the
/// `"` a quoted step argument cannot carry (e.g. an emitted Lua string literal).
#[then("the output contains:")]
fn output_contains_docstring(world: &mut Compiler, step: &Step) {
    let snippet = docstring(step).trim();
    assert_output_contains(world, snippet);
}

fn assert_output_contains(world: &Compiler, snippet: &str) {
    let out = output(world);
    assert!(
        out.output.contains(snippet),
        "output is missing {snippet:?}; output was:\n{}",
        out.output,
    );
}

#[then(regex = r#"^the output does not contain "([^"]*)"$"#)]
fn output_does_not_contain(world: &mut Compiler, snippet: String) {
    assert_output_absent(world, &snippet);
}

/// Docstring form of `the output does not contain`, for a snippet that holds a
/// `"` (e.g. asserting Lua's `..` chosen over a `"…" + …` concat).
#[then("the output does not contain:")]
fn output_does_not_contain_docstring(world: &mut Compiler, step: &Step) {
    let snippet = docstring(step).trim();
    assert_output_absent(world, snippet);
}

fn assert_output_absent(world: &Compiler, snippet: &str) {
    let out = output(world);
    assert!(
        !out.output.contains(snippet),
        "output unexpectedly contains {snippet:?}; output was:\n{}",
        out.output,
    );
}

#[then("the output is:")]
fn output_is(world: &mut Compiler, step: &Step) {
    let expected = docstring(step).trim();
    let out = output(world);
    assert_eq!(
        out.output.trim(),
        expected,
        "emitted output mismatch\n  expected:\n{expected}\n  actual:\n{}",
        out.output.trim(),
    );
}

#[then("the output is empty")]
fn output_is_empty(world: &mut Compiler) {
    let out = output(world);
    assert!(
        out.output.is_empty(),
        "expected empty output, got:\n{}",
        out.output,
    );
}

/// The [`CompileOutput`] from the last `When`, panicking if no transpile ran.
fn output(world: &Compiler) -> &CompileOutput {
    world
        .output
        .as_ref()
        .expect("a `When I transpile` step must run before asserting on the output")
}

/// Every diagnostic rendered `[Stage] message` per line, for assertion failures.
fn render_diagnostics(out: &CompileOutput) -> String {
    out.diagnostics
        .iter()
        .map(|d| format!("  [{:?}] {}", d.stage, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

fn main() {
    // Features live at the repo top (`tooling/compiler/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("compiler");
    futures::executor::block_on(Compiler::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
