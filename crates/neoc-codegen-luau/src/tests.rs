//! Unit goldens for the Luau backend, one per construct family. Each parses a
//! `.neoc` source through `neoc-parser` and asserts on the emitted `.luau`, so
//! the tests exercise the real lowering core, not a hand-built AST.

use super::*;

/// Emit a lone source: no resolved module graph, no imported enums or externs.
fn emit_solo(src: &str) -> EmitResult {
    let parsed = neoc_parser::parse(src);
    assert!(
        parsed.diagnostics.is_empty(),
        "source did not parse cleanly: {:?}",
        parsed.diagnostics
    );
    emit(&parsed.module, src, &[], &[])
}

#[test]
fn typed_fn_returning_result_carries_param_and_return_types() {
    let out = emit_solo("pub fn find(id: number) -> Result<string, string> {\n    Ok(\"x\")\n}\n");
    assert!(
        out.luau
            .contains("function find(id: number): Result<string, string>"),
        "missing typed fn header:\n{}",
        out.luau
    );
    // A self-typed `.luau` declares the `Result`/`Option` types inline.
    assert!(
        out.luau
            .contains("type Result<T, E> = { __enum: \"Result\", __tag: \"Ok\", [1]: T }"),
        "missing Result type alias:\n{}",
        out.luau
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn exported_struct_becomes_type_alias() {
    let out = emit_solo("pub struct Row { x: number }\n");
    assert!(
        out.luau
            .contains("type Row = { __struct: \"Row\", x: number }"),
        "missing struct type:\n{}",
        out.luau
    );
}

#[test]
fn exported_enum_becomes_union_matching_runtime_tags() {
    let out = emit_solo("pub enum Shape { Circle(number), Square { side: number } }\n");
    assert!(
        out.luau.contains(
            "type Shape = \
             { __enum: \"Shape\", __tag: \"Circle\", [1]: number } | \
             { __enum: \"Shape\", __tag: \"Square\", side: number }"
        ),
        "missing enum union:\n{}",
        out.luau
    );
}

#[test]
fn typed_const_carries_declared_type() {
    let out = emit_solo("pub const MAX: number = 10;\n");
    assert!(
        out.luau.contains("MAX: number = 10"),
        "missing typed const:\n{}",
        out.luau
    );
}

#[test]
fn no_lib_aliases_when_result_unreferenced() {
    let out = emit_solo("pub fn f(x: number) -> number { x }\n");
    assert!(
        !out.luau.contains("type Result<T, E>"),
        "should not declare Result/Option when unreferenced:\n{}",
        out.luau
    );
    // The plain typed header is still present.
    assert!(
        out.luau.contains("function f(x: number): number"),
        "missing typed fn header:\n{}",
        out.luau
    );
}

#[test]
fn list_and_fn_types_use_luau_surface_syntax() {
    let out = emit_solo("pub fn g(xs: [number], f: fn(number) -> number) -> [number] { xs }\n");
    assert!(
        out.luau
            .contains("function g(xs: {number}, f: (number) -> number): {number}"),
        "missing Luau list/fn surface syntax:\n{}",
        out.luau
    );
}
