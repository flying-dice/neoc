//! Hover: render a markdown signature for the item under the cursor.
//!
//! Resolution maps the identifier under the cursor (CST) to a declaration in
//! the symbol table built by `neoc-types`, then renders a neoc-flavoured
//! signature: `fn fib(n: number) -> number`, `struct Point { x: number }`,
//! enum variants, consts. Best-effort for locals; returns `None` when the name
//! is not a known item so the editor shows nothing.

use tree_sitter::Node;

use crate::cst::{self};
use crate::position::position_to_byte;
use crate::signature;

/// The hover markdown for the identifier at `position`, if it resolves to a
/// known declaration. Each result is a signature code block, optionally
/// followed by the declaration's `///` doc comment (like rust-analyzer).
#[must_use]
pub fn hover(source: &str, position: tower_lsp::lsp_types::Position) -> Option<String> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);

    // 1. The cursor sits on a member's own declaration name (field / variant /
    //    method) — render it straight from the CST node, unambiguously.
    if let Some(node) = cst::member_decl_at(&tree, byte) {
        let sig = member_signature(node, source)?;
        return Some(with_doc(
            code_block(&sig),
            cst::leading_doc_comment(node, source),
        ));
    }

    let name = cst::identifier_at(&tree, source, byte)?;
    let decl = cst::find_decl(&tree, source, name);

    // 2. The cursor sits on a top-level function's own declaration name. Render
    //    the written signature from the CST so generic parameters and parameter
    //    names survive verbatim (`fn first<T>(a: T, b: T) -> T`) — detail the
    //    type-table rendering below (used at call sites) cannot reconstruct.
    if let Some(decl) = &decl
        && decl.node.kind() == "function_declaration"
        && decl.name_node.byte_range().contains(&byte)
    {
        let sig = function_signature(decl.node, source)?;
        return Some(with_doc(
            code_block(&sig),
            cst::leading_doc_comment(decl.node, source),
        ));
    }

    let table = neoc_types::collect(&neoc_parser::parse(source).module);

    // 3. A top-level item resolved by name (a call site, or a type / const /
    //    alias). Drawn from the symbol table; functions render as their type,
    //    `fn name(types) -> ret`.
    if let Some(sig) = signature::signature(&table, name) {
        let doc = decl.and_then(|decl| cst::leading_doc_comment(decl.node, source));
        return Some(with_doc(code_block(&sig), doc));
    }

    // 4. A member used at a call / field / `Type::Variant` site, resolved by
    //    unique name across the file (silent when ambiguous).
    if let Some(node) = cst::find_unique_member(&tree, source, name) {
        let sig = member_signature(node, source)?;
        return Some(with_doc(
            code_block(&sig),
            cst::leading_doc_comment(node, source),
        ));
    }

    // 5. A local binding or sub-expression with no declaration to resolve —
    //    show the inferred type of the narrowest expression under the cursor,
    //    `name: ty` for an identifier, the bare type otherwise.
    let ty = inferred_type_at(source, byte)?;
    let label = format!("{name}: {ty}");
    Some(code_block(&label))
}

/// The neoc type inferred for the narrowest expression covering `byte`, if any.
fn inferred_type_at(source: &str, byte: usize) -> Option<String> {
    neoc_types::expression_types(&neoc_parser::parse(source).module)
        .into_iter()
        .filter(|t| t.start <= byte && byte < t.end)
        .min_by_key(|t| t.end - t.start)
        .map(|t| t.ty.to_string())
}

/// Appends a `///` doc comment below a signature block, if present.
fn with_doc(mut markdown: String, doc: Option<String>) -> String {
    if let Some(doc) = doc {
        markdown.push_str("\n\n");
        markdown.push_str(&doc);
    }
    markdown
}

/// A one-line signature for a member declaration node, read from the CST.
///
/// A field or variant renders as its own source (`x: number`, `Circle(number)`);
/// a method renders as its signature without the body (`fn go(self) -> number`).
fn member_signature(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "field_declaration" | "variant" => Some(source.get(node.byte_range())?.trim().to_string()),
        "function_declaration" => function_signature(node, source),
        _ => None,
    }
}

/// The written signature of a `function_declaration`, read straight from the
/// CST: `fn name<generics>(params) -> ret`. Generic parameters and parameter
/// names survive verbatim — the type-table rendering ([`value_signature`])
/// keeps neither.
fn function_signature(node: Node, source: &str) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    let params = node.child_by_field_name("parameters")?;
    let generics = node
        .child_by_field_name("generics")
        .and_then(|g| source.get(g.byte_range()))
        .unwrap_or_default();
    let mut sig = format!(
        "fn {}{generics}{}",
        source.get(name.byte_range())?,
        source.get(params.byte_range())?,
    );
    if let Some(ret) = node.child_by_field_name("return_type") {
        sig.push_str(" -> ");
        sig.push_str(source.get(ret.byte_range())?);
    }
    Some(sig)
}

fn code_block(sig: &str) -> String {
    format!("```neoc\n{sig}\n```")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    /// Byte index of the 1-based `occ`-th occurrence of `needle`.
    fn nth(src: &str, needle: &str, occ: usize) -> usize {
        let mut from = 0usize;
        let mut idx = 0usize;
        for _ in 0..occ {
            let i = src[from..].find(needle).expect("occurrence") + from;
            idx = i;
            from = i + 1;
        }
        idx
    }

    fn hover_on(src: &str, needle: &str, occ: usize) -> Option<String> {
        let idx = nth(src, needle, occ);
        hover(src, byte_to_position(src, idx + 1))
    }

    #[test]
    fn fn_signature() {
        let src = "fn fib(n: number) -> number { fib(n) }\n";
        let h = hover_on(src, "fib", 2).expect("hover");
        assert!(h.contains("fn fib(number) -> number"), "{h}");
    }

    #[test]
    fn fn_declaration_shows_generics_and_param_names() {
        // On the declaration name, hover renders the written signature from the
        // CST: generic parameters (`<T>`) and parameter names (`a`, `b`) survive,
        // unlike the call-site form `fn first(T, T) -> T`.
        let src = "fn first<T>(a: T, b: T) -> T { a }\n";
        let h = hover_on(src, "first", 1).expect("hover at declaration");
        assert!(h.contains("fn first<T>(a: T, b: T) -> T"), "{h}");
    }

    #[test]
    fn struct_signature_renders_fields() {
        let src =
            "struct Point { x: number, y: number }\nfn m() -> Point { Point { x: 1, y: 2 } }\n";
        let h = hover_on(src, "Point", 2).expect("hover");
        assert!(h.contains("struct Point { x: number, y: number }"), "{h}");
    }

    #[test]
    fn enum_signature_renders_variants() {
        let src = "enum Shape { Circle(number), Origin }\nfn m() -> Shape { Origin }\n";
        // hover on the type name in the return position.
        let h = hover_on(src, "Shape", 2).expect("hover");
        assert!(h.contains("Circle(number)") && h.contains("Origin"), "{h}");
    }

    #[test]
    fn const_signature() {
        let src = "const PI: number = 3;\nfn m() -> number { PI }\n";
        let h = hover_on(src, "PI", 2).expect("hover");
        assert!(h.contains("PI: number"), "{h}");
    }

    #[test]
    fn local_binding_hover_shows_inferred_type() {
        // A local has no declaration to resolve, so hover falls back to the
        // inferred type of the expression under the cursor.
        let src = "fn m() -> number { let count = 1; count }\n";
        let h = hover_on(src, "count", 2).expect("hover on local use");
        assert!(h.contains("count: number"), "{h}");
    }

    #[test]
    fn unknown_name_outside_any_expression_is_none() {
        // The `fn` keyword is not an expression and names nothing.
        let src = "fn m() -> number { 1 }\n";
        assert_eq!(hover(src, byte_to_position(src, 0)), None);
    }

    #[test]
    fn doc_comment_appears_below_the_signature() {
        let src = "\
/// A point in the plane.
/// Two coordinates.
struct Point { x: number, y: number }
fn m() -> Point { Point { x: 1, y: 2 } }
";
        let h = hover_on(src, "Point", 2).expect("hover");
        assert!(h.contains("struct Point { x: number, y: number }"), "{h}");
        assert!(h.contains("A point in the plane."), "{h}");
        assert!(h.contains("Two coordinates."), "{h}");
    }

    #[test]
    fn method_doc_at_declaration_and_call_site() {
        let src = "\
struct S { x: number }
impl S {
  /// computes the thing
  fn go(self) -> number { self.x }
}
fn m(s: S) -> number { s.go() }
";
        // On the method's own declaration name.
        let at_decl = hover_on(src, "go", 1).expect("hover at decl");
        assert!(at_decl.contains("fn go(self) -> number"), "{at_decl}");
        assert!(at_decl.contains("computes the thing"), "{at_decl}");
        // On the call site `s.go()`.
        let at_call = hover_on(src, "go", 2).expect("hover at call");
        assert!(at_call.contains("computes the thing"), "{at_call}");
    }

    #[test]
    fn variant_doc_is_shown() {
        let src = "\
enum Shape {
  /// a round one
  Circle(number),
  Origin,
}
fn m() -> Shape { Origin }
";
        let h = hover_on(src, "Circle", 1).expect("hover");
        assert!(h.contains("Circle(number)"), "{h}");
        assert!(h.contains("a round one"), "{h}");
    }

    #[test]
    fn field_doc_is_shown() {
        let src = "\
struct Box {
  /// the horizontal size
  width: number,
  height: number,
}
fn m(b: Box) -> number { b.width }
";
        // On the field declaration.
        let at_decl = hover_on(src, "width", 1).expect("hover at field decl");
        assert!(at_decl.contains("width: number"), "{at_decl}");
        assert!(at_decl.contains("the horizontal size"), "{at_decl}");
        // On the usage `b.width`, resolved by unique member name.
        let at_use = hover_on(src, "width", 2).expect("hover at use");
        assert!(at_use.contains("the horizontal size"), "{at_use}");
    }

    #[test]
    fn ordinary_comment_is_not_documentation() {
        // A plain `//` note above an item must not surface as hover docs.
        let src = "\
// internal helper, do not call directly
fn helper() -> number { 1 }
fn m() -> number { helper() }
";
        let h = hover_on(src, "helper", 2).expect("hover");
        assert!(h.contains("fn helper() -> number"), "{h}");
        assert!(!h.contains("internal helper"), "{h}");
    }
}
