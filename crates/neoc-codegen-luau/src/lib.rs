//! AST → typed Luau (`.luau`) emitter.
//!
//! A typed `.luau` is the Lua 5.1 runtime with **native inline Luau type
//! annotations** woven onto its declarations (target #4: luau). This crate is the
//! *typed consumer* of [`neoc_codegen_lua`]'s shared lowering core: the whole
//! expression/statement/`match`/`?`/struct/enum/`Result`/`Option` lowering is
//! emitted by that crate via [`neoc_codegen_lua::emit_with_style`] under
//! [`HeaderStyle::Luau`](neoc_codegen_lua::HeaderStyle::Luau), which gives
//! functions and `const`s typed headers (`function f(a: number): R`,
//! `const MAX: number = …` shaped as a typed binding). This crate adds the rest
//! of what a self-typed `.luau` carries: the data-type declarations (`struct` →
//! `type S = { … }`, `enum` → union `type`, `type` alias), projected with the
//! same neoc→Luau inline-type projector
//! ([`project_luau_struct`](neoc_codegen_lua::project_luau_struct) et al.), and
//! the `Result`/`Option` type aliases.
//!
//! This is the **inline Luau type dialect** — Luau has real first-class types —
//! distinct from the `.d.lua` `---@` `LuaLS` comment sidecar the Lua backend
//! also emits. A `.luau` is **self-typed**: it carries no sidecar, the types live
//! in the file.
//!
//! The crate is a pure wasm-clean leaf: pure string building over `&ast` nodes,
//! no host facilities.

use neoc_ast as ast;
use neoc_codegen_lua::{
    Diagnostic, HeaderStyle, emit_with_style, project_luau_enum, project_luau_struct,
    project_luau_type_alias,
};

/// The result of emitting a module to Luau.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitResult {
    /// The emitted, self-typed Luau module source (`x.luau`).
    pub luau: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower a module to typed Luau (`.luau`) source.
///
/// The parameters mirror [`neoc_codegen_lua::emit_with_imports`] exactly, so the
/// compiler driver calls this the same way it calls the Lua backend: `source` is
/// the originating `.neoc` text, `imported_enums` carries the `pub` enums a `use`
/// brings in (so `match` and variant construction on them lower correctly), and
/// `imported_externs` the extern items a `use` brought in from another file.
///
/// The runtime body is emitted by [`neoc_codegen_lua::emit_with_style`] under
/// [`HeaderStyle::Luau`](neoc_codegen_lua::HeaderStyle::Luau); this function
/// prepends the inline `type` declarations a self-typed `.luau` carries.
#[must_use]
pub fn emit(
    module: &ast::Module,
    source: &str,
    imported_enums: &[&ast::EnumDecl],
    imported_externs: &[ast::ExternItem],
) -> EmitResult {
    // The typed runtime body: the Lua backend's lowering, headers annotated.
    let lowered = emit_with_style(
        module,
        source,
        imported_enums,
        imported_externs,
        HeaderStyle::Luau,
    );
    let body = lowered.lua;

    // The `Result`/`Option` inline type aliases (the runtime body already builds
    // their tagged-table *values* from the inlined prelude). A self-typed `.luau`
    // names them in signatures and union members, so declare their types when the
    // module references them.
    let lib = if references_lib(module) {
        Some(neoc_lib_types())
    } else {
        None
    };

    // The data-type declarations a self-typed `.luau` carries inline: a `struct`
    // is a `type` over a tagged table, an `enum` a union `type`, a `type` alias a
    // Luau `type`. They emit no runtime value (the body constructs them as inline
    // tagged tables), so the projector supplies them here.
    let decls = emit_type_decls(module);

    let luau = assemble(lib.as_deref(), &decls, &body);
    EmitResult {
        luau,
        diagnostics: lowered.diagnostics,
    }
}

/// Stitch the `Result`/`Option` type aliases, the type declarations, and the
/// typed runtime body into one module, blank-line separated, each section elided
/// when empty. A trailing newline is kept iff there is any content.
fn assemble(lib: Option<&str>, decls: &str, body: &str) -> String {
    let mut sections: Vec<&str> = Vec::new();
    if let Some(l) = lib {
        sections.push(l);
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

/// The inline `type` declarations for a module's `struct`/`enum`/`type` items,
/// one per line.
fn emit_type_decls(module: &ast::Module) -> String {
    let mut out: Vec<String> = Vec::new();
    for item in &module.items {
        let exported = item.vis == ast::Visibility::Public;
        match &item.kind {
            ast::ItemKind::Struct(s) => out.push(emit_struct(s, exported)),
            ast::ItemKind::Enum(e) => out.push(emit_enum(e, exported)),
            ast::ItemKind::TypeAlias(a) => out.push(emit_type_alias(a, exported)),
            _ => {}
        }
    }
    out.join("\n")
}

/// Project a `struct` to its Luau `type` declaration.
fn emit_struct(s: &ast::StructDecl, exported: bool) -> String {
    project_luau_struct(s, exported)
}

/// Project an `enum` to its Luau union `type` declaration.
fn emit_enum(e: &ast::EnumDecl, exported: bool) -> String {
    project_luau_enum(e, exported)
}

/// Project a `type` alias to its Luau `type` declaration.
fn emit_type_alias(a: &ast::TypeAlias, exported: bool) -> String {
    project_luau_type_alias(a, exported)
}

/// The `Result`/`Option` inline Luau type aliases, whose union member shapes
/// mirror the runtime tagged tables the inlined prelude builds (`lang/DYNAMIC.md`
/// §7): `Ok`/`Err`/`Some` carry their payload at integer key `[1]`, `None` is the
/// bare tagged table. Prepended to a self-typed `.luau` that references them, so
/// the file names `Result`/`Option` in its signatures and union members.
fn neoc_lib_types() -> String {
    [
        "type Result<T, E> = { __enum: \"Result\", __tag: \"Ok\", [1]: T } | { __enum: \"Result\", __tag: \"Err\", [1]: E }",
        "type Option<T> = { __enum: \"Option\", __tag: \"Some\", [1]: T } | { __enum: \"Option\", __tag: \"None\" }",
    ]
    .join("\n")
}

/// Whether the module references `Result` or `Option` anywhere a Luau signature
/// or union member would name them — a `const`/`fn` type, a `struct` field, an
/// `enum` variant payload, a `type` alias, or an `impl` method signature. The
/// trigger for the inline `Result`/`Option` type aliases.
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
        ast::ItemKind::Use(_) | ast::ItemKind::Extern(_) | ast::ItemKind::Trait(_) => false,
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
