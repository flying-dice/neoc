//! AST → typed TypeScript (`.ts`) emitter.
//!
//! A typed `.ts` is the JavaScript runtime with TypeScript type annotations
//! woven onto its declarations (`tasks/ts-interop-inversion.md`). This crate is
//! the *typed consumer* of [`neoc_codegen_js`]'s shared lowering core: the whole
//! expression/statement/`match`/`?`/struct/enum/`Result`/`Option` lowering is
//! emitted by that crate via [`neoc_codegen_js::emit_with_style`] under
//! [`HeaderStyle::Ts`](neoc_codegen_js::HeaderStyle::Ts), which gives free
//! functions and `const`s typed headers (`function f<T>(a: T): R {`,
//! `const MAX: number = …`). This crate adds the rest of what a self-typed `.ts`
//! carries: the data-type declarations (`pub struct` → `interface`, `pub enum` →
//! discriminated-union `type`, `type` alias), projected with the same neoc→TS
//! projector ([`project_struct`](neoc_codegen_js::project_struct) et al.), and
//! the inline `Result`/`Option` type import.
//!
//! Unlike the JS target, a `.ts` is **self-typed**: it carries no sibling
//! `.d.ts` — the types live in the file. `Result`/`Option` and their
//! constructors come from the shared `./neoc-lib` module.
//!
//! The crate is a pure wasm-clean leaf: pure string building over `&ast` nodes,
//! no host facilities.

use std::collections::HashSet;

use neoc_ast as ast;
use neoc_codegen_js::{
    Diagnostic, HeaderStyle, emit_with_style, project_enum, project_struct, project_trait,
    project_type_alias, ts_type,
};

/// The result of emitting a module to TypeScript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitResult {
    /// The emitted, self-typed TypeScript module source (`x.ts`).
    pub ts: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower a module to typed TypeScript (`.ts`) source.
///
/// The parameters mirror [`neoc_codegen_js::emit`] exactly, so the compiler
/// driver calls this the same way it calls the JavaScript backend:
/// `imported_bindings` / `imported_method_types` key which cross-module `use`s
/// carry a runtime binding, `imported_externs` / `imported_enums` merge a
/// `use`d module's extern items and enums, and `importer_dir` anchors relative
/// import specifiers (`tasks/ts-interop-inversion.md`).
///
/// The runtime body is emitted by [`neoc_codegen_js::emit_with_style`] under
/// [`HeaderStyle::Ts`](neoc_codegen_js::HeaderStyle::Ts); this function prepends
/// the inline type declarations and rewrites relative ES-module specifiers to
/// their extensionless TypeScript form (a sibling `./b.js` import becomes
/// `./b`, resolving the sibling `.ts`), making the output self-typed.
#[must_use]
pub fn emit<S: std::hash::BuildHasher>(
    module: &ast::Module,
    imported_bindings: &HashSet<String, S>,
    imported_method_types: &HashSet<String, S>,
    imported_externs: &[ast::ExternItem],
    imported_enums: &[&ast::EnumDecl],
    importer_dir: &[String],
) -> EmitResult {
    // The typed runtime body: the JS backend's lowering, headers annotated.
    let lowered = emit_with_style(
        module,
        imported_bindings,
        imported_method_types,
        imported_externs,
        imported_enums,
        importer_dir,
        HeaderStyle::Ts,
    );
    let body = rewrite_relative_specifiers(&lowered.js);

    // The data-type declarations a self-typed `.ts` carries inline: a `struct`
    // is an `interface`, an `enum` a discriminated-union `type`, a `type` alias
    // a TS `type`. They emit no runtime value (the JS body constructs them as
    // inline tagged objects), so the projector supplies them here. A `pub` item
    // is `export`ed; a module-local one stays unexported but in scope.
    let decls = emit_type_decls(module);

    // The `Result`/`Option` *types* (the runtime body already imports their
    // constructors from `./neoc-lib`). Imported with `import type` so the
    // self-typed `.ts` names them in signatures and union members.
    let lib_import = if references_lib(module) {
        Some(relative_neoc_lib_type_import(importer_dir))
    } else {
        None
    };

    let ts = assemble(lib_import.as_deref(), &decls, &body);
    EmitResult {
        ts,
        diagnostics: lowered.diagnostics,
    }
}

/// Stitch the `Result`/`Option` type import, the type declarations, and the
/// typed runtime body into one module, blank-line separated, each section
/// elided when empty. A trailing newline is kept iff there is any content.
fn assemble(lib_import: Option<&str>, decls: &str, body: &str) -> String {
    let mut sections: Vec<&str> = Vec::new();
    if let Some(imp) = lib_import {
        sections.push(imp);
    }
    if !decls.is_empty() {
        sections.push(decls);
    }
    let body = body.trim_end_matches('\n');
    if !body.is_empty() {
        sections.push(body);
    }
    if sections.is_empty() {
        return String::new();
    }
    let mut s = sections.join("\n\n");
    s.push('\n');
    s
}

/// The inline type declarations for a module's `struct`/`enum`/`type` items,
/// one per line. A `pub` item is `export`ed.
fn emit_type_decls(module: &ast::Module) -> String {
    let mut out: Vec<String> = Vec::new();
    for item in &module.items {
        let exported = item.vis == ast::Visibility::Public;
        match &item.kind {
            ast::ItemKind::Struct(s) => out.push(emit_struct(s, &item.attrs, module, exported)),
            ast::ItemKind::Enum(e) => out.push(emit_enum(e, &item.attrs, module, exported)),
            ast::ItemKind::TypeAlias(a) => out.push(emit_type_alias(a, exported)),
            ast::ItemKind::Trait(t) => out.push(emit_trait(t, exported)),
            _ => {}
        }
    }
    out.join("\n")
}

/// Project a `struct` to its TS `interface`, folding in every method a
/// constructed value can dispatch — those its `#[derive(...)]`s install on the
/// prototype and those its inherent `impl` blocks define. The JS backend casts a
/// method-bearing struct value `as <Type>`, so the interface must carry those
/// members for a `x.display()` / `x.manhattan()` call to type-check. A struct
/// with no methods projects to the bare field interface, unchanged.
fn emit_struct(
    s: &ast::StructDecl,
    attrs: &[ast::Attribute],
    module: &ast::Module,
    exported: bool,
) -> String {
    let base = project_struct(s, exported);
    let mut members = derive_method_members(&s.name, &ast::derive_names(attrs));
    members.extend(inherent_method_members(&s.name, module));
    if members.is_empty() {
        return base;
    }
    // `project_struct` emits `… interface S { f: T; }`; splice the method members
    // in before the closing brace, after the trailing space the projector leaves.
    match base.strip_suffix('}') {
        Some(head) => format!("{head}{} }}", members.join(" ")),
        None => base,
    }
}

/// The interface members a struct's inherent `impl` methods contribute: each
/// method with a `self` receiver becomes `name(params): ret`, so a `x.method()`
/// call on a constructed value type-checks. Associated functions (no receiver)
/// are reached through the type's namespace, not a value, so they are not
/// members. A trait impl's methods are covered by their derive/own contract.
fn inherent_method_members(name: &str, module: &ast::Module) -> Vec<String> {
    let mut members = Vec::new();
    for item in &module.items {
        let ast::ItemKind::Impl(b) = &item.kind else {
            continue;
        };
        let ast::Type::Path(p) = &b.ty else { continue };
        if p.segments.last().map(|s| s.name.as_str()) != Some(name) {
            continue;
        }
        for f in &b.items {
            let has_receiver = f
                .sig
                .params
                .iter()
                .any(|p| matches!(p, ast::Param::Receiver));
            if has_receiver {
                members.push(format!("{};", method_member(&f.sig)));
            }
        }
    }
    members
}

/// One method member of a struct interface: `name(params): ret`, the `self`
/// receiver dropped (implicit on the value), each named param projected via
/// [`ts_type`], the return projected (absent → `void`).
fn method_member(sig: &ast::FnSig) -> String {
    let params = sig
        .params
        .iter()
        .filter_map(|p| match p {
            ast::Param::Receiver => None,
            ast::Param::Named { name, ty, .. } => Some(format!("{name}: {}", ts_type(ty))),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = sig.ret.as_ref().map_or_else(|| "void".to_string(), ts_type);
    format!("{}({params}): {ret}", sig.name)
}

/// The interface members a struct's derives contribute: each derivable trait maps
/// to the method the JS backend attaches to the prototype (`lang/DYNAMIC.md`
/// §6–§7). Signatures mirror the emitted runtime: `clone(): S`, `eq(a: S, b: S):
/// boolean` (the two-operand comparison), `debug(): string`, `display(): string`.
fn derive_method_members(name: &str, derives: &[String]) -> Vec<String> {
    derives
        .iter()
        .filter_map(|d| match d.as_str() {
            "Clone" => Some(format!("clone(): {name};")),
            "Eq" => Some(format!("eq(a: {name}, b: {name}): boolean;")),
            "Debug" => Some("debug(): string;".to_string()),
            "Display" => Some("display(): string;".to_string()),
            _ => None,
        })
        .collect()
}

/// Project an `enum` to its TS discriminated-union `type` declaration, folding in
/// every method a constructed value can dispatch — those its `#[derive(...)]`s
/// install on the prototype and those its inherent `impl` blocks define. The JS
/// backend casts a method-bearing variant value `as <Enum>`, so each union member
/// must carry those members for a `value.opposite()` / `value.display()` call to
/// type-check. This mirrors the struct interface gaining method members and the
/// way `neoc-lib.d.ts` intersects each `Result`/`Option` union member with its
/// `ResultMethods`/`OptionMethods` interface. An enum with no methods projects to
/// the bare discriminated union, unchanged.
fn emit_enum(
    e: &ast::EnumDecl,
    attrs: &[ast::Attribute],
    module: &ast::Module,
    exported: bool,
) -> String {
    let base = project_enum(e, exported);
    let mut members = derive_method_members(&e.name, &ast::derive_names(attrs));
    members.extend(inherent_method_members(&e.name, module));
    if members.is_empty() {
        return base;
    }
    // `project_enum` emits `… type E<…> = m1 | m2 | …;`. Intersect a methods
    // object literal onto the whole union (`(m1 | m2 | …) & { … }`) so every
    // member carries the methods, matching the `neoc-lib.d.ts` pattern while
    // keeping the variant shapes readable.
    let methods = members.join(" ");
    match base.split_once(" = ") {
        Some((head, rhs)) => {
            let rhs = rhs.strip_suffix(';').unwrap_or(rhs);
            format!("{head} = ({rhs}) & {{ {methods} }};")
        }
        None => base,
    }
}

/// Project a `type` alias to its TS `type` declaration.
fn emit_type_alias(a: &ast::TypeAlias, exported: bool) -> String {
    project_type_alias(a, exported)
}

/// Project a `trait` to its structural TS `type` alias — the port contract.
fn emit_trait(t: &ast::TraitDecl, exported: bool) -> String {
    project_trait(t, exported)
}

/// Rewrite every *relative* ES-module specifier in the emitted body from its
/// `.js`-suffixed runtime form to the extensionless TypeScript form: a sibling
/// `import … from "./b.js"` becomes `… from "./b"` (TypeScript resolves the
/// sibling `.ts`), and `"./neoc-lib.js"` becomes `"./neoc-lib"`. A bare
/// (non-relative) specifier — an `extern mod`'s native import like `"fs"` or
/// `"bun:sqlite"` — is left untouched: it names a host package, not a sibling
/// neoc module.
fn rewrite_relative_specifiers(js: &str) -> String {
    js.lines()
        .map(rewrite_specifier_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rewrite one line's relative-specifier `.js` suffix, if it carries one.
fn rewrite_specifier_line(line: &str) -> String {
    // A specifier is the substring between the last pair of double quotes. Only
    // an `import`/`export … from "…"` line carries one we care about; matching
    // on `from "` keeps a string literal inside a body untouched.
    let Some(open) = line.rfind("from \"") else {
        return line.to_string();
    };
    let spec_start = open + "from \"".len();
    let Some(rel_close) = line[spec_start..].find('"') else {
        return line.to_string();
    };
    let close = spec_start + rel_close;
    let spec = &line[spec_start..close];
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return line.to_string();
    }
    let Some(stripped) = spec.strip_suffix(".js") else {
        return line.to_string();
    };
    format!("{}{stripped}{}", &line[..spec_start], &line[close..])
}

/// The `import type { Result, Option } from "<spec>";` line for a self-typed
/// `.ts`, with the extensionless specifier to the shared `neoc-lib` resolved
/// from `importer_dir` (a root-level module → `./neoc-lib`; a subdirectory
/// module walks up with `../`).
fn relative_neoc_lib_type_import(importer_dir: &[String]) -> String {
    let spec = if importer_dir.is_empty() {
        "./neoc-lib".to_string()
    } else {
        format!("{}neoc-lib", "../".repeat(importer_dir.len()))
    };
    format!("import type {{ Result, Option }} from \"{spec}\";")
}

/// Whether the module references `Result` or `Option` anywhere a TS signature or
/// union member would name them — a `const`/`fn` type, a `struct` field, an
/// `enum` variant payload, a `type` alias, or an `impl` method signature. The
/// trigger for the inline `Result`/`Option` type import.
fn references_lib(module: &ast::Module) -> bool {
    module.items.iter().any(item_references_lib)
}

fn item_references_lib(item: &ast::Item) -> bool {
    match &item.kind {
        ast::ItemKind::Fn(f) => sig_references_lib(&f.sig),
        ast::ItemKind::Const(c) => type_references_lib(&c.ty),
        ast::ItemKind::TypeAlias(a) => type_references_lib(&a.value),
        ast::ItemKind::Struct(s) => struct_references_lib(s),
        ast::ItemKind::Enum(e) => e.variants.iter().any(variant_references_lib),
        ast::ItemKind::Impl(b) => b.items.iter().any(|f| sig_references_lib(&f.sig)),
        ast::ItemKind::Trait(t) => t.items.iter().any(|it| sig_references_lib(&it.sig)),
        ast::ItemKind::Use(_) | ast::ItemKind::Extern(_) => false,
    }
}

fn struct_references_lib(s: &ast::StructDecl) -> bool {
    match &s.body {
        ast::StructBody::Unit => false,
        ast::StructBody::Tuple(t) => type_references_lib(t),
        ast::StructBody::Named(fields) => fields.iter().any(|f| type_references_lib(&f.ty)),
    }
}

fn variant_references_lib(v: &ast::Variant) -> bool {
    match &v.body {
        ast::VariantBody::Unit => false,
        ast::VariantBody::Tuple(tys) => tys.iter().any(type_references_lib),
        ast::VariantBody::Named(fields) => fields.iter().any(|f| type_references_lib(&f.ty)),
    }
}

fn sig_references_lib(sig: &ast::FnSig) -> bool {
    let in_params = sig.params.iter().any(|p| match p {
        ast::Param::Receiver => false,
        ast::Param::Named { ty, .. } => type_references_lib(ty),
    });
    in_params || sig.ret.as_ref().is_some_and(type_references_lib)
}

/// Whether a type mentions `Result` or `Option` at any depth.
fn type_references_lib(ty: &ast::Type) -> bool {
    match ty {
        ast::Type::Unit | ast::Type::SelfType => false,
        ast::Type::List(inner) => type_references_lib(inner),
        ast::Type::Fn { params, ret } => {
            params.iter().any(type_references_lib)
                || ret.as_deref().is_some_and(type_references_lib)
        }
        ast::Type::Path(p) => p.segments.iter().any(|seg| {
            matches!(seg.name.as_str(), "Result" | "Option")
                || seg.args.iter().any(type_references_lib)
        }),
    }
}

#[cfg(test)]
mod tests;
