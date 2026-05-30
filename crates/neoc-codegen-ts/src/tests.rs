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
