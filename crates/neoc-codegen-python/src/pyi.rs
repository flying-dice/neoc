//! neoc → Python type projection and the `.pyi` stub.
//!
//! Two jobs live here:
//!
//! 1. [`py_type`] projects a neoc [`ast::Type`] to its idiomatic Python 3.10+
//!    annotation — `number` → `float`, `string` → `str`, `bool` → `bool`,
//!    `()` → `None`, `[T]` → `list[T]`, `fn(A) -> B` → `Callable[[A], B]`,
//!    `Result`/`Option`/user types by name. The same projection feeds the
//!    inline annotations in the emitted `.py` (dataclass fields, `def`
//!    signatures) and the typed surface in the `.pyi`.
//! 2. [`module_pyi`] projects a module to its `.pyi` stub: a `@dataclass`
//!    header per `struct`/enum variant, a `def … -> R: ...` per `pub` fn, a
//!    typed name per `pub const`, and the union alias for an enum — the typed
//!    surface a consumer (or a type checker) reads, with no bodies.
//!
//! Pure string building over `&ast` nodes; no host facilities, wasm-clean.

use neoc_ast as ast;

/// Project a neoc type to its idiomatic Python annotation.
///
/// `number` is a double, so it maps to `float`; `string` → `str`; `bool` →
/// `bool`; `()` (unit) → `None`; `[T]` → `list[T]`; `fn(A, …) -> B` →
/// `Callable[[A, …], B]`. A `Result`/`Option`/user type keeps its name, its
/// generic arguments projected in turn. A bare type parameter keeps its name (a
/// `TypeVar` declared at the use site).
#[must_use]
pub fn py_type(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Unit => "None".to_string(),
        ast::Type::SelfType => "Self".to_string(),
        ast::Type::List(inner) => format!("list[{}]", py_type(inner)),
        ast::Type::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(py_type).collect();
            let r = ret.as_deref().map_or_else(|| "None".to_string(), py_type);
            format!("Callable[[{}], {r}]", ps.join(", "))
        }
        ast::Type::Path(p) => py_type_path(p),
    }
}

/// Project a type path, mapping each primitive name and recursing into generic
/// arguments. A multi-segment path keeps only its final segment (the type name);
/// module qualification is erased — Python imports bring the name into scope.
fn py_type_path(p: &ast::TypePath) -> String {
    let Some(seg) = p.segments.last() else {
        return "object".to_string();
    };
    let name = match seg.name.as_str() {
        "number" => "float",
        "string" => "str",
        "bool" => "bool",
        other => other,
    };
    if seg.args.is_empty() {
        name.to_string()
    } else {
        let args: Vec<String> = seg.args.iter().map(py_type).collect();
        format!("{name}[{}]", args.join(", "))
    }
}

/// The Python annotation for a function's return type — its declared type, or
/// `None` when the function returns unit.
fn ret_type(ret: Option<&ast::Type>) -> String {
    ret.map_or_else(|| "None".to_string(), py_type)
}

/// Render a function signature's parameter list as Python `name: T` entries.
/// A `self` receiver is kept verbatim (untyped, as Python methods take it).
fn params(sig: &ast::FnSig) -> Vec<String> {
    sig.params
        .iter()
        .map(|p| match p {
            ast::Param::Receiver => "self".to_string(),
            ast::Param::Named { name, ty, .. } => format!("{name}: {}", py_type(ty)),
        })
        .collect()
}

/// Whether a signature has a `self` receiver (it is a method, not a free fn).
fn has_receiver(sig: &ast::FnSig) -> bool {
    sig.params.iter().any(|p| matches!(p, ast::Param::Receiver))
}

/// Project a free function's `def … -> R: ...` stub line for a `.pyi`.
#[must_use]
pub fn project_fn(sig: &ast::FnSig) -> String {
    format!(
        "def {}({}) -> {}: ...",
        sig.name,
        params(sig).join(", "),
        ret_type(sig.ret.as_ref())
    )
}

/// Project a method's `def … -> R: ...` stub line (indented under a class in a
/// `.pyi`). Identical to [`project_fn`] but kept distinct for intent.
fn project_method(sig: &ast::FnSig) -> String {
    project_fn(sig)
}

/// The `.pyi` stub for a module: the typed surface a consumer reads.
///
/// Emits, in source order: a `from __future__`/typing import preamble when
/// referenced, a `@dataclass` class stub per `struct` (fields typed, methods
/// from any `impl`), one `@dataclass` per enum variant plus the union alias, a
/// `def … -> R: ...` per `pub` fn, and a typed name per `pub const`. Private
/// items are still surfaced in the stub (Python has no enforced visibility); the
/// `__all__` controls the public surface in the runtime `.py`.
#[must_use]
pub fn module_pyi(module: &ast::Module) -> String {
    let mut imports: Vec<String> = vec!["from dataclasses import dataclass".to_string()];
    let mut typing: Vec<&str> = Vec::new();
    let mut needs_lib = false;
    let mut needs_self = false;
    let mut body: Vec<String> = Vec::new();

    // Method blocks keyed by their target type so a struct/enum stub carries its
    // methods. Collected first so the type stubs can fold them in.
    let methods = method_index(module);

    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Struct(s) => {
                needs_lib |= struct_refs_lib(s);
                needs_self |= struct_refs_self(s);
                body.push(struct_pyi(
                    s,
                    methods.get(s.name.as_str()).map(Vec::as_slice),
                ));
                body.push(String::new());
            }
            ast::ItemKind::Enum(e) => {
                needs_lib |= e.variants.iter().any(variant_refs_lib);
                typing.push("Union");
                body.extend(enum_pyi(e, methods.get(e.name.as_str()).map(Vec::as_slice)));
                body.push(String::new());
            }
            ast::ItemKind::Fn(f) if item.vis == ast::Visibility::Public => {
                needs_lib |= sig_refs_lib(&f.sig);
                if sig_refs_callable(&f.sig) {
                    typing.push("Callable");
                }
                body.push(project_fn(&f.sig));
            }
            ast::ItemKind::Const(c) if item.vis == ast::Visibility::Public => {
                needs_lib |= type_refs_lib(&c.ty);
                body.push(format!("{}: {}", c.name, py_type(&c.ty)));
            }
            _ => {}
        }
    }

    if needs_lib {
        imports.push("from .neoc_lib import Result, Option, Ok, Err, Some, None_".to_string());
    }
    if needs_self {
        typing.push("Self");
    }
    typing.sort_unstable();
    typing.dedup();
    if !typing.is_empty() {
        imports.push(format!("from typing import {}", typing.join(", ")));
    }

    // Trim trailing blank lines off the body.
    while body.last().is_some_and(String::is_empty) {
        body.pop();
    }
    if body.is_empty() {
        return String::new();
    }

    let mut out = imports;
    out.push(String::new());
    out.extend(body);
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// The `@dataclass(frozen=True)` stub for a struct, with typed field
/// declarations and any method signatures.
fn struct_pyi(s: &ast::StructDecl, methods: Option<&[ast::FnSig]>) -> String {
    let mut lines = vec![
        "@dataclass(frozen=True)".to_string(),
        class_header(&s.name, &s.generics),
    ];
    match &s.body {
        ast::StructBody::Unit => {}
        ast::StructBody::Tuple(t) => lines.push(format!("    _0: {}", py_type(t))),
        ast::StructBody::Named(fields) => {
            for f in fields {
                lines.push(format!("    {}: {}", f.name, py_type(&f.ty)));
            }
        }
    }
    if let Some(ms) = methods {
        for sig in ms {
            lines.push(format!("    {}", project_method(sig)));
        }
    }
    if lines.len() == 2 {
        // No fields, no methods: a bare class body needs `...`.
        lines.push("    ...".to_string());
    }
    lines.join("\n")
}

/// The `.pyi` stubs for an enum: a `@dataclass` per variant followed by the
/// union alias `E = Union[A, B, …]`. Methods land on every variant class (each
/// is matchable and carries the shared behaviour).
fn enum_pyi(e: &ast::EnumDecl, methods: Option<&[ast::FnSig]>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut variant_names: Vec<String> = Vec::new();
    for v in &e.variants {
        let cls = variant_class_name(&v.name);
        variant_names.push(cls.clone());
        out.push("@dataclass(frozen=True)".to_string());
        out.push(format!("class {cls}:"));
        let mut had_field = false;
        match &v.body {
            ast::VariantBody::Unit => {}
            ast::VariantBody::Tuple(tys) => {
                for (i, t) in tys.iter().enumerate() {
                    out.push(format!("    _{i}: {}", py_type(t)));
                    had_field = true;
                }
            }
            ast::VariantBody::Named(fields) => {
                for f in fields {
                    out.push(format!("    {}: {}", f.name, py_type(&f.ty)));
                    had_field = true;
                }
            }
        }
        if let Some(ms) = methods {
            for sig in ms {
                out.push(format!("    {}", project_method(sig)));
                had_field = true;
            }
        }
        if !had_field {
            out.push("    ...".to_string());
        }
    }
    out.push(format!("{} = Union[{}]", e.name, variant_names.join(", ")));
    out
}

/// A class header `class Name:` or `class Name(Generic[T, …]):` for a type with
/// generic parameters.
fn class_header(name: &str, generics: &[ast::GenericParam]) -> String {
    if generics.is_empty() {
        format!("class {name}:")
    } else {
        let ps: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
        format!("class {name}(Generic[{}]):", ps.join(", "))
    }
}

/// The Python class name for an enum variant. neoc's `None` variant collides
/// with Python's `None` literal, so it is emitted as `None_`; every other
/// variant keeps its name.
#[must_use]
pub fn variant_class_name(variant: &str) -> String {
    if variant == "None" {
        "None_".to_string()
    } else {
        variant.to_string()
    }
}

/// Index the methods declared in `impl` blocks by their target type name, so a
/// type's `.pyi` stub can fold its method signatures into the class body.
fn method_index(module: &ast::Module) -> std::collections::HashMap<String, Vec<ast::FnSig>> {
    let mut out: std::collections::HashMap<String, Vec<ast::FnSig>> =
        std::collections::HashMap::new();
    for item in &module.items {
        if let ast::ItemKind::Impl(b) = &item.kind
            && let ast::Type::Path(p) = &b.ty
            && let Some(seg) = p.segments.last()
        {
            let entry = out.entry(seg.name.clone()).or_default();
            for f in &b.items {
                if has_receiver(&f.sig) {
                    entry.push(f.sig.clone());
                }
            }
        }
    }
    out
}

// ---- `Result`/`Option` reference detection --------------------------------

/// Whether the module references `Result`/`Option` in a typed position — the
/// trigger for the `neoc_lib` import in a `.py` or `.pyi`.
#[must_use]
pub fn references_lib(module: &ast::Module) -> bool {
    module.items.iter().any(|item| match &item.kind {
        ast::ItemKind::Fn(f) => sig_refs_lib(&f.sig),
        ast::ItemKind::Const(c) => type_refs_lib(&c.ty),
        ast::ItemKind::Struct(s) => struct_refs_lib(s),
        ast::ItemKind::Enum(e) => e.variants.iter().any(variant_refs_lib),
        ast::ItemKind::Impl(b) => b.items.iter().any(|f| sig_refs_lib(&f.sig)),
        ast::ItemKind::TypeAlias(a) => type_refs_lib(&a.value),
        _ => false,
    })
}

fn struct_refs_lib(s: &ast::StructDecl) -> bool {
    match &s.body {
        ast::StructBody::Unit => false,
        ast::StructBody::Tuple(t) => type_refs_lib(t),
        ast::StructBody::Named(fields) => fields.iter().any(|f| type_refs_lib(&f.ty)),
    }
}

fn struct_refs_self(s: &ast::StructDecl) -> bool {
    match &s.body {
        ast::StructBody::Unit => false,
        ast::StructBody::Tuple(t) => type_is_self(t),
        ast::StructBody::Named(fields) => fields.iter().any(|f| type_is_self(&f.ty)),
    }
}

fn type_is_self(ty: &ast::Type) -> bool {
    match ty {
        ast::Type::SelfType => true,
        ast::Type::List(inner) => type_is_self(inner),
        ast::Type::Fn { params, ret } => {
            params.iter().any(type_is_self) || ret.as_deref().is_some_and(type_is_self)
        }
        ast::Type::Path(p) => p.segments.iter().any(|s| s.args.iter().any(type_is_self)),
        ast::Type::Unit => false,
    }
}

fn variant_refs_lib(v: &ast::Variant) -> bool {
    match &v.body {
        ast::VariantBody::Unit => false,
        ast::VariantBody::Tuple(tys) => tys.iter().any(type_refs_lib),
        ast::VariantBody::Named(fields) => fields.iter().any(|f| type_refs_lib(&f.ty)),
    }
}

fn sig_refs_lib(sig: &ast::FnSig) -> bool {
    let in_params = sig.params.iter().any(|p| match p {
        ast::Param::Receiver => false,
        ast::Param::Named { ty, .. } => type_refs_lib(ty),
    });
    in_params || sig.ret.as_ref().is_some_and(type_refs_lib)
}

fn type_refs_lib(ty: &ast::Type) -> bool {
    match ty {
        ast::Type::Unit | ast::Type::SelfType => false,
        ast::Type::List(inner) => type_refs_lib(inner),
        ast::Type::Fn { params, ret } => {
            params.iter().any(type_refs_lib) || ret.as_deref().is_some_and(type_refs_lib)
        }
        ast::Type::Path(p) => p.segments.iter().any(|seg| {
            matches!(seg.name.as_str(), "Result" | "Option") || seg.args.iter().any(type_refs_lib)
        }),
    }
}

fn sig_refs_callable(sig: &ast::FnSig) -> bool {
    let in_params = sig.params.iter().any(|p| match p {
        ast::Param::Receiver => false,
        ast::Param::Named { ty, .. } => type_refs_callable(ty),
    });
    in_params || sig.ret.as_ref().is_some_and(type_refs_callable)
}

fn type_refs_callable(ty: &ast::Type) -> bool {
    match ty {
        ast::Type::Fn { .. } => true,
        ast::Type::List(inner) => type_refs_callable(inner),
        ast::Type::Path(p) => p
            .segments
            .iter()
            .any(|s| s.args.iter().any(type_refs_callable)),
        _ => false,
    }
}
