//! Executable Python-backend spec for `neoc-codegen-python`.
//!
//! The Gherkin features in `tooling/codegen-python/` are the behavioural
//! contract: a `.neoc` module is parsed (`neoc-parser`) and lowered to idiomatic
//! Python plus its `.pyi` sibling (`neoc_codegen_python::emit`), and the
//! scenarios assert on the emitted source. A struct is a frozen `@dataclass`, an
//! enum a `@dataclass` per variant plus a `Union` alias, `match` lowers to native
//! `match`/`case`, and `Result`/`Option` to tagged dataclasses imported from
//! `neoc_lib`. The step definitions drive the real `emit`, so the features *are*
//! the spec — mirroring the other codegen crates' harnesses.

#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario.
#[derive(Debug, Default, World)]
struct Codegen {
    source: String,
    parse_diagnostics: Vec<String>,
    py: String,
    pyi: String,
    emit_diagnostics: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Codegen, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I emit Python")]
fn emit_python(world: &mut Codegen) {
    let parsed = neoc_parser::parse(&world.source);
    world.parse_diagnostics = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    let result = neoc_codegen_python::emit(&parsed.module, &[]);
    world.py = result.py;
    world.pyi = result.pyi;
    world.emit_diagnostics = result.diagnostics.into_iter().map(|d| d.message).collect();
}

#[then(regex = r#"^the Python contains "(.+)"$"#)]
fn python_contains(world: &mut Codegen, snippet: String) {
    assert!(
        world.py.contains(&snippet),
        "expected the emitted Python to contain {snippet:?}\n  emitted Python:\n{}",
        world.py,
    );
}

#[then(regex = r#"^the Python does not contain "(.+)"$"#)]
fn python_does_not_contain(world: &mut Codegen, snippet: String) {
    assert!(
        !world.py.contains(&snippet),
        "expected the emitted Python to omit {snippet:?}\n  emitted Python:\n{}",
        world.py,
    );
}

#[then(regex = r#"^the .pyi contains "(.+)"$"#)]
fn pyi_contains(world: &mut Codegen, snippet: String) {
    assert!(
        world.pyi.contains(&snippet),
        "expected the emitted .pyi to contain {snippet:?}\n  emitted .pyi:\n{}",
        world.pyi,
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
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("codegen-python");
    futures::executor::block_on(Codegen::filter_run(
        features,
        |_feature, _rule, scenario| !scenario.tags.iter().any(|tag| tag == "pending"),
    ));
}
