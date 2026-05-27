//! `expression_types` records the inferred type of every expression with its
//! byte span, the basis for tooling position→type queries.

use neoc_types::{Ty, expression_types};

/// The type of the narrowest recorded expression whose span covers `byte`.
fn type_at(src: &str, byte: usize) -> Option<Ty> {
    let parsed = neoc_parser::parse(src);
    let types = expression_types(&parsed.module);
    types
        .into_iter()
        .filter(|t| t.start <= byte && byte < t.end)
        .min_by_key(|t| t.end - t.start)
        .map(|t| t.ty)
}

#[test]
fn infers_a_local_binding_use() {
    let src = "fn f() -> number { let x = 1; x }\n";
    // The use of `x` is its last occurrence.
    let at = src.rfind('x').unwrap();
    assert_eq!(type_at(src, at), Some(Ty::Number));
}

#[test]
fn infers_a_string_binding_use() {
    let src = "fn f() -> string { let s = \"hi\"; s }\n";
    let at = src.rfind('s').unwrap();
    assert_eq!(type_at(src, at), Some(Ty::Str));
}

#[test]
fn infers_a_literal() {
    let src = "fn f() -> number { 42 }\n";
    let at = src.find("42").unwrap();
    assert_eq!(type_at(src, at), Some(Ty::Number));
}

#[test]
fn narrowest_span_wins_over_an_enclosing_expression() {
    // At the `1`, the narrowest expression is the literal, not the `1 + 2` sum —
    // both are numbers here, but the literal's span is the tighter match.
    let src = "fn f() -> number { 1 + 2 }\n";
    let at = src.find('1').unwrap();
    assert_eq!(type_at(src, at), Some(Ty::Number));
}

#[test]
fn no_expression_at_a_position_is_none() {
    let src = "fn f() -> number { 1 }\n";
    // Byte 0 is the `fn` keyword — no expression there.
    assert_eq!(type_at(src, 0), None);
}
