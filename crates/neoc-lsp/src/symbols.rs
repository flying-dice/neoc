//! Document symbols (outline): a pure CST walk over the top-level items.
//!
//! Emits a [`DocumentSymbol`] tree — items at the top, with struct fields,
//! enum variants, and `impl` methods as children. Ranges come straight from
//! the CST byte spans.

use tower_lsp::lsp_types::{DocumentSymbol, SymbolKind};
use tree_sitter::Node;

use crate::cst::{self, node_text};
use crate::position::byte_range;

/// Builds the outline for `source`.
#[must_use]
pub fn document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let tree = cst::parse(source);
    let root = tree.root_node();
    let mut out = Vec::new();
    for item in root.named_children(&mut root.walk()) {
        if item.kind() != "item" {
            continue;
        }
        for decl in item.named_children(&mut item.walk()) {
            if let Some(sym) = symbol_for_decl(&decl, source) {
                out.push(sym);
            }
        }
    }
    out
}

fn symbol_for_decl(decl: &Node, source: &str) -> Option<DocumentSymbol> {
    match decl.kind() {
        "function_declaration" => named_symbol(decl, source, SymbolKind::FUNCTION, Vec::new()),
        "const_declaration" => named_symbol(decl, source, SymbolKind::CONSTANT, Vec::new()),
        "type_alias" => named_symbol(decl, source, SymbolKind::TYPE_PARAMETER, Vec::new()),
        "struct_declaration" => {
            let fields = struct_fields(decl, source);
            named_symbol(decl, source, SymbolKind::STRUCT, fields)
        }
        "enum_declaration" => {
            let variants = enum_variants(decl, source);
            named_symbol(decl, source, SymbolKind::ENUM, variants)
        }
        "trait_declaration" => {
            let methods = trait_methods(decl, source);
            named_symbol(decl, source, SymbolKind::INTERFACE, methods)
        }
        "impl_block" => Some(impl_symbol(decl, source)),
        _ => None,
    }
}

/// A `DocumentSymbol` for a node that carries a `name` field.
fn named_symbol(
    decl: &Node,
    source: &str,
    kind: SymbolKind,
    children: Vec<DocumentSymbol>,
) -> Option<DocumentSymbol> {
    let name_node = decl.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?.to_string();
    Some(symbol(
        name,
        kind,
        decl,
        &name_node,
        source,
        nonempty(children),
    ))
}

fn struct_fields(decl: &Node, source: &str) -> Vec<DocumentSymbol> {
    let Some(body) = decl.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in body.named_children(&mut body.walk()) {
        if field.kind() != "field_declaration" {
            continue;
        }
        if let Some(name_node) = field.child_by_field_name("name")
            && let Some(name) = node_text(&name_node, source)
        {
            out.push(symbol(
                name.to_string(),
                SymbolKind::FIELD,
                &field,
                &name_node,
                source,
                None,
            ));
        }
    }
    out
}

fn enum_variants(decl: &Node, source: &str) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for variant in decl.named_children(&mut decl.walk()) {
        if variant.kind() != "variant" {
            continue;
        }
        if let Some(name_node) = variant.child_by_field_name("name")
            && let Some(name) = node_text(&name_node, source)
        {
            out.push(symbol(
                name.to_string(),
                SymbolKind::ENUM_MEMBER,
                &variant,
                &name_node,
                source,
                None,
            ));
        }
    }
    out
}

fn trait_methods(decl: &Node, source: &str) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for item in decl.named_children(&mut decl.walk()) {
        if item.kind() != "trait_item" {
            continue;
        }
        if let Some(name_node) = item.child_by_field_name("name")
            && let Some(name) = node_text(&name_node, source)
        {
            out.push(symbol(
                name.to_string(),
                SymbolKind::METHOD,
                &item,
                &name_node,
                source,
                None,
            ));
        }
    }
    out
}

/// An `impl` block: named after its target type, methods as children.
fn impl_symbol(decl: &Node, source: &str) -> DocumentSymbol {
    let target = decl
        .child_by_field_name("type")
        .and_then(|t| node_text(&t, source))
        .unwrap_or("impl")
        .to_string();
    let mut methods = Vec::new();
    for item in decl.named_children(&mut decl.walk()) {
        if item.kind() != "impl_item" {
            continue;
        }
        for f in item.named_children(&mut item.walk()) {
            if f.kind() != "function_declaration" {
                continue;
            }
            if let Some(name_node) = f.child_by_field_name("name")
                && let Some(name) = node_text(&name_node, source)
            {
                methods.push(symbol(
                    name.to_string(),
                    SymbolKind::METHOD,
                    &f,
                    &name_node,
                    source,
                    None,
                ));
            }
        }
    }
    let name_node = decl.child_by_field_name("type").unwrap_or(*decl);
    symbol(
        format!("impl {target}"),
        SymbolKind::CLASS,
        decl,
        &name_node,
        source,
        nonempty(methods),
    )
}

/// Assembles a `DocumentSymbol` from a name, a full-extent node, and the node
/// used as the selection (the name).
fn symbol(
    name: String,
    kind: SymbolKind,
    full: &Node,
    selection: &Node,
    source: &str,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    let full_range = byte_range(source, full.start_byte(), full.end_byte());
    let sel_range = byte_range(source, selection.start_byte(), selection.end_byte());
    #[allow(deprecated)] // `deprecated` field is required by the struct literal
    DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range: sel_range,
        children,
    }
}

fn nonempty(children: Vec<DocumentSymbol>) -> Option<Vec<DocumentSymbol>> {
    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(syms: &[DocumentSymbol]) -> Vec<(&str, SymbolKind)> {
        syms.iter().map(|s| (s.name.as_str(), s.kind)).collect()
    }

    #[test]
    fn lists_top_level_items() {
        let src = "struct Point { x: number, y: number }\nenum Shape { Circle, Square }\nfn main() {}\nconst PI: number = 3;\ntype Num = number;\n";
        let syms = document_symbols(src);
        assert_eq!(
            names(&syms),
            vec![
                ("Point", SymbolKind::STRUCT),
                ("Shape", SymbolKind::ENUM),
                ("main", SymbolKind::FUNCTION),
                ("PI", SymbolKind::CONSTANT),
                ("Num", SymbolKind::TYPE_PARAMETER),
            ]
        );
    }

    #[test]
    fn struct_fields_are_children() {
        let src = "struct Point { x: number, y: number }";
        let syms = document_symbols(src);
        let point = &syms[0];
        let fields = point.children.as_ref().expect("fields");
        assert_eq!(
            names(fields),
            vec![("x", SymbolKind::FIELD), ("y", SymbolKind::FIELD)]
        );
    }

    #[test]
    fn enum_variants_are_children() {
        let src = "enum Shape { Circle(number), Origin }";
        let variants = document_symbols(src)[0].children.clone().expect("variants");
        assert_eq!(
            names(&variants),
            vec![
                ("Circle", SymbolKind::ENUM_MEMBER),
                ("Origin", SymbolKind::ENUM_MEMBER)
            ]
        );
    }

    #[test]
    fn impl_methods_are_children() {
        let src = "struct P {}\nimpl P { fn origin() -> P { P {} }\nfn scale(self) {} }\n";
        let syms = document_symbols(src);
        let impl_sym = syms.iter().find(|s| s.name == "impl P").expect("impl");
        let methods = impl_sym.children.as_ref().expect("methods");
        assert_eq!(
            names(methods),
            vec![
                ("origin", SymbolKind::METHOD),
                ("scale", SymbolKind::METHOD)
            ]
        );
    }
}
