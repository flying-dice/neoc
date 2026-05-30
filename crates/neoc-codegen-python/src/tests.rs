//! Unit goldens for the Python backend, one per construct family. Each parses a
//! `.neoc` source and asserts on the emitted `.py` / `.pyi`, mirroring
//! `lang/DYNAMIC.md` and the from-scratch idiomatic-Python mapping.

use super::*;

/// Parse `src` and emit Python with no cross-module graph (single-file).
fn emit_src(src: &str) -> EmitResult {
    let parsed = neoc_parser::parse(src);
    assert!(
        parsed.diagnostics.is_empty(),
        "fixture should parse cleanly: {:?}",
        parsed.diagnostics
    );
    emit(&parsed.module, &[], &[], &[])
}

#[test]
fn struct_lowers_to_a_frozen_dataclass_with_float_field() {
    // `number` is a double → `float` (the spec's number→float call).
    let out = emit_src("pub struct Row { id: number }");
    assert!(out.py.contains("@dataclass(frozen=True)"), "{}", out.py);
    assert!(out.py.contains("class Row:"), "{}", out.py);
    assert!(out.py.contains("    id: float"), "{}", out.py);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn enum_lowers_to_a_dataclass_per_variant_plus_union() {
    let out = emit_src("pub enum E { A, B(number), C { x: string } }");
    assert!(out.py.contains("class A:"), "{}", out.py);
    assert!(out.py.contains("class B:"), "{}", out.py);
    assert!(out.py.contains("    _0: float"), "{}", out.py);
    assert!(out.py.contains("class C:"), "{}", out.py);
    assert!(out.py.contains("    x: str"), "{}", out.py);
    assert!(out.py.contains("E = Union[A, B, C]"), "{}", out.py);
}

#[test]
fn fn_returning_result_carries_annotations_and_imports_neoc_lib() {
    let out = emit_src("pub fn find(id: number) -> Result<string, string> { Ok(\"x\") }");
    assert!(
        out.py.contains("def find(id: float) -> Result[str, str]:"),
        "{}",
        out.py
    );
    assert!(
        out.py
            .contains("from .neoc_lib import Result, Option, Ok, Err, Some, None_"),
        "{}",
        out.py
    );
    assert!(out.py.contains("return Ok(\"x\")"), "{}", out.py);
}

#[test]
fn match_lowers_to_native_case_patterns() {
    let src = "pub fn f(o: Option<number>) -> number {\n  match o {\n    Some(x) => x,\n    None => 0,\n  }\n}";
    let out = emit_src(src);
    assert!(out.py.contains("match o:"), "{}", out.py);
    assert!(out.py.contains("case Some(x):"), "{}", out.py);
    assert!(out.py.contains("case None_():"), "{}", out.py);
}

#[test]
fn try_operator_hoists_a_guard_returning_the_carrier() {
    let src = "pub fn f(r: Result<number, string>) -> Result<number, string> {\n  let x = r?;\n  Ok(x)\n}";
    let out = emit_src(src);
    assert!(out.py.contains("isinstance("), "{}", out.py);
    assert!(out.py.contains("(Err, None_)"), "{}", out.py);
}

#[test]
fn method_lands_in_the_dataclass_body() {
    let src = "pub struct Point { x: number, y: number }\nimpl Point { fn sum(self) -> number { self.x + self.y } }";
    let out = emit_src(src);
    assert!(out.py.contains("class Point:"), "{}", out.py);
    assert!(out.py.contains("    def sum(self) -> float:"), "{}", out.py);
    assert!(out.py.contains("return (self.x + self.y)"), "{}", out.py);
}

#[test]
fn pyi_projects_the_typed_surface() {
    let out = emit_src("pub struct Row { id: number }\npub fn one() -> number { 1 }");
    assert!(out.pyi.contains("@dataclass(frozen=True)"), "{}", out.pyi);
    assert!(out.pyi.contains("class Row:"), "{}", out.pyi);
    assert!(out.pyi.contains("    id: float"), "{}", out.pyi);
    assert!(out.pyi.contains("def one() -> float: ..."), "{}", out.pyi);
}

#[test]
fn use_lowers_to_a_relative_import() {
    // The driver records which resolved `use` paths bind a runtime symbol; the
    // emitter imports those and drops the rest (traits, aliases). `add` is a fn,
    // so its resolved path `math::add` is a binding.
    let parsed = neoc_parser::parse("use math::add;\npub fn f() -> number { add(1, 2) }");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let out = emit(&parsed.module, &[], &[], &["math::add".to_string()]);
    assert!(out.py.contains("from .math import add"), "{}", out.py);
}

#[test]
fn use_of_a_non_binding_emits_no_import() {
    // A `use` whose resolved path is absent from the binding set names a trait
    // or alias — no runtime Python symbol — so its import is dropped rather than
    // left to dangle against a module that never defines it.
    let parsed = neoc_parser::parse("use db::SqlExecutor;\npub fn f() -> number { 1 }");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let out = emit(&parsed.module, &[], &[], &[]);
    assert!(
        !out.py.contains("import SqlExecutor"),
        "trait import should be dropped: {}",
        out.py
    );
}

#[test]
fn use_resolves_relative_to_a_subpackage() {
    // From a module in `autosql/`, a `use autosql::ports::SqlValue` resolves to
    // a sibling — `from .ports import …` — not `from .autosql.ports`.
    let parsed = neoc_parser::parse("use autosql::ports::SqlValue;\npub fn f() -> number { 1 }");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let out = emit(
        &parsed.module,
        &[],
        &["autosql".to_string()],
        &["autosql::ports::SqlValue".to_string()],
    );
    assert!(out.py.contains("from .ports import SqlValue"), "{}", out.py);
}

#[test]
fn neoc_lib_unwrap_messages_are_neoc_shaped() {
    let lib = neoc_lib_py();
    assert!(lib.contains("called unwrap on a None value"), "{lib}");
    assert!(lib.contains("called unwrap on an Err value"), "{lib}");
    assert!(lib.contains("called unwrap_err on an Ok value"), "{lib}");
}

#[test]
fn list_and_closure_types_project_idiomatically() {
    let out = emit_src("pub fn f(xs: [number], g: fn(number) -> number) -> number { xs[0] }");
    assert!(
        out.py
            .contains("def f(xs: list[float], g: Callable[[float], float]) -> float:"),
        "{}",
        out.py
    );
}

#[test]
fn debug_derive_lands_on_each_enum_variant() {
    let out = emit_src("#[derive(Debug)]\npub enum Color { Red, Green }");
    assert!(out.py.contains("class Red:"), "{}", out.py);
    assert!(out.py.contains("    def debug(self) -> str:"), "{}", out.py);
    assert!(out.py.contains("return \"Red\""), "{}", out.py);
    assert!(out.py.contains("return \"Green\""), "{}", out.py);
}

#[test]
fn struct_update_uses_dataclasses_replace() {
    let src = "pub struct P { x: number, y: number }\npub fn shift(p: P) -> P { P { x: 9, .. p } }";
    let out = emit_src(src);
    assert!(out.py.contains("replace(p, x=9)"), "{}", out.py);
    assert!(
        out.py
            .contains("from dataclasses import dataclass, replace"),
        "{}",
        out.py
    );
}

#[test]
fn tail_if_without_else_emits_no_return() {
    // A function-body tail that is a unit-valued `if`-without-`else` lowers to a
    // bare `if` statement — there is no value temp, so no `return _ifN` is
    // emitted (that name would be unbound). Regression for the dangling
    // `return _if1` the run-oracle caught on `6-02-derive-eq`.
    let src =
        "extern { fn use_it(x: number); }\npub fn f(c: bool) {\n  if c {\n    use_it(1);\n  }\n}";
    let out = emit_src(src);
    assert!(out.py.contains("def f(c: bool) -> None:"), "{}", out.py);
    assert!(out.py.contains("if c:"), "{}", out.py);
    assert!(
        !out.py.contains("return _if"),
        "a tail if-without-else must not return an unbound hoist temp:\n{}",
        out.py
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn tail_if_else_with_values_still_returns() {
    // A value-valued tail `if`/`else` (both arms a value) DOES produce a value —
    // it must still be returned. Here it collapses to a conditional expression.
    let src = "pub fn pick(c: bool) -> number {\n  if c { 1 } else { 2 }\n}";
    let out = emit_src(src);
    assert!(out.py.contains("return (1 if c else 2)"), "{}", out.py);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn tail_match_for_value_still_returns() {
    // A tail `match` used for its value hoists to a bound temp and is returned.
    let src = "pub fn area(o: Option<number>) -> number {\n  match o {\n    Some(x) => x,\n    None => 0,\n  }\n}";
    let out = emit_src(src);
    assert!(out.py.contains("match o:"), "{}", out.py);
    assert!(out.py.contains("return _m1"), "{}", out.py);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn or_short_circuits_a_hoisting_rhs_match() {
    // `cond || (match …)`: the match hoists statements before its value. Those
    // must NOT run when `cond` is true — so the match lands inside an
    // `if not _sc:` guard, never unconditionally in the function body.
    let src = "pub fn f(cond: bool, o: Option<number>) -> bool {\n  cond || (match o {\n    Some(x) => true,\n    None => false,\n  })\n}";
    let out = emit_src(src);
    // Seed from the lhs, then guard the hoisted rhs behind the lhs being falsey.
    assert!(out.py.contains("_sc1 = cond"), "{}", out.py);
    assert!(out.py.contains("if not _sc1:"), "{}", out.py);
    // The `match` is emitted inside the guard (8-space indent: fn body + guard),
    // never at the unconditional 4-space function-body level.
    assert!(out.py.contains("        match o:"), "{}", out.py);
    assert!(
        !out.py.contains("\n    match o:"),
        "the match must be guarded, not unconditional:\n{}",
        out.py
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn and_short_circuits_a_hoisting_rhs_match() {
    // `cond && (match …)`: the match runs only when `cond` is true — inside an
    // `if _sc:` guard, never unconditionally.
    let src = "pub fn g(cond: bool, o: Option<number>) -> bool {\n  cond && (match o {\n    Some(x) => true,\n    None => false,\n  })\n}";
    let out = emit_src(src);
    assert!(out.py.contains("_sc1 = cond"), "{}", out.py);
    assert!(out.py.contains("if _sc1:"), "{}", out.py);
    assert!(out.py.contains("        match o:"), "{}", out.py);
    assert!(
        !out.py.contains("\n    match o:"),
        "the match must be guarded, not unconditional:\n{}",
        out.py
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}
