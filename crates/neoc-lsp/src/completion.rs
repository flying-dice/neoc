//! Completion: offer in-scope names at the cursor.
//!
//! Two modes, chosen from the CST context just before the cursor:
//!
//! - **Member access** after `.` — the fields and methods of the receiver's
//!   type, found from the type inferred for the receiver expression (so a
//!   parameter or local receiver resolves), falling back to the name.
//! - **Path access** after `Type::` — the variants of that enum.
//! - **Top-level** otherwise — every declared item name plus the neoc
//!   keywords.

use neoc_types::{StructFields, SymbolTable, TypeDef};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Documentation};

use crate::position::position_to_byte;
use crate::signature;

/// neoc keywords offered at the top level (`SYNTAX.md`).
const KEYWORDS: &[&str] = &[
    "fn", "let", "const", "struct", "enum", "trait", "impl", "type", "use", "match", "if", "else",
    "for", "while", "loop", "break", "continue", "return", "true", "false", "self",
];

/// The completion items offered at `position`.
#[must_use]
pub fn completions(source: &str, position: tower_lsp::lsp_types::Position) -> Vec<CompletionItem> {
    let byte = position_to_byte(source, position);
    let table = neoc_types::collect(&neoc_parser::parse(source).module);

    if let Some(items) = path_completions(source, byte, &table) {
        return items;
    }
    if let Some(items) = member_completions(source, byte, &table) {
        return items;
    }
    top_level_completions(&table)
}

/// Fills a completion `item`'s detail and documentation, resolved by its label
/// against the document's top-level items.
///
/// The editor sends this back for the entry the user focuses; until then the
/// item carries only a label and kind. Top-level functions, types, and consts
/// gain a signature (`detail`) and their `///` doc comment (`documentation`);
/// keywords and members — which name no top-level item — are returned
/// unchanged.
#[must_use]
pub fn resolve(source: &str, mut item: CompletionItem) -> CompletionItem {
    if let Some((sig, doc)) = signature::item_signature_and_doc(source, &item.label) {
        item.detail = Some(sig);
        item.documentation = doc.map(Documentation::String);
    }
    item
}

/// After `Type::`, the variants of that enum.
///
/// Context is read from the source text immediately before the cursor rather
/// than the CST: a half-typed `Type::` parses with errors, so the byte prefix
/// is the reliable signal.
fn path_completions(source: &str, byte: usize, table: &SymbolTable) -> Option<Vec<CompletionItem>> {
    let prefix = source.get(..byte)?;
    let trimmed = prefix.trim_end();
    let type_name = trimmed
        .strip_suffix("::")?
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?;
    if type_name.is_empty() {
        return None;
    }
    let TypeDef::Enum(e) = table.types.get(type_name)? else {
        return None;
    };
    Some(
        e.variants
            .iter()
            .map(|v| simple_item(&v.name, CompletionItemKind::ENUM_MEMBER))
            .collect(),
    )
}

/// After `receiver.`, the members of the receiver's type — its named fields
/// (for a struct) and every method on it (inherent, trait-impl, derived).
fn member_completions(
    source: &str,
    byte: usize,
    table: &SymbolTable,
) -> Option<Vec<CompletionItem>> {
    let prefix = source.get(..byte)?;
    let before_dot = prefix.trim_end().strip_suffix('.')?.trim_end();
    let recv_name = before_dot
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?;
    if recv_name.is_empty() {
        return None;
    }
    let type_name = receiver_type(source, before_dot.len(), recv_name, table)?;
    let mut out = Vec::new();
    if let Some(TypeDef::Struct(s)) = table.types.get(&type_name)
        && let StructFields::Named(fields) = &s.fields
    {
        for (name, _) in fields {
            out.push(simple_item(name, CompletionItemKind::FIELD));
        }
    }
    for (ty, method) in table.methods.keys() {
        if *ty == type_name {
            out.push(simple_item(method, CompletionItemKind::METHOD));
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// The named type of the receiver: first the type inferred for the receiver
/// expression ending at `recv_end` (handles parameters, locals, and chained
/// access), falling back to the receiver name itself naming a type or a
/// top-level value of a named type.
fn receiver_type(
    source: &str,
    recv_end: usize,
    recv_name: &str,
    table: &SymbolTable,
) -> Option<String> {
    // The receiver's last byte is `recv_end - 1`; find the narrowest expression
    // type covering it, written to avoid the underflow-prone subtraction.
    let inferred = neoc_types::expression_types(&neoc_parser::parse(source).module)
        .into_iter()
        .filter(|t| t.start < recv_end && recv_end <= t.end)
        .min_by_key(|t| t.end - t.start);
    if let Some(t) = inferred
        && let neoc_types::Ty::Named { name, .. } = t.ty
        && table.types.contains_key(&name)
    {
        return Some(name);
    }
    if table.types.contains_key(recv_name) {
        return Some(recv_name.to_string());
    }
    if let Some(neoc_types::Ty::Named { name, .. }) = table.values.get(recv_name)
        && table.types.contains_key(name)
    {
        return Some(name.clone());
    }
    None
}

/// Every top-level item name plus the keywords.
fn top_level_completions(table: &SymbolTable) -> Vec<CompletionItem> {
    let mut out = Vec::new();
    for name in table.values.keys() {
        out.push(simple_item(name, CompletionItemKind::FUNCTION));
    }
    for name in table.types.keys() {
        out.push(simple_item(name, CompletionItemKind::STRUCT));
    }
    for kw in KEYWORDS {
        out.push(simple_item(kw, CompletionItemKind::KEYWORD));
    }
    out
}

fn simple_item(label: &str, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        ..CompletionItem::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    /// Resolves the top-level completion labelled `label`, as the editor would
    /// when the user focuses it.
    fn resolve_labelled(src: &str, label: &str) -> CompletionItem {
        let item = completions(src, byte_to_position(src, 0))
            .into_iter()
            .find(|i| i.label == label)
            .unwrap_or_else(|| panic!("no completion labelled {label:?}"));
        resolve(src, item)
    }

    fn doc_text(item: &CompletionItem) -> Option<&str> {
        match &item.documentation {
            Some(Documentation::String(s)) => Some(s),
            _ => None,
        }
    }

    #[test]
    fn top_level_includes_items_and_keywords() {
        let src = "fn fib(n: number) -> number { n }\nstruct Point { x: number }\n";
        let items = completions(src, byte_to_position(src, 0));
        let ls = labels(&items);
        assert!(ls.iter().any(|l| l == "fib"), "fib missing: {ls:?}");
        assert!(ls.iter().any(|l| l == "Point"), "Point missing: {ls:?}");
        assert!(ls.iter().any(|l| l == "fn"), "keyword missing: {ls:?}");
    }

    #[test]
    fn enum_variants_after_path_sep() {
        let src = "enum Shape { Circle, Origin }\nfn m() -> Shape { Shape:: }\n";
        let byte = src.rfind("::").unwrap() + 2;
        let items = completions(src, byte_to_position(src, byte));
        let ls = labels(&items);
        assert_eq!(ls, vec!["Circle", "Origin"]);
    }

    #[test]
    fn struct_fields_after_dot() {
        let src = "struct Point { x: number, y: number }\nconst P: Point = Point { x: 1, y: 2 };\nfn m() -> number { P. }\n";
        let byte = src.rfind("P.").unwrap() + 2;
        let items = completions(src, byte_to_position(src, byte));
        let ls = labels(&items);
        assert_eq!(ls, vec!["x", "y"]);
    }

    #[test]
    fn member_completion_offers_fields_and_methods_on_a_parameter() {
        // The receiver is a parameter `c: C` — the old name-only resolver could
        // not type it. Members include the field `n` and the method `get`.
        let src = "struct C { n: number }\nimpl C {\n  fn get(self) -> number { self.n }\n}\nfn m(c: C) -> number { c. }\n";
        let byte = src.rfind("c.").unwrap() + 2;
        let ls = labels(&completions(src, byte_to_position(src, byte)));
        assert!(ls.iter().any(|l| l == "n"), "field `n` missing: {ls:?}");
        assert!(
            ls.iter().any(|l| l == "get"),
            "method `get` missing: {ls:?}"
        );
    }

    #[test]
    fn resolve_fills_function_signature_and_doc() {
        let src =
            "/// Adds one.\nfn inc(n: number) -> number { n }\nfn main() -> number { inc(1) }\n";
        let item = resolve_labelled(src, "inc");
        assert_eq!(item.detail.as_deref(), Some("fn inc(number) -> number"));
        assert_eq!(doc_text(&item), Some("Adds one."));
    }

    #[test]
    fn resolve_fills_struct_signature() {
        let src = "struct Point { x: number, y: number }\n";
        let item = resolve_labelled(src, "Point");
        assert_eq!(
            item.detail.as_deref(),
            Some("struct Point { x: number, y: number }")
        );
    }

    #[test]
    fn resolve_leaves_keywords_unchanged() {
        let src = "fn main() {}\n";
        let item = resolve_labelled(src, "fn");
        assert_eq!(item.detail, None);
        assert!(item.documentation.is_none());
    }
}
