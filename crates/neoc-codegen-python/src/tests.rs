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
    emit(&parsed.module, &[])
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
    let out = emit_src("use math::add;\npub fn f() -> number { add(1, 2) }");
    assert!(out.py.contains("from .math import add"), "{}", out.py);
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
