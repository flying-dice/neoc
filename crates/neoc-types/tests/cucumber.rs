//! Executable static-semantics spec for `neoc-types`.
//!
//! The Gherkin features in `tooling/types/` are the behavioural contract for
//! the type checker: well-typed source passes, and ill-typed source reports a
//! specific, neoc-flavoured diagnostic — `number`, `string`, `bool`,
//! `Result<T, E>`, user-defined `struct`s — never a host-language type name.
//! The step definitions drive `neoc_types::check` over the real parser
//! (source → AST → checker), the same pipeline the LSP's diagnostics handler
//! uses, so the features *are* the spec. This mirrors the grounded assertions
//! in `crates/neoc-types/tests/check.rs` and `lang/CONFORMANCE/`.
//!
//! Scenarios tagged `@pending` describe specified-but-unverified behaviour (or
//! a message that currently leaks a host repr); the runner filters them out so
//! the suite stays green. Drop the tag once the behaviour is settled.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};

/// State threaded through one scenario: the source under test and the checker's
/// diagnostic messages from the last `When I type-check it`.
#[derive(Debug, Default, World)]
struct Types {
    source: String,
    diagnostics: Vec<String>,
}

#[given("the source:")]
fn given_source(world: &mut Types, step: &Step) {
    docstring(step).trim().clone_into(&mut world.source);
}

#[when("I type-check it")]
fn type_check_it(world: &mut Types) {
    // Parse first, then check — the bridge the LSP uses. A parse error in a
    // spec source is a bug in the scenario, not a checker outcome, so refuse it
    // loudly rather than silently checking a malformed tree.
    let parsed = neoc_parser::parse(&world.source);
    assert!(
        parsed.diagnostics.is_empty(),
        "the scenario source has a syntax error, so it cannot exercise the type checker: {:?}",
        parsed.diagnostics,
    );
    world.diagnostics = neoc_types::check(&parsed.module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect();
}

#[then("it type-checks")]
fn it_type_checks(world: &mut Types) {
    assert!(
        world.diagnostics.is_empty(),
        "expected the source to type-check, but it reported: {:?}",
        world.diagnostics,
    );
}

#[then(regex = r#"^a type error reports "([^"]+)"$"#)]
fn type_error_reports(world: &mut Types, expected: String) {
    assert!(
        world.diagnostics.iter().any(|m| m.contains(&expected)),
        "no type error contained {expected:?}\n  diagnostics: {:?}",
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
    // Features live at the repo top (`tooling/types/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("types");
    futures::executor::block_on(Types::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
