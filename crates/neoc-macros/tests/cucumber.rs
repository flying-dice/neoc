//! Executable derive spec for `neoc-macros`.
//!
//! The Gherkin features in `tooling/macros/` are the behavioural contract for
//! the built-in derives (`Clone`, `Eq`, `Debug`, `Display`). Each scenario
//! gives a real `.neoc` `struct` or `enum`, the step definitions parse it with
//! `neoc-parser`, pull the declaration out of the AST, and call the crate's
//! real entry point — `neoc_macros::derive` — asserting the emitted Lua 5.1
//! lines. The features *are* the spec; they mirror the crate's own
//! `#[cfg(test)] mod tests` and `lang/DYNAMIC.md` §6–§7.
//!
//! Scenarios tagged `@pending` describe specified-but-unimplemented behaviour;
//! the runner filters them out so the suite stays green. Drop the tag once the
//! behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use neoc_ast::{self as ast, ItemKind};
use neoc_macros::{DeriveTarget, Helper, derive};

/// State threaded through one scenario: the parsed declaration under test and
/// the outcome of the last derive — either the emitted Lua lines or the
/// neoc-flavoured error message.
#[derive(Debug, Default, World)]
struct Macros {
    /// The parsed source. The `struct`/`enum` declaration is the first item;
    /// each `When` step borrows it to build a `DeriveTarget`.
    module: Option<ast::Module>,
    /// The emitted Lua lines from the last successful derive.
    emitted: Option<Vec<String>>,
    /// The helpers the last successful derive calls.
    helpers: Vec<Helper>,
    /// The error message from the last rejected derive.
    error: Option<String>,
}

#[given("the struct:")]
fn given_struct(world: &mut Macros, step: &Step) {
    world.module = Some(parse_decl(docstring(step)));
}

#[given("the enum:")]
fn given_enum(world: &mut Macros, step: &Step) {
    world.module = Some(parse_decl(docstring(step)));
}

#[when(regex = r"^I derive (\w+)$")]
fn derive_trait(world: &mut Macros, trait_name: String) {
    let module = world.module.as_ref().expect("a Given step must run first");
    let target = first_target(module);
    match derive(&trait_name, target) {
        Ok(derived) => {
            world.emitted = Some(derived.lines);
            world.helpers = derived.helpers;
            world.error = None;
        }
        Err(message) => {
            world.error = Some(message);
            world.emitted = None;
        }
    }
}

#[then(regex = r#"^it emits the line "(.*)"$"#)]
fn emits_the_line(world: &mut Macros, expected: String) {
    let lines = emitted(world);
    assert!(
        lines.iter().any(|line| line == &expected),
        "no emitted line equals {expected:?}\n  emitted:\n{}",
        lines.join("\n"),
    );
}

#[then(regex = r#"^the derive contains "(.*)"$"#)]
fn derive_contains(world: &mut Macros, fragment: String) {
    let lines = emitted(world);
    let joined = lines.join("\n");
    assert!(
        joined.contains(&fragment),
        "the emitted Lua does not contain {fragment:?}\n  emitted:\n{joined}",
    );
}

#[then(regex = r#"^the derive does not contain "(.*)"$"#)]
fn derive_does_not_contain(world: &mut Macros, fragment: String) {
    let joined = emitted(world).join("\n");
    assert!(
        !joined.contains(&fragment),
        "the emitted Lua unexpectedly contains {fragment:?}\n  emitted:\n{joined}",
    );
}

#[then("the emitted Lua is:")]
fn emitted_lua_is(world: &mut Macros, step: &Step) {
    let expected = docstring(step).trim();
    let actual = emitted(world).join("\n");
    assert_eq!(
        actual.trim(),
        expected,
        "emitted Lua mismatch\n  expected:\n{expected}\n  actual:\n{}",
        actual.trim(),
    );
}

#[then(regex = r#"^the derive uses the helper "(Eq|Debug)"$"#)]
fn derive_uses_helper(world: &mut Macros, name: String) {
    let expected = if name == "Eq" {
        Helper::Eq
    } else {
        Helper::Debug
    };
    assert!(
        world.helpers.contains(&expected),
        "expected helper {expected:?}, got {:?}",
        world.helpers,
    );
}

#[then("the derive uses no helper")]
fn derive_uses_no_helper(world: &mut Macros) {
    assert!(
        world.helpers.is_empty(),
        "expected no helper, got {:?}",
        world.helpers,
    );
}

#[then("deriving is rejected")]
fn deriving_is_rejected(world: &mut Macros) {
    assert!(
        world.error.is_some(),
        "expected the derive to be rejected, but it emitted: {:?}",
        world.emitted,
    );
}

#[then(regex = r#"^the rejection message is "(.*)"$"#)]
fn rejection_message_is(world: &mut Macros, expected: String) {
    assert_eq!(
        world.error.as_deref(),
        Some(expected.as_str()),
        "rejection message mismatch",
    );
}

/// The emitted lines from the last derive, panicking (with the captured error)
/// if it was rejected instead.
fn emitted(world: &Macros) -> &[String] {
    world
        .emitted
        .as_deref()
        .unwrap_or_else(|| panic!("expected emitted Lua, got rejection {:?}", world.error))
}

/// Parse a single `struct`/`enum` declaration, asserting the source is
/// syntactically valid — a malformed `Given` is a broken scenario, not a
/// behaviour under test.
fn parse_decl(source: &str) -> ast::Module {
    let result = neoc_parser::parse(source.trim());
    assert!(
        result.ok(),
        "the Given declaration must parse cleanly; diagnostics: {:?}",
        result.diagnostics,
    );
    result.module
}

/// Build a `DeriveTarget` from the first `struct`/`enum` item in `module`.
fn first_target(module: &ast::Module) -> DeriveTarget<'_> {
    for item in &module.items {
        match &item.kind {
            ItemKind::Struct(s) => return DeriveTarget::Struct(s),
            ItemKind::Enum(e) => return DeriveTarget::Enum(e),
            _ => {}
        }
    }
    panic!("the Given source declares no struct or enum");
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

fn main() {
    // Features live at the repo top (`tooling/macros/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("macros");
    futures::executor::block_on(Macros::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
