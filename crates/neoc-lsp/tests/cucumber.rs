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
//! implemented. The runner filters them out so the suite stays green; drop the
//! tag once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
// That is the framework's contract, not a needless move.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;

use cucumber::{World, gherkin::Step, given, then, when};
use tower_lsp::lsp_types::{
    DiagnosticSeverity, DocumentHighlightKind, Documentation, InlayHintLabel, TextEdit, Url,
};

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
    edited_source: Option<String>,
    goto_lines: Vec<u32>,
    highlights: Vec<(u32, u32, String)>,
    folds: Vec<(u32, u32)>,
    selection_steps: Vec<String>,
    inlay_labels: Vec<String>,
    workspace_symbol_names: Vec<String>,
    /// Project index for multi-file scenarios, populated by `Given the file
    /// "<path>":`. When present, cross-file `When` steps query it instead of the
    /// single `source`.
    index: Option<neoc_lsp::WorkspaceIndex>,
    /// The file a cross-file goto resolved to, as a project-relative path.
    cross_file_goto: Option<String>,
    /// Project-relative paths a cross-file references query touched, sorted.
    cross_file_reference_files: Vec<String>,
    /// Project-relative paths a cross-file rename edited, sorted.
    cross_file_rename_files: Vec<String>,
    call_hierarchy_names: Vec<String>,
    semantic_tokens: Vec<(u32, u32, String)>,
    diagnostics: Vec<(String, String)>,
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

#[when("I format the document")]
fn format_document(world: &mut Ide) {
    let edits = neoc_lsp::formatting(&world.source).unwrap_or_default();
    world.edited_source = Some(apply_text_edits(&world.source, &edits));
}

#[when("I organize imports")]
fn organize_imports(world: &mut Ide) {
    let edits = neoc_lsp::organize_imports(&world.source).unwrap_or_default();
    world.edited_source = Some(apply_text_edits(&world.source, &edits));
}

#[when(regex = r#"^I remove the unused import "([^"]+)"$"#)]
fn remove_unused_import(world: &mut Ide, name: String) {
    let edits: Vec<TextEdit> = neoc_lsp::unused_imports(&world.source)
        .into_iter()
        .filter(|unused| unused.name == name)
        .map(|unused| TextEdit {
            range: unused.removal,
            new_text: String::new(),
        })
        .collect();
    world.edited_source = Some(apply_text_edits(&world.source, &edits));
}

#[when(regex = r#"^I rename "([^"]+)" to "([^"]+)"$"#)]
fn rename_symbol(world: &mut Ide, old: String, new: String) {
    let byte = nth_occurrence(&world.source, &old, 1) + old.len() / 2;
    let position = neoc_lsp::byte_to_position(&world.source, byte);
    let edits = neoc_lsp::rename(&world.source, position, &new);
    world.edited_source = Some(apply_text_edits(&world.source, &edits));
}

#[then("the source becomes:")]
fn the_source_becomes(world: &mut Ide, step: &Step) {
    let expected = docstring(step).trim();
    let actual = world
        .edited_source
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

#[when(regex = r#"^I highlight "([^"]+)"$"#)]
fn highlight_symbol(world: &mut Ide, ident: String) {
    let byte = nth_occurrence(&world.source, &ident, 1) + ident.len() / 2;
    let position = neoc_lsp::byte_to_position(&world.source, byte);
    world.highlights = neoc_lsp::document_highlight(&world.source, position)
        .into_iter()
        .map(|highlight| {
            let kind = match highlight.kind {
                Some(DocumentHighlightKind::WRITE) => "write",
                Some(DocumentHighlightKind::READ) => "read",
                _ => "text",
            };
            (
                highlight.range.start.line + 1,
                highlight.range.start.character + 1,
                kind.to_string(),
            )
        })
        .collect();
}

#[when(regex = r#"^I expand the selection at "([^"]+)"$"#)]
fn expand_selection(world: &mut Ide, marker: String) {
    let byte = nth_occurrence(&world.source, &marker, 1) + marker.len() / 2;
    let position = neoc_lsp::byte_to_position(&world.source, byte);
    let mut steps = Vec::new();
    let mut node = neoc_lsp::selection_range(&world.source, position);
    while let Some(selection) = node {
        let start = neoc_lsp::position_to_byte(&world.source, selection.range.start);
        let end = neoc_lsp::position_to_byte(&world.source, selection.range.end);
        steps.push(world.source[start..end].to_string());
        node = selection.parent.map(|parent| *parent);
    }
    world.selection_steps = steps;
}

#[then(regex = r#"^the innermost selection is "([^"]+)"$"#)]
fn innermost_selection_is(world: &mut Ide, expected: String) {
    assert_eq!(
        world.selection_steps.first().map(String::as_str),
        Some(expected.as_str()),
        "innermost selection mismatch; steps: {:?}",
        world.selection_steps,
    );
}

#[then(regex = r#"^the selection eventually covers "([^"]+)"$"#)]
fn selection_covers(world: &mut Ide, expected: String) {
    assert!(
        world.selection_steps.iter().any(|step| step == &expected),
        "no selection step covers {expected:?}; steps: {:?}",
        world.selection_steps,
    );
}

#[when("I request semantic tokens")]
fn request_semantic_tokens(world: &mut Ide) {
    world.semantic_tokens = neoc_lsp::semantic_tokens(&world.source)
        .into_iter()
        .map(|token| {
            let kind = neoc_lsp::TOKEN_TYPES[token.type_index as usize]
                .as_str()
                .to_string();
            // 1-based line and column for the editor's coordinates.
            (token.line + 1, token.start + 1, kind)
        })
        .collect();
}

#[then("the semantic tokens include:")]
fn semantic_tokens_include(world: &mut Ide, step: &Step) {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    for row in table.rows.iter().skip(1) {
        let expected = (
            row[0].trim().parse::<u32>().expect("line"),
            row[1].trim().parse::<u32>().expect("column"),
            row[2].trim().to_string(),
        );
        assert!(
            world.semantic_tokens.contains(&expected),
            "missing token {expected:?}; got {:?}",
            world.semantic_tokens,
        );
    }
}

#[when("I check the document")]
fn check_document(world: &mut Ide) {
    world.diagnostics = neoc_lsp::diagnose(&world.source)
        .into_iter()
        .map(|diag| {
            let severity = match diag.severity {
                Some(DiagnosticSeverity::ERROR) => "error",
                Some(DiagnosticSeverity::WARNING) => "warning",
                _ => "info",
            };
            (severity.to_string(), diag.message)
        })
        .collect();
}

#[then("the diagnostics include:")]
fn diagnostics_include(world: &mut Ide, step: &Step) {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    for row in table.rows.iter().skip(1) {
        let expected = (row[0].trim().to_string(), row[1].trim().to_string());
        assert!(
            world.diagnostics.contains(&expected),
            "missing diagnostic {expected:?}; got {:?}",
            world.diagnostics,
        );
    }
}

#[when(regex = r#"^I find the callers of "([^"]+)"$"#)]
fn find_callers(world: &mut Ide, function: String) {
    world.call_hierarchy_names = neoc_lsp::incoming_calls(&world.source, &function)
        .into_iter()
        .map(|edge| edge.item.name)
        .collect();
}

#[when(regex = r#"^I find the callees of "([^"]+)"$"#)]
fn find_callees(world: &mut Ide, function: String) {
    world.call_hierarchy_names = neoc_lsp::outgoing_calls(&world.source, &function)
        .into_iter()
        .map(|edge| edge.item.name)
        .collect();
}

#[then("the call hierarchy lists:")]
fn call_hierarchy_lists(world: &mut Ide, step: &Step) {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    let expected: Vec<String> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| row[0].trim().to_string())
        .collect();
    assert_eq!(
        world.call_hierarchy_names, expected,
        "call hierarchy mismatch",
    );
}

/// The synthetic workspace root for multi-file scenarios; `use` paths resolve
/// relative to it the same way the compiler resolves a project.
const SPEC_ROOT: &str = if cfg!(windows) { "C:\\spec" } else { "/spec" };

#[given(regex = r#"^the file "([^"]+)":$"#)]
fn given_file(world: &mut Ide, path: String, step: &Step) {
    let text = docstring(step).trim().to_string();
    let root = Path::new(SPEC_ROOT);
    let index = world
        .index
        .get_or_insert_with(|| neoc_lsp::WorkspaceIndex::new(root));
    let uri = Url::from_file_path(root.join(path)).expect("a valid file path");
    index.set_document(uri, 1, text);
}

#[when(regex = r#"^I search workspace symbols for "([^"]*)"$"#)]
fn search_workspace_symbols(world: &mut Ide, query: String) {
    // A multi-file scenario queries the project index (results sorted, since the
    // index has no inherent file order); a single-file one keeps the document
    // scan and its declaration order.
    world.workspace_symbol_names = if let Some(index) = &world.index {
        let mut names: Vec<String> = index
            .workspace_symbols(&query)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        names.sort();
        names
    } else {
        neoc_lsp::workspace_symbols(&world.source, &query)
            .into_iter()
            .map(|symbol| symbol.name)
            .collect()
    };
}

/// The URI of a spec file path under the synthetic project root.
fn spec_uri(path: &str) -> Url {
    Url::from_file_path(Path::new(SPEC_ROOT).join(path)).expect("a valid file path")
}

/// A spec URI rendered back to its project-relative path (forward slashes), for
/// asserting which file a cross-file result landed in.
fn relative_path(uri: &Url) -> String {
    let path = uri.to_file_path().expect("a file URI");
    path.strip_prefix(SPEC_ROOT)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The first column of a step's table, skipping the header row.
fn table_column(step: &Step) -> Vec<String> {
    step.table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value))
        .rows
        .iter()
        .skip(1)
        .map(|row| row[0].trim().to_string())
        .collect()
}

#[when(regex = r#"^I goto the definition of "([^"]+)" from "([^"]+)"$"#)]
fn goto_def_cross_file(world: &mut Ide, name: String, path: String) {
    let index = world.index.as_ref().expect("a multi-file scenario");
    world.cross_file_goto = index
        .imported_definition(&spec_uri(&path), &name)
        .map(|loc| relative_path(&loc.uri));
}

#[then(regex = r#"^the definition is in "([^"]+)"$"#)]
fn definition_is_in(world: &mut Ide, path: String) {
    assert_eq!(
        world.cross_file_goto.as_deref(),
        Some(path.as_str()),
        "cross-file goto landed in the wrong file",
    );
}

#[when(regex = r#"^I find references to "([^"]+)" from "([^"]+)"$"#)]
fn references_cross_file(world: &mut Ide, name: String, path: String) {
    let index = world.index.as_ref().expect("a multi-file scenario");
    let mut files: Vec<String> = neoc_lsp::project_references(index, &spec_uri(&path), &name, true)
        .into_iter()
        .map(|(uri, _)| relative_path(&uri))
        .collect();
    files.sort();
    files.dedup();
    world.cross_file_reference_files = files;
}

#[then("the references span the files:")]
fn references_span_files(world: &mut Ide, step: &Step) {
    assert_eq!(world.cross_file_reference_files, table_column(step));
}

#[when(regex = r#"^I rename "([^"]+)" to "([^"]+)" from "([^"]+)"$"#)]
fn rename_cross_file(world: &mut Ide, name: String, new_name: String, path: String) {
    let index = world.index.as_ref().expect("a multi-file scenario");
    let mut files: Vec<String> =
        neoc_lsp::project_rename(index, &spec_uri(&path), &name, &new_name)
            .into_keys()
            .map(|uri| relative_path(&uri))
            .collect();
    files.sort();
    world.cross_file_rename_files = files;
}

#[then("the rename edits the files:")]
fn rename_edits_files(world: &mut Ide, step: &Step) {
    assert_eq!(world.cross_file_rename_files, table_column(step));
}

#[then("the workspace symbols are:")]
fn workspace_symbols_are(world: &mut Ide, step: &Step) {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    let expected: Vec<String> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| row[0].trim().to_string())
        .collect();
    assert_eq!(
        world.workspace_symbol_names, expected,
        "workspace symbols mismatch",
    );
}

#[when("I request inlay hints")]
fn request_inlay_hints(world: &mut Ide) {
    world.inlay_labels = neoc_lsp::inlay_hints(&world.source)
        .into_iter()
        .map(|hint| match hint.label {
            InlayHintLabel::String(text) => text,
            InlayHintLabel::LabelParts(_) => String::new(),
        })
        .collect();
}

#[then("the inlay hints are:")]
fn inlay_hints_are(world: &mut Ide, step: &Step) {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    let expected: Vec<String> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| row[0].trim().to_string())
        .collect();
    assert_eq!(world.inlay_labels, expected, "inlay hints mismatch");
}

#[when("I request folds")]
fn request_folds(world: &mut Ide) {
    world.folds = neoc_lsp::folding_ranges(&world.source)
        .into_iter()
        .map(|fold| (fold.start_line + 1, fold.end_line + 1))
        .collect();
}

#[then("the folds are:")]
fn the_folds_are(world: &mut Ide, step: &Step) {
    let expected = expected_positions(step);
    assert_eq!(
        world.folds, expected,
        "folds mismatch (start, end, 1-based)\n  expected: {expected:?}\n  actual:   {:?}",
        world.folds,
    );
}

#[then("the highlights are:")]
fn highlights_are(world: &mut Ide, step: &Step) {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("step {:?} requires a table", step.value));
    let expected: Vec<(u32, u32, String)> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| {
            (
                row[0].trim().parse().expect("line"),
                row[1].trim().parse().expect("column"),
                row[2].trim().to_string(),
            )
        })
        .collect();
    assert_eq!(
        world.highlights, expected,
        "highlights mismatch (line, column, kind)",
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
