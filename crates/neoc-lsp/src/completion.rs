//! Completion: offer in-scope names at the cursor.
//!
//! Two modes, chosen from the CST context just before the cursor:
//!
//! - **Member access** after `.` — the fields of the receiver's struct type,
//!   when the receiver type is known.
//! - **Path access** after `Type::` — the variants of that enum.
//! - **Top-level** otherwise — every declared item name plus the neoc
//!   keywords.

use neoc_types::{StructFields, SymbolTable, TypeDef};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

use crate::position::position_to_byte;

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
    if let Some(items) = field_completions(source, byte, &table) {
        return items;
    }
    top_level_completions(&table)
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

/// After `receiver.`, the fields of the receiver's struct type.
fn field_completions(
    source: &str,
    byte: usize,
    table: &SymbolTable,
) -> Option<Vec<CompletionItem>> {
    let prefix = source.get(..byte)?;
    let trimmed = prefix.trim_end();
    let receiver = trimmed.strip_suffix('.')?;
    let recv_name = receiver
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?;
    if recv_name.is_empty() {
        return None;
    }
    // Resolve the receiver name to a struct type: either a const/binding whose
    // type is a named struct, or the struct name itself.
    let struct_name = resolve_struct_name(table, recv_name)?;
    let TypeDef::Struct(s) = table.types.get(&struct_name)? else {
        return None;
    };
    let StructFields::Named(fields) = &s.fields else {
        return None;
    };
    Some(
        fields
            .iter()
            .map(|(name, _)| simple_item(name, CompletionItemKind::FIELD))
            .collect(),
    )
}

/// The struct type a receiver name denotes, if determinable from the symbol
/// table: the name is itself a struct, or it is a value whose type is a named
/// struct.
fn resolve_struct_name(table: &SymbolTable, name: &str) -> Option<String> {
    if matches!(table.types.get(name), Some(TypeDef::Struct(_))) {
        return Some(name.to_string());
    }
    if let Some(neoc_types::Ty::Named { name: ty, .. }) = table.values.get(name)
        && matches!(table.types.get(ty), Some(TypeDef::Struct(_)))
    {
        return Some(ty.clone());
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
}
