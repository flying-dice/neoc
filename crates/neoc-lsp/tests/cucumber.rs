//! Executable IDE-UX spec for `neoc-lsp`.
//!
//! The Gherkin features in `tooling/ide/` are the behavioural contract for the
//! language server's editor interactions — one feature per interaction (hover,
//! references, completion, goto, symbols). The step definitions below drive them
//! against the real handlers (`neoc_lsp::hover`, …) over source text, so the
//! features *are* the spec; there is no separate prose document. This mirrors
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
use tower_lsp::lsp_types::{Documentation, TextEdit};

/// State threaded through one scenario: the document under test and the result
/// produced by the last `When` step — hover markdown, reference locations as
/// 1-based `(line, column)` pairs (the editor's own coordinates), a resolved
/// completion's detail and documentation, or the active signature and parameter
/// of a call.
#[derive(Debug, Default, World)]
struct Ide {
    source: String,
    hover: Option<String>,
    references: Vec<(u32, u32)>,
    completion_detail: Option<String>,
    completion_documentation: Option<String>,
    signature_label: Option<String>,
    signature_active_parameter: Option<u32>,
    renamed_source: Option<String>,
    goto_lines: Vec<u32>,
}

#[given("the source:")]
fn given_source(world: &mut Ide, step: &Step) {
    // Trim the docstring's surrounding newlines so the first line of source is
    // line 1 — location-based assertions (references) read in editor
    // coordinates, and byte-based ones (hover) are unaffected.
    docstring(step).trim().clone_into(&mut world.source);
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

#[when(regex = r#"^I find all references to "([^"]+)"$"#)]
fn find_all_references(world: &mut Ide, ident: String) {
    world.references = references_to(&world.source, &ident, true);
}

#[when(regex = r#"^I find references to "([^"]+)" excluding the declaration$"#)]
fn find_references_excluding_declaration(world: &mut Ide, ident: String) {
    world.references = references_to(&world.source, &ident, false);
}

#[then("the references are at:")]
fn references_are_at(world: &mut Ide, step: &Step) {
    let expected = expected_positions(step);
    assert_eq!(
        world.references, expected,
        "references mismatch (line, column, 1-based)\n  expected: {expected:?}\n  actual:   {:?}",
        world.references,
    );
}

#[when(regex = r#"^I resolve the completion "([^"]+)"$"#)]
fn resolve_completion(world: &mut Ide, label: String) {
    let source = &world.source;
    let item = neoc_lsp::completions(source, neoc_lsp::byte_to_position(source, 0))
        .into_iter()
        .find(|item| item.label == label)
        .unwrap_or_else(|| panic!("no completion labelled {label:?}"));
    let resolved = neoc_lsp::resolve_completion(source, item);
    world.completion_detail = resolved.detail;
    world.completion_documentation = match resolved.documentation {
        Some(Documentation::String(text)) => Some(text),
        _ => None,
    };
}

#[then(regex = r#"^the completion detail is "([^"]+)"$"#)]
fn completion_detail_is(world: &mut Ide, expected: String) {
    assert_eq!(
        world.completion_detail.as_deref(),
        Some(expected.as_str()),
        "completion detail mismatch",
    );
}

#[then(regex = r#"^the completion documentation is "([^"]+)"$"#)]
fn completion_documentation_is(world: &mut Ide, expected: String) {
    assert_eq!(
        world.completion_documentation.as_deref(),
        Some(expected.as_str()),
        "completion documentation mismatch",
    );
}

#[then("the completion has no detail")]
fn completion_has_no_detail(world: &mut Ide) {
    assert_eq!(
        world.completion_detail, None,
        "expected no detail, got {:?}",
        world.completion_detail,
    );
}

#[when(regex = r#"^I request signature help after "([^"]+)"$"#)]
fn request_signature_help(world: &mut Ide, marker: String) {
    // The cursor sits at the end of the marker's last occurrence — a call like
    // `add(` also appears in the declaration `fn add(`, and the call follows it.
    let byte = world
        .source
        .rfind(&marker)
        .unwrap_or_else(|| panic!("marker {marker:?} not in source"))
        + marker.len();
    let help = neoc_lsp::signature_help(
        &world.source,
        neoc_lsp::byte_to_position(&world.source, byte),
    );
    world.signature_label = help.as_ref().map(|h| h.signatures[0].label.clone());
    world.signature_active_parameter = help.and_then(|h| h.active_parameter);
}

#[then(regex = r#"^the active signature is "([^"]+)"$"#)]
fn active_signature_is(world: &mut Ide, expected: String) {
    assert_eq!(
        world.signature_label.as_deref(),
        Some(expected.as_str()),
        "active signature mismatch",
    );
}

#[then(regex = r"^the active parameter is (\d+)$")]
fn active_parameter_is(world: &mut Ide, expected: u32) {
    assert_eq!(
        world.signature_active_parameter,
        Some(expected),
        "active parameter mismatch",
    );
}

#[then("there is no signature help")]
fn no_signature_help(world: &mut Ide) {
    assert_eq!(
        world.signature_label, None,
        "expected no signature help, got {:?}",
        world.signature_label,
    );
}

#[when(regex = r#"^I rename "([^"]+)" to "([^"]+)"$"#)]
fn rename_symbol(world: &mut Ide, old: String, new: String) {
    let byte = nth_occurrence(&world.source, &old, 1) + old.len() / 2;
    let position = neoc_lsp::byte_to_position(&world.source, byte);
    let edits = neoc_lsp::rename(&world.source, position, &new);
    world.renamed_source = Some(apply_text_edits(&world.source, &edits));
}

#[then("the source becomes:")]
fn the_source_becomes(world: &mut Ide, step: &Step) {
    let expected = docstring(step).trim();
    let actual = world
        .renamed_source
        .as_deref()
        .unwrap_or_else(|| panic!("no edits produced\nsource:\n{}", world.source));
    assert_eq!(
        actual.trim(),
        expected,
        "renamed source mismatch\n  expected:\n{expected}\n  actual:\n{}",
        actual.trim(),
    );
}

/// Applies `edits` to `source`, right-to-left so earlier byte offsets stay
/// valid as later (rightward) edits are applied first.
fn apply_text_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|edit| {
            (
                neoc_lsp::position_to_byte(source, edit.range.start),
                neoc_lsp::position_to_byte(source, edit.range.end),
                edit.new_text.as_str(),
            )
        })
        .collect();
    spans.sort_by_key(|&(start, _, _)| start);
    let mut result = source.to_string();
    for (start, end, text) in spans.into_iter().rev() {
        result.replace_range(start..end, text);
    }
    result
}

#[when(regex = r#"^I go to the definition of "([^"]+)"$"#)]
fn go_to_definition(world: &mut Ide, ident: String) {
    let position = at_use(&world.source, &ident);
    world.goto_lines = neoc_lsp::goto_definition(&world.source, position)
        .map(|def| def.name_range.start.line + 1)
        .into_iter()
        .collect();
}

#[when(regex = r#"^I go to the type of "([^"]+)"$"#)]
fn go_to_type(world: &mut Ide, ident: String) {
    let position = at_use(&world.source, &ident);
    world.goto_lines = neoc_lsp::goto_type_definition(&world.source, position)
        .map(|def| def.name_range.start.line + 1)
        .into_iter()
        .collect();
}

#[when(regex = r#"^I go to the implementations of "([^"]+)"$"#)]
fn go_to_implementations(world: &mut Ide, ident: String) {
    // The declaration name is the first occurrence; clicking it lists impls.
    let byte = nth_occurrence(&world.source, &ident, 1) + ident.len() / 2;
    let position = neoc_lsp::byte_to_position(&world.source, byte);
    world.goto_lines = neoc_lsp::goto_implementation(&world.source, position)
        .iter()
        .map(|def| def.name_range.start.line + 1)
        .collect();
}

#[then(regex = r"^it resolves to line (\d+)$")]
fn it_resolves_to_line(world: &mut Ide, line: u32) {
    assert_eq!(
        world.goto_lines,
        vec![line],
        "goto target lines (1-based) mismatch",
    );
}

#[then("it resolves to nothing")]
fn it_resolves_to_nothing(world: &mut Ide) {
    assert!(
        world.goto_lines.is_empty(),
        "expected no target, got lines {:?}",
        world.goto_lines,
    );
}

/// The cursor at a use of `ident` — its last occurrence, which in these sources
/// follows the declaration, so goto jumps back to the declaration.
fn at_use(source: &str, ident: &str) -> tower_lsp::lsp_types::Position {
    let byte = source.rfind(ident).expect("identifier") + ident.len() / 2;
    neoc_lsp::byte_to_position(source, byte)
}

/// The reference locations for `ident` in `source`, as 1-based `(line, column)`
/// pairs. The cursor is placed inside the name's first occurrence — references
/// resolve by name, so any occurrence yields the same set.
fn references_to(source: &str, ident: &str, include_declaration: bool) -> Vec<(u32, u32)> {
    let byte = nth_occurrence(source, ident, 1) + ident.len() / 2;
    let position = neoc_lsp::byte_to_position(source, byte);
    neoc_lsp::references(source, position, include_declaration)
        .iter()
        .map(|range| (range.start.line + 1, range.start.character + 1))
        .collect()
}

/// The `(line, column)` rows of a step's data table, 1-based, skipping the
/// header row.
fn expected_positions(step: &Step) -> Vec<(u32, u32)> {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    table
        .rows
        .iter()
        .skip(1)
        .map(|row| {
            let line = row[0].trim().parse().expect("line number");
            let column = row[1].trim().parse().expect("column number");
            (line, column)
        })
        .collect()
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
