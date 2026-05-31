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
fn same_module_reserved_name_call_references_the_escaped_def() {
    // A top-level `fn list` is defined escaped (`def list_`), so a call site in
    // the same module must reference `list_`, not the Python builtin `list`.
    let out =
        emit_src("pub fn list(n: number) -> number { n + 1 }\npub fn run() -> number { list(41) }");
    assert!(
        out.py.contains("def list_(n: float) -> float:"),
        "{}",
        out.py
    );
    assert!(out.py.contains("return list_(41)"), "{}", out.py);
    assert!(
        !out.py.contains("return list(41)"),
        "the call must name the escaped def, not the builtin:\n{}",
        out.py
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn cross_module_reserved_name_call_references_the_escaped_import() {
    // `use other::list;` imports `list_`; a call must reference `list_` so it
    // binds the import, not the builtin. Regression for the call-site gap that
    // left `list(41)` referencing the wrong/undefined symbol.
    let parsed = neoc_parser::parse("use other::list;\npub fn run() -> number { list(41) }");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let out = emit(&parsed.module, &[], &[], &["other::list".to_string()]);
    assert!(out.py.contains("from .other import list_"), "{}", out.py);
    assert!(out.py.contains("return list_(41)"), "{}", out.py);
    assert!(
        !out.py.contains("return list(41)"),
        "the call must name the escaped import, not the builtin:\n{}",
        out.py
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn local_spelled_like_a_reserved_word_is_emitted_verbatim() {
    // A **local** binding is never escaped (only module-level/imported names
    // are). A local spelled `list` shadows nothing at module scope, so both its
    // binding and its reference emit verbatim — `list`, not `list_`.
    let out = emit_src("pub fn run() -> number { let list = 41; list + 1 }");
    assert!(out.py.contains("list = 41"), "{}", out.py);
    assert!(out.py.contains("(list + 1)"), "{}", out.py);
    assert!(
        !out.py.contains("list_"),
        "a local must not be escaped:\n{}",
        out.py
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
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

#[test]
fn generic_fn_param_declares_a_module_typevar() {
    // `fn init<E: SqlExecutor>(db: E)` uses `E` in its signature; `E` must be a
    // declared `TypeVar` or pyright sees an undefined name.
    let src = "extern { fn run(db: number); }\npub fn init<E>(db: E) { run(1) }";
    let out = emit_src(src);
    assert!(out.py.contains("E = TypeVar(\"E\")"), "{}", out.py);
    assert!(out.py.contains("def init(db: E)"), "{}", out.py);
    assert!(out.pyi.contains("E = TypeVar(\"E\")"), "{}", out.pyi);
    assert!(out.pyi.contains("def init(db: E)"), "{}", out.pyi);
}

#[test]
fn result_alias_is_generic_so_signatures_may_subscript_it() {
    // `Result = Union[Ok[T], Err[E]]` (generic alias) lets `Result[U, object]`
    // re-subscript without pyright's "already specialized".
    assert!(
        crate::neoc_lib_py().contains("Result = Union[Ok[T], Err[E]]"),
        "{}",
        crate::neoc_lib_py()
    );
    assert!(
        crate::neoc_lib_py().contains("Option = Union[Some[T], None_]"),
        "{}",
        crate::neoc_lib_py()
    );
    assert!(
        crate::neoc_lib_pyi().contains("Result = Union[Ok[T], Err[E]]"),
        "{}",
        crate::neoc_lib_pyi()
    );
}

#[test]
fn trait_lowers_to_a_protocol_and_bounds_substitute() {
    // A `trait` is a `Protocol`; a `fn` param bounded by it projects to the
    // protocol (no `TypeVar`), so a method call through the bound type-checks.
    let src = "pub trait Db {\n  fn run(self, sql: string) -> number;\n}\npub fn go<D: Db>(db: D) -> number { db.run(\"x\") }";
    let out = emit_src(src);
    assert!(out.py.contains("class Db(Protocol):"), "{}", out.py);
    assert!(out.py.contains("Protocol"), "py import:\n{}", out.py);
    assert!(out.py.contains("def go(db: Db) -> float:"), "{}", out.py);
    assert!(!out.py.contains("def go(db: D)"), "{}", out.py);
    assert!(out.pyi.contains("class Db(Protocol):"), "{}", out.pyi);
    assert!(
        out.pyi.contains("def go(db: Db) -> float: ..."),
        "{}",
        out.pyi
    );
    // The single-bound param must not leak a `TypeVar` declaration.
    assert!(!out.py.contains("D = TypeVar(\"D\")"), "{}", out.py);
}
