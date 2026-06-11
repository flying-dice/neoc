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

/// A function's generic parameters that carry exactly one trait bound, mapped
/// from the parameter name to that bound's name. Such a parameter projects to
/// the bound's Python `Protocol` (e.g. `E: SqlExecutor` → `SqlExecutor`),
/// dropping the `TypeVar` — a single-use type variable is a pyright warning and
/// carries no more information than its bound's structural surface.
#[must_use]
pub fn single_bound_generics(sig: &ast::FnSig) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for g in &sig.effective_generics() {
        if let [bound] = g.bounds.as_slice()
            && let Some(seg) = bound.segments.last()
        {
            out.insert(g.name.clone(), seg.name.clone());
        }
    }
    out
}

/// Project a neoc type to Python, substituting each single-bound generic
/// parameter for its bound's `Protocol` name (see [`single_bound_generics`]).
#[must_use]
pub fn py_type_bound(ty: &ast::Type, bounds: &std::collections::HashMap<String, String>) -> String {
    py_type(&subst_bounds(ty, bounds))
}

/// Rewrite a type, replacing any path naming a single-bound generic parameter
/// with its bound's path. Pure — returns a fresh tree, never mutates the input.
fn subst_bounds(ty: &ast::Type, bounds: &std::collections::HashMap<String, String>) -> ast::Type {
    match ty {
        ast::Type::List(inner) => ast::Type::List(Box::new(subst_bounds(inner, bounds))),
        ast::Type::Fn { params, ret } => ast::Type::Fn {
            params: params.iter().map(|p| subst_bounds(p, bounds)).collect(),
            ret: ret.as_ref().map(|r| Box::new(subst_bounds(r, bounds))),
        },
        ast::Type::Path(p) => {
            if let [seg] = p.segments.as_slice()
                && seg.args.is_empty()
                && let Some(bound) = bounds.get(&seg.name)
            {
                return ast::Type::Path(ast::TypePath {
                    segments: vec![ast::TypeSegment {
                        name: bound.clone(),
                        args: Vec::new(),
                    }],
                });
            }
            let segments = p
                .segments
                .iter()
                .map(|seg| ast::TypeSegment {
                    name: seg.name.clone(),
                    args: seg.args.iter().map(|a| subst_bounds(a, bounds)).collect(),
                })
                .collect();
            ast::Type::Path(ast::TypePath { segments })
        }
        ast::Type::Unit | ast::Type::SelfType => ty.clone(),
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
        "number" => "float".to_string(),
        "string" => "str".to_string(),
        "bool" => "bool".to_string(),
        // A user type whose name collides with a Python builtin/keyword is
        // emitted escaped (it was declared with that escaped name too).
        other => py_ident(other),
    };
    if seg.args.is_empty() {
        name
    } else {
        let args: Vec<String> = seg.args.iter().map(py_type).collect();
        format!("{name}[{}]", args.join(", "))
    }
}

/// The Python annotation for a function's return type — its declared type, or
/// `None` when the function returns unit. Single-bound generics project to their
/// bound's `Protocol`.
fn ret_type(ret: Option<&ast::Type>, bounds: &std::collections::HashMap<String, String>) -> String {
    ret.map_or_else(|| "None".to_string(), |t| py_type_bound(t, bounds))
}

/// Render a function signature's parameter list as Python `name: T` entries.
/// A `self` receiver is kept verbatim (untyped, as Python methods take it).
/// Single-bound generics project to their bound's `Protocol`.
fn params(sig: &ast::FnSig, bounds: &std::collections::HashMap<String, String>) -> Vec<String> {
    sig.params
        .iter()
        .map(|p| match p {
            ast::Param::Receiver => "self".to_string(),
            ast::Param::Named { name, ty, .. } => format!("{name}: {}", py_type_bound(ty, bounds)),
        })
        .collect()
}

/// Whether a signature has a `self` receiver (it is a method, not a free fn).
fn has_receiver(sig: &ast::FnSig) -> bool {
    sig.params.iter().any(|p| matches!(p, ast::Param::Receiver))
}

/// Project a function's `def … -> R: ...` stub line. `is_method` keeps the name
/// verbatim (an attribute never collides); a free fn's name is escaped on a
/// reserved Python collision so it matches its module binding.
fn project_sig(sig: &ast::FnSig, is_method: bool) -> String {
    let bounds = single_bound_generics(sig);
    let name = if is_method {
        sig.name.clone()
    } else {
        py_ident(&sig.name)
    };
    format!(
        "def {name}({}) -> {}: ...",
        params(sig, &bounds).join(", "),
        ret_type(sig.ret.as_ref(), &bounds)
    )
}

/// Project a free function's `def … -> R: ...` stub line for a `.pyi`.
#[must_use]
pub fn project_fn(sig: &ast::FnSig) -> String {
    project_sig(sig, false)
}

/// Project a method's `def … -> R: ...` stub line (indented under a class in a
/// `.pyi`).
fn project_method(sig: &ast::FnSig) -> String {
    project_sig(sig, true)
}

/// The `.pyi` stub for a module: the typed surface a consumer reads.
///
/// Emits, in source order: a `from __future__`/typing import preamble when
/// referenced, a `@dataclass` class stub per `struct` (fields typed, methods
/// from any `impl`), one `@dataclass` per enum variant plus the union alias, a
/// `def … -> R: ...` per `pub` fn, and a typed name per `pub const`. Private
/// items are still surfaced in the stub (Python has no enforced visibility); the
/// `__all__` controls the public surface in the runtime `.py`.
///
/// `importer_dir` is this module's directory as root-relative path segments and
/// `bindings` the resolved `use` paths that name a Python symbol — together they
/// resolve a cross-module `use` to a relative `from … import …` so a referenced
/// type (`SqlExecutor`, an imported `struct`) is in scope in the stub, mirroring
/// the runtime `.py`.
#[must_use]
pub fn module_pyi<S: std::hash::BuildHasher>(
    module: &ast::Module,
    importer_dir: &[String],
    bindings: &std::collections::HashSet<String, S>,
) -> String {
    let mut imports: Vec<String> = vec!["from dataclasses import dataclass".to_string()];
    let use_imports = use_imports(module, importer_dir, bindings);
    let mut typing: Vec<&str> = Vec::new();
    let mut needs_lib = false;
    let mut needs_self = false;
    let mut needs_protocol = false;
    let mut body: Vec<String> = Vec::new();
    // Distinct generic-parameter names across the module's types and `fn`s, each
    // declared as a module-level `TypeVar` so annotations naming them resolve.
    let type_vars = collect_type_vars(module);

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
                body.push(format!("{}: {}", py_ident(&c.name), py_type(&c.ty)));
            }
            ast::ItemKind::Trait(t) => {
                needs_protocol = true;
                needs_lib |= t.items.iter().any(|i| sig_refs_lib(&i.sig));
                if t.items.iter().any(|i| sig_refs_callable(&i.sig)) {
                    typing.push("Callable");
                }
                body.extend(trait_pyi(t));
                body.push(String::new());
            }
            _ => {}
        }
    }

    if needs_lib {
        // `neoc_lib` sits at the output root; a nested module reaches it through
        // that many extra parent dots (`autosql/x.pyi` → `from ..neoc_lib …`).
        let dots = ".".repeat(1 + importer_dir.len());
        imports.push(format!(
            "from {dots}neoc_lib import Result, Option, Ok, Err, Some, None_"
        ));
    }
    imports.extend(use_imports);
    if needs_self {
        typing.push("Self");
    }
    if type_carries_generics(module) {
        typing.push("Generic");
    }
    if !type_vars.is_empty() {
        typing.push("TypeVar");
    }
    if needs_protocol {
        typing.push("Protocol");
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
    for tv in &type_vars {
        out.push(format!("{tv} = TypeVar(\"{tv}\")"));
    }
    if !type_vars.is_empty() {
        out.push(String::new());
    }
    out.extend(body);
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// The cross-module `use` imports a `.pyi` needs: each `use a::b::c` whose path
/// names a Python symbol becomes a relative `from … import c` (escaped + aliased
/// as written), so a referenced imported type is in scope in the stub.
fn use_imports<S: std::hash::BuildHasher>(
    module: &ast::Module,
    importer_dir: &[String],
    bindings: &std::collections::HashSet<String, S>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in &module.items {
        if let ast::ItemKind::Use(u) = &item.kind
            && let Some((last, prefix)) = u.path.split_last()
            && !prefix.is_empty()
            && bindings.contains(&u.path.join("::"))
        {
            let target: Vec<&str> = prefix.iter().map(String::as_str).collect();
            let module_spec = relative_module(importer_dir, &target);
            let last = py_ident(last);
            match &u.alias {
                Some(alias) => {
                    out.push(format!(
                        "from {module_spec} import {last} as {}",
                        py_ident(alias)
                    ));
                }
                None => out.push(format!("from {module_spec} import {last}")),
            }
        }
    }
    out
}

/// The Python relative-import specifier for a target module given as
/// root-relative path segments, resolved against `importer_dir`. Mirrors the
/// runtime `.py` resolver: drop the shared leading directories, then emit
/// `1 + ups` leading dots followed by the remaining segments joined with `.`.
fn relative_module(importer_dir: &[String], target: &[&str]) -> String {
    let mut shared = 0;
    while shared < importer_dir.len()
        && shared + 1 < target.len()
        && importer_dir[shared] == target[shared]
    {
        shared += 1;
    }
    let ups = importer_dir.len() - shared;
    let dots = ".".repeat(1 + ups);
    format!("{dots}{}", target[shared..].join("."))
}

/// The `.pyi` stub for a `trait`: a `Protocol` whose method signatures form the
/// structural contract an implementor satisfies. A generic parameter bounded by
/// the trait projects to this `Protocol`, so `db.method(...)` type-checks.
fn trait_pyi(t: &ast::TraitDecl) -> Vec<String> {
    let mut out = vec![format!("class {}(Protocol):", py_ident(&t.name))];
    if t.items.is_empty() {
        out.push("    ...".to_string());
        return out;
    }
    for item in &t.items {
        out.push(format!("    {}", project_method(&item.sig)));
    }
    out
}

/// The distinct generic-parameter names declared across a module's types and
/// functions, in first-seen order — each becomes a module-level `TypeVar` in
/// the `.pyi` so any annotation naming it resolves.
fn collect_type_vars(module: &ast::Module) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // A type's params are always `TypeVar`s (they sit in `Generic[...]`).
    let push_all = |gs: &[ast::GenericParam], out: &mut Vec<String>| {
        for g in gs {
            if !out.contains(&g.name) {
                out.push(g.name.clone());
            }
        }
    };
    // A `fn`'s single-bound params project to their bound's `Protocol`, not a
    // `TypeVar`; only the remaining params need a `TypeVar` declaration.
    let push_fn = |sig: &ast::FnSig, out: &mut Vec<String>| {
        for g in &sig.effective_generics() {
            let single_bound = matches!(g.bounds.as_slice(), [b] if b.segments.last().is_some());
            if !single_bound && !out.contains(&g.name) {
                out.push(g.name.clone());
            }
        }
    };
    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Struct(s) => push_all(&s.generics, &mut out),
            ast::ItemKind::Enum(e) => push_all(&e.generics, &mut out),
            ast::ItemKind::Fn(f) => push_fn(&f.sig, &mut out),
            ast::ItemKind::Impl(b) => {
                for f in &b.items {
                    push_fn(&f.sig, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether any `struct`/`enum` in the module carries generic parameters — the
/// trigger for the `Generic` import (its class header reads `Generic[...]`).
fn type_carries_generics(module: &ast::Module) -> bool {
    module.items.iter().any(|item| match &item.kind {
        ast::ItemKind::Struct(s) => !s.generics.is_empty(),
        ast::ItemKind::Enum(e) => !e.generics.is_empty(),
        _ => false,
    })
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
    out.push(format!(
        "{} = Union[{}]",
        py_ident(&e.name),
        variant_names.join(", ")
    ));
    out
}

/// A class header `class Name:` or `class Name(Generic[T, …]):` for a type with
/// generic parameters.
fn class_header(name: &str, generics: &[ast::GenericParam]) -> String {
    let name = py_ident(name);
    if generics.is_empty() {
        format!("class {name}:")
    } else {
        let ps: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
        format!("class {name}(Generic[{}]):", ps.join(", "))
    }
}

/// The Python class name for an enum variant. neoc's `None` variant collides
/// with Python's `None` literal, so it is emitted as `None_`; every other
/// variant keeps its name (still routed through [`py_ident`] for any other
/// reserved collision).
#[must_use]
pub fn variant_class_name(variant: &str) -> String {
    if variant == "None" {
        "None_".to_string()
    } else {
        py_ident(variant)
    }
}

/// Python names that a neoc top-level `fn`/`struct`/`enum`/`trait`/`const` must
/// not take verbatim: a `def`/`class` binding them would shadow the builtin in
/// module scope, and a builtin like `list` doubling as a generic type form
/// (`list[T]`) then resolves to the binding, not the type. Each is escaped with
/// a trailing underscore, mirroring the `None` → `None_` precedent.
const RESERVED_PY_NAMES: &[&str] = &[
    // Builtins that double as type forms in annotations.
    "list",
    "dict",
    "set",
    "frozenset",
    "tuple",
    "type",
    "str",
    "int",
    "float",
    "bool",
    "bytes",
    "object",
    "complex",
    "range",
    "bytearray",
    "memoryview",
    // Soft/hard keywords that cannot be a binding name.
    "match",
    "case",
    "None",
    "True",
    "False",
    "class",
    "def",
    "return",
    "import",
    "from",
    "as",
    "if",
    "else",
    "elif",
    "for",
    "while",
    "in",
    "is",
    "and",
    "or",
    "not",
    "lambda",
    "with",
    "try",
    "except",
    "finally",
    "raise",
    "yield",
    "global",
    "nonlocal",
    "pass",
    "break",
    "continue",
    "del",
    "assert",
    "async",
    "await",
];

/// Escape a neoc identifier that would collide with a Python builtin-as-type or
/// keyword when emitted as a top-level binding. A reserved name gains a trailing
/// underscore (`list` → `list_`); every other name passes through unchanged.
#[must_use]
pub fn py_ident(name: &str) -> String {
    if RESERVED_PY_NAMES.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
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
