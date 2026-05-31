//! Unit goldens for the TypeScript backend, one per construct family. Each
//! parses a `.neoc` source through `neoc-parser` and asserts on the emitted
//! `.ts`, so the tests exercise the real lowering core, not a hand-built AST.

use std::collections::HashSet;

use super::*;

/// Emit a lone source: no resolved module graph, root-level importer.
fn emit_solo(src: &str) -> EmitResult {
    let parsed = neoc_parser::parse(src);
    assert!(
        parsed.diagnostics.is_empty(),
        "source did not parse cleanly: {:?}",
        parsed.diagnostics
    );
    let empty: HashSet<String> = HashSet::new();
    emit(&parsed.module, &empty, &empty, &[], &[], &[])
}

#[test]
fn typed_fn_returning_result_carries_param_and_return_types() {
    let out = emit_solo(
        "pub fn find(id: number) -> Result<string, string> {\n\
         \x20   Ok(\"x\")\n\
         }\n",
    );
    assert!(
        out.ts
            .contains("export function find(id: number): Result<string, string> {"),
        "missing typed fn header:\n{}",
        out.ts
    );
    // A self-typed `.ts` imports the `Result`/`Option` types inline, plus their
    // constructors from the runtime body's `./neoc-lib` import.
    assert!(
        out.ts
            .contains("import type { Result, Option } from \"./neoc-lib\";"),
        "missing Result/Option type import:\n{}",
        out.ts
    );
    assert!(
        out.ts.contains("from \"./neoc-lib\";") && !out.ts.contains(".js\""),
        "neoc-lib runtime import should be extensionless:\n{}",
        out.ts
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn exported_struct_becomes_interface() {
    let out = emit_solo("pub struct Row { id: number, body: string }\n");
    assert!(
        out.ts
            .contains("export interface Row { id: number; body: string; }"),
        "missing interface:\n{}",
        out.ts
    );
}

#[test]
fn private_struct_interface_is_unexported() {
    let out = emit_solo("struct Row { id: number }\n");
    assert!(
        out.ts.contains("interface Row { id: number; }")
            && !out.ts.contains("export interface Row"),
        "private struct should emit an unexported interface:\n{}",
        out.ts
    );
}

#[test]
fn exported_enum_becomes_discriminated_union() {
    let out = emit_solo("pub enum Shape { Dot, Circle(number), Rect { w: number, h: number } }\n");
    assert!(
        out.ts.contains(
            "export type Shape = \
             { __enum: \"Shape\"; __tag: \"Dot\" } | \
             { __enum: \"Shape\"; __tag: \"Circle\"; _0: number } | \
             { __enum: \"Shape\"; __tag: \"Rect\"; w: number; h: number };"
        ),
        "missing discriminated union:\n{}",
        out.ts
    );
}

#[test]
fn pub_enum_namespace_is_typed_under_strict() {
    // A `.ts` ships no `.d.ts`, so the namespace value carries inline type
    // annotations: typed constructor arrows, a unit value with an `as <Enum>`
    // assertion, and a generic `match` whose last arm is the bare `else` (no
    // `undefined` fallthrough). Mirrors the JS form but typed — passes
    // `tsc --strict`.
    let out = emit_solo(
        "pub enum SqlValue { Int(number), Text(string), Pair { a: number, b: number }, Null }\n",
    );
    assert!(
        out.ts.contains(
            "export const SqlValue = {\n  \
             Int: (_0: number): SqlValue => ({ __enum: \"SqlValue\", __tag: \"Int\", _0 }),\n  \
             Text: (_0: string): SqlValue => ({ __enum: \"SqlValue\", __tag: \"Text\", _0 }),\n  \
             Pair: (fields: { a: number; b: number }): SqlValue => \
             ({ __enum: \"SqlValue\", __tag: \"Pair\", ...fields }),\n  \
             Null: { __enum: \"SqlValue\", __tag: \"Null\" } as SqlValue,\n  \
             match: <R>(value: SqlValue, arms: { Int(_0: number): R; Text(_0: string): R; \
             Pair(fields: { a: number; b: number }): R; Null(): R }): R => \
             value.__tag === \"Int\" ? arms.Int(value._0) : \
             value.__tag === \"Text\" ? arms.Text(value._0) : \
             value.__tag === \"Pair\" ? arms.Pair({ a: value.a, b: value.b }) : \
             arms.Null(),\n\
             };"
        ),
        "missing typed enum namespace:\n{}",
        out.ts
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn enum_with_inherent_method_carries_member_on_its_union() {
    // An `enum` with an inherent `impl` method: the union type folds in the
    // method member (`opposite(): Dir`), the method's `this` is typed, and a
    // constructed variant is widened `as unknown as <Enum>` so `value.opposite()`
    // resolves at the call site (TS does not track `__proto__` dispatch).
    let out = emit_solo(
        "enum Dir { North, South }\n\
         impl Dir {\n\
         \x20   fn opposite(self) -> Dir {\n\
         \x20       match self { Dir::North => Dir::South, Dir::South => Dir::North }\n\
         \x20   }\n\
         }\n\
         pub fn go() -> Dir { let d = Dir::North; d.opposite() }\n",
    );
    assert!(
        out.ts.contains(
            "type Dir = ({ __enum: \"Dir\"; __tag: \"North\" } | \
             { __enum: \"Dir\"; __tag: \"South\" }) & { opposite(): Dir; };"
        ),
        "union missing inherent method member:\n{}",
        out.ts
    );
    assert!(
        out.ts.contains("Dir$.opposite = function(this: Dir) {"),
        "method `this` not typed for enum:\n{}",
        out.ts
    );
    assert!(
        out.ts.contains(
            "let d = { __enum: \"Dir\", __tag: \"North\", __proto__: Dir$ } as unknown as Dir;"
        ),
        "enum variant not widened to its union:\n{}",
        out.ts
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn derived_display_enum_value_type_checks_as_a_method() {
    // A `#[derive(Display)]` enum: the union folds in `display(): string`, and a
    // constructed variant is widened so `value.display()` resolves.
    let out = emit_solo(
        "#[derive(Display)]\n\
         enum Color { Red, Green }\n\
         pub fn name() -> string { let c = Color::Red; c.display() }\n",
    );
    assert!(
        out.ts.contains(
            "type Color = ({ __enum: \"Color\"; __tag: \"Red\" } | \
             { __enum: \"Color\"; __tag: \"Green\" }) & { display(): string; };"
        ),
        "union missing derived display member:\n{}",
        out.ts
    );
    assert!(
        out.ts.contains(
            "let c = { __enum: \"Color\", __tag: \"Red\", __proto__: Color$ } as unknown as Color;"
        ),
        "derived-method enum variant not widened:\n{}",
        out.ts
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn data_only_enum_keeps_its_bare_union() {
    // An enum with no methods or derives projects to the bare discriminated union
    // and its variants take no cast — unchanged behaviour.
    let out = emit_solo(
        "enum E { A, B }\n\
         pub fn pick() -> E { E::A }\n",
    );
    assert!(
        out.ts.contains(
            "type E = { __enum: \"E\"; __tag: \"A\" } | { __enum: \"E\"; __tag: \"B\" };"
        ),
        "data-only enum should keep its bare union:\n{}",
        out.ts
    );
    assert!(
        !out.ts.contains("as unknown as E"),
        "data-only enum variant should take no cast:\n{}",
        out.ts
    );
}

#[test]
fn typed_const_carries_declared_type() {
    let out = emit_solo("pub const MAX: number = 10;\n");
    assert!(
        out.ts.contains("export const MAX: number = 10;"),
        "missing typed const:\n{}",
        out.ts
    );
}

#[test]
fn cross_module_use_imports_a_sibling_ts() {
    // A `use` whose resolved binding is present imports the sibling module by
    // its extensionless specifier (resolving `b.ts`), not `./b.js`.
    let parsed = neoc_parser::parse("use a::b::helper;\npub fn f() -> number { helper() }\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let mut bindings: HashSet<String> = HashSet::new();
    bindings.insert("a::b::helper".to_string());
    let empty: HashSet<String> = HashSet::new();
    let out = emit(&parsed.module, &bindings, &empty, &[], &[], &[]);
    assert!(
        out.ts.contains("import { helper } from \"./a/b\";"),
        "expected an extensionless sibling import:\n{}",
        out.ts
    );
    assert!(
        !out.ts.contains(".js\""),
        "no relative specifier should keep its .js suffix:\n{}",
        out.ts
    );
}

#[test]
fn no_lib_import_when_result_unreferenced() {
    let out = emit_solo("pub fn f(x: number) -> number { x }\n");
    assert!(
        !out.ts.contains("import type { Result, Option }"),
        "should not import Result/Option when unreferenced:\n{}",
        out.ts
    );
}

#[test]
fn generic_bound_carries_as_extends_in_fn_header() {
    // A `<E: SqlExecutor>` bound on a fn header is carried as a TS `extends` so
    // `db.query`/`db.execute` resolve against the port (else `E` is unconstrained
    // and the methods error). The bound's trait is in scope via the `use`d type.
    let out = emit_solo(
        "use a::ports::SqlExecutor;\n\
         pub fn run<E: SqlExecutor>(db: E) -> number { 0 }\n",
    );
    assert!(
        out.ts
            .contains("export function run<E extends SqlExecutor>(db: E): number {"),
        "missing carried bound:\n{}",
        out.ts
    );
}

#[test]
fn multi_bound_becomes_intersection_in_fn_header() {
    let out = emit_solo(
        "use a::t::A;\nuse a::t::B;\n\
         pub fn f<T: A + B>(x: T) -> number { 0 }\n",
    );
    assert!(
        out.ts
            .contains("export function f<T extends A & B>(x: T): number {"),
        "missing intersection bound:\n{}",
        out.ts
    );
}

#[test]
fn type_only_use_emits_an_import_type() {
    // A `use`d data-only type used in type position only (here as a bound's trait
    // and a list element) is erased in JS but must be imported in a self-typed
    // `.ts`, as `import type` — extensionless sibling specifier.
    let out = emit_solo(
        "use a::ports::SqlValue;\n\
         pub fn decode(cols: [SqlValue]) -> number { 0 }\n",
    );
    assert!(
        out.ts
            .contains("import type { SqlValue } from \"./a/ports\";"),
        "missing type-only import:\n{}",
        out.ts
    );
}

#[test]
fn derived_display_struct_value_type_checks_as_a_method() {
    // A `#[derive(Display)]` struct: the interface carries `display(): string`,
    // the prototype is typed `Record<string, Function>`, the method's `this` is
    // typed, and a constructed value is widened `as <Type>` so `x.display()`
    // resolves (TS does not track `__proto__` dispatch).
    let out = emit_solo(
        "#[derive(Display)]\n\
         struct NoteLine { id: number, body: string }\n\
         pub fn render() -> string {\n\
         \x20   let line = NoteLine { id: 1, body: \"b\" };\n\
         \x20   line.display()\n\
         }\n",
    );
    assert!(
        out.ts
            .contains("interface NoteLine { id: number; body: string; display(): string; }"),
        "interface missing display member:\n{}",
        out.ts
    );
    assert!(
        out.ts
            .contains("const NoteLine$: Record<string, Function> = {};"),
        "prototype not typed:\n{}",
        out.ts
    );
    assert!(
        out.ts
            .contains("NoteLine$.display = function(this: NoteLine) {"),
        "derived method `this` not typed:\n{}",
        out.ts
    );
    assert!(
        out.ts.contains("as unknown as NoteLine"),
        "struct value not widened to its interface:\n{}",
        out.ts
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn try_short_circuits_on_the_success_tag_and_casts_the_return() {
    // Under TS, `?` tests the enclosing carrier's success tag negated (`!== "Ok"`
    // for a `Result`-returning fn) — TS narrows it cleanly — and casts the early
    // return to the return type. The dual-tag JS form would trip TS2367.
    let out = emit_solo(
        "use a::p::find;\n\
         pub fn run() -> Result<number, string> {\n\
         \x20   let x = find()?;\n\
         \x20   Ok(x)\n\
         }\n",
    );
    assert!(
        out.ts.contains("if (__try_0.__tag !== \"Ok\")"),
        "try should test the success tag negated:\n{}",
        out.ts
    );
    assert!(
        out.ts
            .contains("return __try_0 as unknown as Result<number, string>;"),
        "try early-return should cast to the carrier:\n{}",
        out.ts
    );
    assert!(
        !out.ts.contains("=== \"None\""),
        "no non-carrier tag should be tested under TS:\n{}",
        out.ts
    );
}

#[test]
fn exhaustive_match_last_arm_returns_unconditionally() {
    // The final unguarded arm of an exhaustive value-position `match` is emitted
    // as an unconditional `return` so TS sees a definite return (else the IIFE
    // widens to `T | undefined`).
    let out = emit_solo(
        "pub enum E { A, B }\n\
         pub fn pick(e: E) -> number {\n\
         \x20   match e {\n\
         \x20       E::A => 1,\n\
         \x20       E::B => 2,\n\
         \x20   }\n\
         }\n",
    );
    // The last arm has no gating `if`; the body's `return 2;` is at the IIFE's
    // top level.
    assert!(
        out.ts.contains("return 2;"),
        "last arm should return:\n{}",
        out.ts
    );
    // The first arm keeps its tag test.
    assert!(
        out.ts.contains("if (e.__tag === \"A\")") || out.ts.contains("__s1.__tag === \"A\""),
        "first arm should keep its guard:\n{}",
        out.ts
    );
}

#[test]
fn typed_empty_let_carries_its_element_type() {
    // An empty-array `let` with a declared type carries that type so TS does not
    // infer `any[]` under `noImplicitAny`.
    let out = emit_solo(
        "use a::ports::SqlValue;\n\
         pub fn f() -> number {\n\
         \x20   let params: [SqlValue] = [];\n\
         \x20   0\n\
         }\n",
    );
    assert!(
        out.ts.contains("let params: SqlValue[] = [];"),
        "empty let should carry its element type:\n{}",
        out.ts
    );
}

#[test]
fn list_index_gets_a_non_null_assertion() {
    // Under TS with `noUncheckedIndexedAccess`, a list index widens to `T |
    // undefined`; neoc's checked index restores the element type with `!`.
    let out = emit_solo("pub fn first(xs: [number]) -> number { xs[0] }\n");
    assert!(
        out.ts.contains("return xs[0]!;"),
        "list index should carry a non-null assertion:\n{}",
        out.ts
    );
}

#[test]
fn extern_mod_import_keeps_its_host_specifier() {
    // A non-relative specifier (a host package) is not rewritten.
    let out = emit_solo(
        "extern {\n\
         \x20   mod fs {\n\
         \x20       fn read(path: string) -> string;\n\
         \x20   }\n\
         }\n\
         pub fn cfg() -> string { fs::read(\"c\") }\n",
    );
    assert!(
        out.ts.contains("from \"fs\";"),
        "host specifier should be preserved:\n{}",
        out.ts
    );
}
