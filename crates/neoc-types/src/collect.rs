//! Declaration collection and AST-type lowering.
//!
//! Walks a `Module` once and fills the [`SymbolTable`] with every type and
//! value a later expression check might reference (`STATIC.md` §4.3). Also
//! lowers AST `Type` nodes into checker [`Ty`] values, resolving names against
//! the in-scope type parameters and the predeclared primitives.

use std::collections::HashMap;

use neoc_ast as ast;

use crate::env::{
    EnumInfo, MethodInfo, StructFields, StructInfo, SymbolTable, TraitInfo, TraitMethod, TypeDef,
    VariantFields, VariantInfo,
};
use crate::ty::Ty;

/// Lowers an AST `Type` to a checker [`Ty`].
///
/// `params` lists the type-parameter names in scope so that a single-segment
/// path naming one of them lowers to [`Ty::Param`] rather than a nominal type.
#[must_use]
pub fn lower_type(ty: &ast::Type, params: &[String]) -> Ty {
    match ty {
        ast::Type::Unit => Ty::Unit,
        ast::Type::List(inner) => Ty::List(Box::new(lower_type(inner, params))),
        ast::Type::Fn { params: ps, ret } => {
            let lowered = ps.iter().map(|p| lower_type(p, params)).collect();
            let ret = ret.as_ref().map_or(Ty::Unit, |r| lower_type(r, params));
            Ty::Fn(lowered, Box::new(ret))
        }
        // `Self` resolves to the impl/trait target; the caller substitutes it.
        ast::Type::SelfType => Ty::named("Self"),
        ast::Type::Path(path) => lower_path(path, params),
    }
}

fn lower_path(path: &ast::TypePath, params: &[String]) -> Ty {
    let Some(last) = path.segments.last() else {
        return Ty::Unknown;
    };
    // Only the final segment names the type; leading segments are module
    // qualifiers, which a single-module checker treats as transparent.
    let name = &last.name;
    let args: Vec<Ty> = last.args.iter().map(|a| lower_type(a, params)).collect();
    match name.as_str() {
        "number" => Ty::Number,
        "string" => Ty::Str,
        "bool" => Ty::Bool,
        _ if path.segments.len() == 1 && params.iter().any(|p| p == name) => {
            Ty::Param(name.clone())
        }
        _ => Ty::Named {
            name: name.clone(),
            args,
        },
    }
}

fn generic_names(generics: &[ast::GenericParam]) -> Vec<String> {
    generics.iter().map(|g| g.name.clone()).collect()
}

/// The trait-name bounds declared inline on a set of generic parameters
/// (`<T: Display, U: Clone>`). The bound's trait name is the last segment of
/// its path. Shared by function signatures and generic type declarations.
fn generic_param_bounds(generics: &[ast::GenericParam]) -> HashMap<String, Vec<String>> {
    let trait_name = |p: &ast::TypePath| p.segments.last().map(|s| s.name.clone());
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for g in generics {
        let names: Vec<String> = g.bounds.iter().filter_map(trait_name).collect();
        if !names.is_empty() {
            out.entry(g.name.clone()).or_default().extend(names);
        }
    }
    out
}

/// The trait-name bounds of each type parameter of a signature, gathered from
/// both the inline form (`<T: Display>`) and the `where` clause
/// (`where T: Display`). Drives method resolution on a bounded parameter and
/// call-site bound enforcement (`STATIC.md` §3.4, §3.5).
pub(crate) fn param_bounds(sig: &ast::FnSig) -> HashMap<String, Vec<String>> {
    let trait_name = |p: &ast::TypePath| p.segments.last().map(|s| s.name.clone());
    let mut out = generic_param_bounds(&sig.generics);
    for w in &sig.where_clause {
        let names: Vec<String> = w.bounds.iter().filter_map(trait_name).collect();
        if !names.is_empty() {
            out.entry(w.ty.clone()).or_default().extend(names);
        }
    }
    out
}

/// The function type of a signature, plus whether it has a `self` receiver.
fn fn_type(sig: &ast::FnSig, type_params: &[String]) -> (Vec<Ty>, Ty, bool) {
    let mut params = type_params.to_vec();
    params.extend(generic_names(&sig.generics));
    let mut tys = Vec::new();
    let mut has_receiver = false;
    for p in &sig.params {
        match p {
            ast::Param::Receiver => has_receiver = true,
            ast::Param::Named { ty, .. } => tys.push(lower_type(ty, &params)),
        }
    }
    let ret = sig
        .ret
        .as_ref()
        .map_or(Ty::Unit, |r| lower_type(r, &params));
    (tys, ret, has_receiver)
}

/// Collects every module-level declaration into a fresh [`SymbolTable`],
/// seeded with the prelude (`Result`, `Option` and their constructors).
#[must_use]
pub fn collect(module: &ast::Module) -> SymbolTable {
    let mut table = SymbolTable::new();
    seed_prelude(&mut table);
    for item in &module.items {
        collect_item(&mut table, item);
        record_visibility(&mut table, item);
    }
    // Derived methods are registered after every declaration (including user
    // `impl` blocks) is collected, so a hand-written method of the same name
    // always wins regardless of source order.
    for item in &module.items {
        collect_derives(&mut table, item);
    }
    table.normalize_aliases();
    table
}

/// Method names a derive provides and the signature each one carries, for a
/// receiver type `self_ty`. The receiver itself is implicit (`has_receiver`),
/// so the returned `params` list holds only the explicit arguments.
fn derived_method(derive: &str, self_ty: &Ty) -> Option<(&'static str, MethodInfo)> {
    let info = match derive {
        "Clone" => (
            "clone",
            MethodInfo {
                params: Vec::new(),
                ret: self_ty.clone(),
                has_receiver: true,
            },
        ),
        "Eq" => (
            "eq",
            MethodInfo {
                params: vec![self_ty.clone()],
                ret: Ty::Bool,
                has_receiver: true,
            },
        ),
        "Debug" => (
            "debug",
            MethodInfo {
                params: Vec::new(),
                ret: Ty::Str,
                has_receiver: true,
            },
        ),
        "Display" => (
            "display",
            MethodInfo {
                params: Vec::new(),
                ret: Ty::Str,
                has_receiver: true,
            },
        ),
        _ => return None,
    };
    Some(info)
}

/// Registers the methods that `#[derive(..)]` on `item` synthesises onto a
/// `struct` or `enum`, so calls like `x.clone()` type-check (`DYNAMIC.md` §6).
///
/// `self` is the receiver and resolves to the declaring type, applied to its
/// own generics (`clone(self) -> Box<T>`). A derived method is skipped when an
/// inherent or trait-impl method of the same name already exists, so a
/// hand-written definition takes precedence.
fn collect_derives(table: &mut SymbolTable, item: &ast::Item) {
    let (name, generics) = match &item.kind {
        ast::ItemKind::Struct(s) => (s.name.clone(), generic_names(&s.generics)),
        ast::ItemKind::Enum(e) => (e.name.clone(), generic_names(&e.generics)),
        _ => return,
    };
    let self_ty = Ty::Named {
        name: name.clone(),
        args: generics.iter().map(|g| Ty::Param(g.clone())).collect(),
    };
    for derive in derive_names(&item.attrs) {
        let Some((method, info)) = derived_method(&derive, &self_ty) else {
            continue;
        };
        // A recognised derive is an implicit trait impl — the derive name is
        // the trait name, so a `<T: Clone>` bound is satisfied by `#[derive(Clone)]`.
        table.trait_impls.insert((name.clone(), derive.clone()));
        let key = (name.clone(), method.to_string());
        if table.methods.contains_key(&key) {
            continue;
        }
        table.methods.insert(key, info);
    }
}

/// The derive names listed in every `#[derive(..)]` attribute on an item, e.g.
/// `["Clone", "Eq"]` for `#[derive(Clone, Eq)]`.
fn derive_names(attrs: &[ast::Attribute]) -> Vec<String> {
    let mut names = Vec::new();
    for attr in attrs {
        for outer in &attr.items {
            if outer.name != "derive" {
                continue;
            }
            for arg in &outer.args {
                if let ast::AttrArg::Item(inner) = arg {
                    names.push(inner.name.clone());
                }
            }
        }
    }
    names
}

fn seed_prelude(table: &mut SymbolTable) {
    // enum Result<T, E> { Ok(T), Err(E) }
    table.types.insert(
        "Result".to_string(),
        TypeDef::Enum(EnumInfo {
            generics: vec!["T".to_string(), "E".to_string()],
            variants: vec![
                VariantInfo {
                    name: "Ok".to_string(),
                    fields: VariantFields::Tuple(vec![Ty::Param("T".to_string())]),
                },
                VariantInfo {
                    name: "Err".to_string(),
                    fields: VariantFields::Tuple(vec![Ty::Param("E".to_string())]),
                },
            ],
        }),
    );
    // enum Option<T> { Some(T), None }
    table.types.insert(
        "Option".to_string(),
        TypeDef::Enum(EnumInfo {
            generics: vec!["T".to_string()],
            variants: vec![
                VariantInfo {
                    name: "Some".to_string(),
                    fields: VariantFields::Tuple(vec![Ty::Param("T".to_string())]),
                },
                VariantInfo {
                    name: "None".to_string(),
                    fields: VariantFields::Unit,
                },
            ],
        }),
    );
    // Constructors in the value namespace. Their signatures are generic; the
    // checker infers the type arguments at the call site.
    let t = Ty::Param("T".to_string());
    let e = Ty::Param("E".to_string());
    table.values.insert(
        "Ok".to_string(),
        Ty::Fn(vec![t.clone()], Box::new(Ty::result(t.clone(), e.clone()))),
    );
    table.values.insert(
        "Err".to_string(),
        Ty::Fn(vec![e.clone()], Box::new(Ty::result(t.clone(), e))),
    );
    table.values.insert(
        "Some".to_string(),
        Ty::Fn(vec![t.clone()], Box::new(Ty::option(t.clone()))),
    );
    table.values.insert("None".to_string(), Ty::option(t));
}

/// Records the names a `pub` item exports into the table's visibility sets, so
/// a cross-module `use` can tell a public item from a private one (`STATIC.md`
/// §2). A private item records nothing; an item declared `pub` registers the
/// name(s) it introduces in the value and/or type namespace.
fn record_visibility(table: &mut SymbolTable, item: &ast::Item) {
    if item.vis != ast::Visibility::Public {
        return;
    }
    match &item.kind {
        ast::ItemKind::Fn(f) => {
            table.pub_values.insert(f.sig.name.clone());
        }
        ast::ItemKind::Const(c) => {
            table.pub_values.insert(c.name.clone());
        }
        ast::ItemKind::TypeAlias(a) => {
            table.pub_types.insert(a.name.clone());
        }
        ast::ItemKind::Struct(s) => {
            table.pub_types.insert(s.name.clone());
            // A unit or tuple struct also introduces a value-namespace
            // constructor of the same name.
            if !matches!(s.body, ast::StructBody::Named(_)) {
                table.pub_values.insert(s.name.clone());
            }
        }
        ast::ItemKind::Enum(e) => {
            table.pub_types.insert(e.name.clone());
        }
        ast::ItemKind::Trait(t) => {
            table.pub_types.insert(t.name.clone());
        }
        // `use` re-exports, `impl` blocks, and `ext`/`extern` items do not
        // introduce a fresh module-level export name to gate.
        ast::ItemKind::Use(_)
        | ast::ItemKind::Impl(_)
        | ast::ItemKind::Ext(_)
        | ast::ItemKind::Extern { .. } => {}
    }
}

fn collect_item(table: &mut SymbolTable, item: &ast::Item) {
    match &item.kind {
        ast::ItemKind::Struct(s) => collect_struct(table, s),
        ast::ItemKind::Enum(e) => collect_enum(table, e),
        ast::ItemKind::Trait(t) => collect_trait(table, t),
        ast::ItemKind::Fn(f) => {
            let (params, ret, _) = fn_type(&f.sig, &[]);
            table
                .values
                .insert(f.sig.name.clone(), Ty::Fn(params, Box::new(ret)));
            let bounds = param_bounds(&f.sig);
            if !bounds.is_empty() {
                table.fn_bounds.insert(f.sig.name.clone(), bounds);
            }
        }
        ast::ItemKind::Const(c) => {
            table.values.insert(c.name.clone(), lower_type(&c.ty, &[]));
        }
        ast::ItemKind::TypeAlias(a) => {
            let generics = generic_names(&a.generics);
            let value = lower_type(&a.value, &generics);
            table
                .types
                .insert(a.name.clone(), TypeDef::Alias { generics, value });
        }
        ast::ItemKind::Impl(b) => collect_impl(table, b),
        ast::ItemKind::Ext(items) | ast::ItemKind::Extern { items, .. } => {
            for ext in items {
                collect_ext(table, ext);
            }
        }
        ast::ItemKind::Use(_) => {}
    }
}

fn collect_struct(table: &mut SymbolTable, s: &ast::StructDecl) {
    let generics = generic_names(&s.generics);
    // The struct's parameter bounds are enforced at construction — keyed by the
    // type name, read by the named-literal path and the tuple constructor call.
    let bounds = generic_param_bounds(&s.generics);
    if !bounds.is_empty() {
        table.fn_bounds.insert(s.name.clone(), bounds);
    }
    let fields = match &s.body {
        ast::StructBody::Unit => {
            // Unit struct value constructor: the type itself.
            table.values.insert(s.name.clone(), Ty::named(&s.name));
            StructFields::Unit
        }
        ast::StructBody::Tuple(ty) => {
            let inner = lower_type(ty, &generics);
            // Newtype constructor `N : fn(τ) -> N`.
            table.values.insert(
                s.name.clone(),
                Ty::Fn(vec![inner.clone()], Box::new(Ty::named(&s.name))),
            );
            StructFields::Tuple(inner)
        }
        ast::StructBody::Named(fs) => {
            let named = fs
                .iter()
                .map(|f| (f.name.clone(), lower_type(&f.ty, &generics)))
                .collect();
            StructFields::Named(named)
        }
    };
    table.types.insert(
        s.name.clone(),
        TypeDef::Struct(StructInfo { generics, fields }),
    );
}

fn collect_enum(table: &mut SymbolTable, e: &ast::EnumDecl) {
    let generics = generic_names(&e.generics);
    // The enum's parameter bounds are enforced at each tuple-variant
    // construction, so they are keyed by the variant's constructor name.
    let bounds = generic_param_bounds(&e.generics);
    let mut variants = Vec::new();
    for v in &e.variants {
        let fields = match &v.body {
            ast::VariantBody::Unit => {
                // Unit variant constructor lives under `Enum::Variant`; the
                // value namespace keys it by the variant name for the common
                // unqualified-prelude cases and as a fallback resolution.
                VariantFields::Unit
            }
            ast::VariantBody::Tuple(tys) => {
                let lowered: Vec<Ty> = tys.iter().map(|t| lower_type(t, &generics)).collect();
                let ret = enum_ty(&e.name, &generics);
                table
                    .values
                    .insert(v.name.clone(), Ty::Fn(lowered.clone(), Box::new(ret)));
                if !bounds.is_empty() {
                    table.fn_bounds.insert(v.name.clone(), bounds.clone());
                }
                VariantFields::Tuple(lowered)
            }
            ast::VariantBody::Named(fs) => {
                let named = fs
                    .iter()
                    .map(|f| (f.name.clone(), lower_type(&f.ty, &generics)))
                    .collect();
                VariantFields::Named(named)
            }
        };
        variants.push(VariantInfo {
            name: v.name.clone(),
            fields,
        });
    }
    table.types.insert(
        e.name.clone(),
        TypeDef::Enum(EnumInfo { generics, variants }),
    );
}

fn enum_ty(name: &str, generics: &[String]) -> Ty {
    Ty::Named {
        name: name.to_string(),
        args: generics.iter().map(|g| Ty::Param(g.clone())).collect(),
    }
}

fn collect_trait(table: &mut SymbolTable, t: &ast::TraitDecl) {
    let generics = generic_names(&t.generics);
    let methods = t
        .items
        .iter()
        .map(|item| {
            let (params, ret, has_receiver) = fn_type(&item.sig, &generics);
            TraitMethod {
                name: item.sig.name.clone(),
                params,
                ret,
                has_receiver,
                has_default: item.default.is_some(),
            }
        })
        .collect();
    table.types.insert(
        t.name.clone(),
        TypeDef::Trait(TraitInfo { generics, methods }),
    );
}

fn collect_impl(table: &mut SymbolTable, b: &ast::ImplBlock) {
    let impl_generics = generic_names(&b.generics);
    let Some(type_name) = impl_target_name(&b.ty) else {
        return;
    };
    // A trait impl records the `(type, trait)` pair so call-site bound checks
    // can see that `type_name` satisfies the trait.
    if let Some(trait_name) = b.trait_.as_ref().and_then(|p| p.segments.last()) {
        table
            .trait_impls
            .insert((type_name.clone(), trait_name.name.clone()));
    }
    for f in &b.items {
        let (params, ret, has_receiver) = fn_type(&f.sig, &impl_generics);
        // Substitute `Self` with the target type in params/ret.
        let params = params
            .into_iter()
            .map(|p| subst_self(p, &type_name))
            .collect();
        let ret = subst_self(ret, &type_name);
        table.methods.insert(
            (type_name.clone(), f.sig.name.clone()),
            MethodInfo {
                params,
                ret,
                has_receiver,
            },
        );
    }
}

/// The bare type name an `impl` targets, ignoring generic arguments.
fn impl_target_name(ty: &ast::Type) -> Option<String> {
    match ty {
        ast::Type::Path(p) => p.segments.last().map(|s| s.name.clone()),
        _ => None,
    }
}

fn subst_self(ty: Ty, target: &str) -> Ty {
    match ty {
        Ty::Named { name, args } if name == "Self" && args.is_empty() => Ty::named(target),
        Ty::Named { name, args } => Ty::Named {
            name,
            args: args.into_iter().map(|a| subst_self(a, target)).collect(),
        },
        Ty::List(inner) => Ty::List(Box::new(subst_self(*inner, target))),
        Ty::Fn(params, ret) => Ty::Fn(
            params.into_iter().map(|p| subst_self(p, target)).collect(),
            Box::new(subst_self(*ret, target)),
        ),
        other => other,
    }
}

fn collect_ext(table: &mut SymbolTable, ext: &ast::ExtItem) {
    match ext {
        ast::ExtItem::Fn(sig) => {
            let (params, ret, _) = fn_type(sig, &[]);
            table
                .values
                .insert(sig.name.clone(), Ty::Fn(params, Box::new(ret)));
        }
        ast::ExtItem::Type { name, .. } => {
            table.types.insert(name.clone(), TypeDef::Opaque);
        }
        ast::ExtItem::Const { name, ty } => {
            table.values.insert(name.clone(), lower_type(ty, &[]));
        }
    }
}
