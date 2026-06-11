//! Typed AST for neoc. Pure data: no host or runtime types, no I/O. Nodes the
//! type checker reports against ([`Expr`], [`Stmt`], [`Item`]) carry a byte
//! [`Span`] so diagnostics can point at the offending source. Mirrors
//! `lang/SYNTAX.md`.
//!
//! Every node derives `Debug, Clone, PartialEq` so the parser can be tested by
//! asserting the produced AST against an expected value, independently of the
//! emitter. [`Span`] compares always-equal (see its `PartialEq`), so adding a
//! span to a node leaves AST-equality tests untouched.

/// An identifier — a name in either the value or type namespace.
pub type Ident = String;

/// A half-open byte range `[start, end)` into the source text the node came
/// from.
///
/// All `Span`s compare equal regardless of their offsets. This is deliberate:
/// the AST derives `PartialEq` so the parser can be tested by asserting the
/// produced tree against an expected one, and a span carries position, not
/// structure. Comparing spans structurally would force every expected-AST test
/// to thread exact byte offsets; comparing them as always-equal lets a node
/// gain a span without disturbing those tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl PartialEq for Span {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Span {}

/// A complete source file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub items: Vec<Item>,
}

// ---- items -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub attrs: Vec<Attribute>,
    pub vis: Visibility,
    pub kind: ItemKind,
    pub span: Span,
}

/// A backend target. One target is chosen per compilation; `extern` blocks
/// describe that target's surface (`lang/EXTERN.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Lua,
    Js,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Use(UseDecl),
    Const(ConstDecl),
    TypeAlias(TypeAlias),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplBlock),
    Fn(FnDecl),
    /// An `extern { ... }` block: the surface of the external target
    /// (`lang/EXTERN.md`). A syntactic region carrying bodyless `mod`/`struct`/
    /// `impl`/`fn` items. The block itself takes no visibility and binds no
    /// name; its contents describe target values that lower directly — the neoc
    /// value of an extern type *is* the target value.
    Extern(ExternBlock),
}

/// An `extern { ... }` block (`lang/EXTERN.md` §Block syntax). A region whose
/// items have no bodies; each describes a piece of the bound target's surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    pub items: Vec<ExternItem>,
}

/// One item inside an `extern` block: its attributes, visibility, and the form
/// it declares (`lang/EXTERN.md` §Items).
#[derive(Debug, Clone, PartialEq)]
pub struct ExternItem {
    pub attrs: Vec<Attribute>,
    pub vis: Visibility,
    pub kind: ExternItemKind,
    pub span: Span,
}

/// The four item forms permitted in an `extern` block. `struct` reuses
/// [`StructDecl`] (carrying fields), `impl` reuses [`ImplBlock`] (its methods
/// bodyless), and `fn` reuses [`FnSig`] (no body). `mod` is a named scope that
/// nests further extern items.
#[derive(Debug, Clone, PartialEq)]
pub enum ExternItemKind {
    /// A named target-module scope. Nesting maps to the target path joined by
    /// `/` unless overridden by `#[extern(path = "...")]`.
    Mod(ExternMod),
    /// A nominal target handle, with data/callback fields but no methods.
    Struct(StructDecl),
    /// A bodyless inherent method block (method signatures only).
    Impl(ImplBlock),
    /// A bodyless function signature.
    Fn(FnSig),
}

/// A `mod` inside an `extern` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternMod {
    pub name: Ident,
    pub items: Vec<ExternItem>,
}

/// A target module to import, computed from an extern `mod`
/// (`lang/EXTERN.md` §Lowering). The neoc-side binding is `local_name`; the
/// target value is reached via `path` (a module specifier to import, or — when
/// `ambient` — a dotted member-access chain rooted at the target global, with
/// no import emitted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternImport {
    /// The neoc-side identifier a `mod::item` reference resolves through (the
    /// `mod`'s name, or its `#[extern(name = "...")]`).
    pub local_name: Ident,
    /// The target path: a module specifier (`vnd:rusqlite`, `fs/promises`) for a
    /// normal `mod`, or the dotted global root (`globalThis.crypto.subtle`) for
    /// an ambient one.
    pub path: String,
    /// Whether the `mod` is `#[extern(ambient)]` — no import is emitted; the
    /// `path` is a member-access chain from the target global.
    pub ambient: bool,
}

/// The top-level extern `mod` imports a `target` backend must wire, computed
/// from the `extern` blocks in `module` (`lang/EXTERN.md` §Lowering).
///
/// One entry per top-level `mod` declared directly under an `extern` block. Its
/// `path` is the `mod`'s `#[extern(path = "...")]`, or its name when none is
/// set; `ambient` mods carry their global-root path and emit no import. Entries
/// preserve declaration order. Nested mods append to the path with `/` but are
/// not surfaced as separate imports — they are reached through the top-level
/// binding. `target` is accepted for parity with the other backend queries; the
/// computed imports are target-agnostic, so it does not yet vary the result.
#[must_use]
pub fn extern_imports(module: &Module, target: Target) -> Vec<ExternImport> {
    let mut out = Vec::new();
    for item in &module.items {
        if let ItemKind::Extern(block) = &item.kind {
            out.extend(extern_imports_from_items(&block.items, target));
        }
    }
    out
}

/// The extern-`mod` imports declared directly among `items`, the item-list form
/// of [`extern_imports`].
///
/// A backend feeds it both its own module's extern items and the extern items a
/// `use` brought in from another file (`lang/EXTERN.md` §Lowering): a `use`d
/// extern item lowers to its **native** import in the consumer, not an import
/// from the defining file's emitted module, so the consumer reconstructs the
/// `mod`'s import from the imported item's `#[extern(...)]` metadata. Only
/// top-level `mod`s among `items` produce imports; nested mods are reached
/// through the top-level binding.
#[must_use]
pub fn extern_imports_from_items(items: &[ExternItem], _target: Target) -> Vec<ExternImport> {
    let mut out = Vec::new();
    for ext in items {
        let ExternItemKind::Mod(m) = &ext.kind else {
            continue;
        };
        // `#[extern(name)]` is for `fn`/`struct`, not `mod`; a mod's neoc
        // binding is always its declared name. `#[extern(path)]` overrides
        // the target path; `ambient` makes the path a global-root chain.
        let attr = ExternAttr::parse(&ext.attrs).unwrap_or_default();
        let path = attr.path.clone().unwrap_or_else(|| m.name.clone());
        out.push(ExternImport {
            local_name: m.name.clone(),
            path,
            ambient: attr.ambient,
        });
    }
    out
}

/// Extracts the top-level extern items from `source` whose names are in
/// `names`, preserving each enclosing `mod` so the import path / `ambient` /
/// `name` metadata survives. The result is the slice a consumer needs to
/// reconstruct the native import and lowering for the extern items it `use`s
/// across files (`lang/EXTERN.md` §Lowering).
///
/// An extern item is kept when it (or, for a `mod`, any descendant `struct`/`fn`)
/// names one of `names`. A kept `mod` carries the full descendant subtree of
/// matching items — its constructors (`impl`s targeting a kept struct) and the
/// `mod`'s own `#[extern(...)]` attributes ride along — so the consumer's
/// backend can run the same extern collectors over the result it runs over its
/// own blocks.
#[must_use]
pub fn imported_extern_items<S: std::hash::BuildHasher>(
    source: &Module,
    names: &std::collections::HashSet<Ident, S>,
) -> Vec<ExternItem> {
    let mut out = Vec::new();
    for item in &source.items {
        let ItemKind::Extern(block) = &item.kind else {
            continue;
        };
        for ext in &block.items {
            if let Some(kept) = filter_extern_item(ext, names) {
                out.push(kept);
            }
        }
    }
    out
}

/// Keeps `ext` if it (or, for a `mod`, any descendant) names one of `names`.
/// A `mod` is rebuilt with only its matching descendants and any `impl` whose
/// target type matches (its constructors). A bare `struct`/`fn` is kept whole.
fn filter_extern_item<S: std::hash::BuildHasher>(
    ext: &ExternItem,
    names: &std::collections::HashSet<Ident, S>,
) -> Option<ExternItem> {
    match &ext.kind {
        ExternItemKind::Struct(s) if names.contains(&s.name) => Some(ext.clone()),
        ExternItemKind::Fn(sig) if names.contains(&sig.name) => Some(ext.clone()),
        ExternItemKind::Mod(m) => {
            let kept = filter_extern_items(&m.items, names);
            if kept.is_empty() {
                return None;
            }
            Some(ExternItem {
                attrs: ext.attrs.clone(),
                vis: ext.vis,
                kind: ExternItemKind::Mod(ExternMod {
                    name: m.name.clone(),
                    items: kept,
                }),
                span: ext.span,
            })
        }
        _ => None,
    }
}

/// Filters a list of extern items to those reaching `names`, keeping any `impl`
/// whose target type is among the kept structs so a `use`d struct carries its
/// constructors and associated functions.
fn filter_extern_items<S: std::hash::BuildHasher>(
    items: &[ExternItem],
    names: &std::collections::HashSet<Ident, S>,
) -> Vec<ExternItem> {
    // First pass: kept structs/fns/mods. Second pass: impls whose target type
    // was kept, so the consumer reconstructs constructors and assoc calls.
    let mut kept: Vec<ExternItem> = items
        .iter()
        .filter_map(|e| filter_extern_item(e, names))
        .collect();
    let kept_types: std::collections::HashSet<Ident> = kept
        .iter()
        .filter_map(|e| match &e.kind {
            ExternItemKind::Struct(s) => Some(s.name.clone()),
            _ => None,
        })
        .collect();
    for ext in items {
        if let ExternItemKind::Impl(b) = &ext.kind
            && let Type::Path(p) = &b.ty
            && let Some(seg) = p.segments.last()
            && kept_types.contains(&seg.name)
        {
            kept.push(ext.clone());
        }
    }
    kept
}

/// The names declared as extern `struct`s anywhere in `module`'s `extern`
/// blocks (recursively, including inside `mod`s).
///
/// A struct-literal `T { f: v }` of such a name lowers to a bare target object
/// literal `{ f: v }` (not a tagged neoc struct), and a method/associated call
/// on it lowers directly. Backends consult this set to tell an extern handle
/// apart from a native neoc struct.
#[must_use]
pub fn extern_struct_names(module: &Module) -> std::collections::HashSet<Ident> {
    let mut names = std::collections::HashSet::new();
    for item in &module.items {
        if let ItemKind::Extern(block) = &item.kind {
            collect_extern_structs(&block.items, &mut names);
        }
    }
    names
}

/// The extern `struct` names among `items` (recursively through `mod`s), the
/// item-list form of [`extern_struct_names`]. A backend feeds it the extern
/// items a `use` brought in so an imported extern struct lowers the same as a
/// local one.
#[must_use]
pub fn extern_struct_names_from_items(items: &[ExternItem]) -> std::collections::HashSet<Ident> {
    let mut names = std::collections::HashSet::new();
    collect_extern_structs(items, &mut names);
    names
}

/// The neoc-side names of every extern `struct` and `fn` among `items`
/// (recursively through `mod`s). A backend uses this to recognise a `use`d name
/// as extern-origin and so suppress an import from the defining file — a `use`d
/// extern value is reached through its native import, not the defining module's
/// emitted output (`lang/EXTERN.md` §Lowering).
#[must_use]
pub fn extern_value_names_from_items(items: &[ExternItem]) -> std::collections::HashSet<Ident> {
    let mut names = std::collections::HashSet::new();
    collect_extern_value_names(items, &mut names);
    names
}

fn collect_extern_value_names(items: &[ExternItem], names: &mut std::collections::HashSet<Ident>) {
    for ext in items {
        match &ext.kind {
            ExternItemKind::Struct(s) => {
                names.insert(s.name.clone());
            }
            ExternItemKind::Fn(sig) => {
                names.insert(sig.name.clone());
            }
            ExternItemKind::Mod(m) => collect_extern_value_names(&m.items, names),
            ExternItemKind::Impl(_) => {}
        }
    }
}

fn collect_extern_structs(items: &[ExternItem], names: &mut std::collections::HashSet<Ident>) {
    for ext in items {
        match &ext.kind {
            ExternItemKind::Struct(s) => {
                names.insert(s.name.clone());
            }
            ExternItemKind::Mod(m) => collect_extern_structs(&m.items, names),
            _ => {}
        }
    }
}

/// The `(type, method)` pairs marked `#[extern(constructor)]` across every
/// `impl` in `module`'s `extern` blocks.
///
/// A call `T::new(args)` to such a pair lowers to the target's constructor form
/// (`new T(args)` on JavaScript) rather than a static call. The `type` is the
/// impl's target name; the `method` is the constructor `fn`'s name.
#[must_use]
pub fn extern_constructors(module: &Module) -> std::collections::HashSet<(Ident, Ident)> {
    let mut out = std::collections::HashSet::new();
    for item in &module.items {
        if let ItemKind::Extern(block) = &item.kind {
            collect_extern_constructors(&block.items, &mut out);
        }
    }
    out
}

/// The `(type, method)` constructor pairs among `items`, the item-list form of
/// [`extern_constructors`]. A backend feeds it `use`-imported extern items so an
/// imported `T::new(args)` lowers to the target's constructor form.
#[must_use]
pub fn extern_constructors_from_items(
    items: &[ExternItem],
) -> std::collections::HashSet<(Ident, Ident)> {
    let mut out = std::collections::HashSet::new();
    collect_extern_constructors(items, &mut out);
    out
}

fn collect_extern_constructors(
    items: &[ExternItem],
    out: &mut std::collections::HashSet<(Ident, Ident)>,
) {
    for ext in items {
        match &ext.kind {
            ExternItemKind::Impl(b) => {
                let Type::Path(p) = &b.ty else { continue };
                let Some(ty) = p.segments.last().map(|s| s.name.clone()) else {
                    continue;
                };
                for m in &b.items {
                    if ExternAttr::parse(&m.attrs).is_some_and(|a| a.constructor) {
                        out.insert((ty.clone(), m.sig.name.clone()));
                    }
                }
            }
            ExternItemKind::Mod(m) => collect_extern_constructors(&m.items, out),
            _ => {}
        }
    }
}

/// Maps each extern `struct` declared inside an extern `mod` to the neoc-side
/// binding its enclosing `mod` resolves through (the `mod`'s name — its
/// [`ExternImport::local_name`]).
///
/// An extern struct's **value** — its `#[extern(constructor)]` call
/// `T::new(args)`, any associated/static call `T::f(args)`, and the constructed
/// identity — is reached through the enclosing module's import, not a bare name
/// (`lang/EXTERN.md` §Lowering). Backends qualify the reference with the
/// returned binding (`sqlite.Database` for a `Database` in `mod sqlite`). A
/// struct declared directly under an `extern` block (no enclosing `mod`) is
/// absent from the map and keeps its bare name.
#[must_use]
pub fn extern_struct_namespaces(module: &Module) -> std::collections::HashMap<Ident, Ident> {
    let mut out = std::collections::HashMap::new();
    for item in &module.items {
        if let ItemKind::Extern(block) = &item.kind {
            collect_extern_struct_namespaces(&block.items, None, &mut out);
        }
    }
    out
}

/// The extern-struct-to-`mod`-binding map among `items`, the item-list form of
/// [`extern_struct_namespaces`]. A backend feeds it `use`-imported extern items
/// so an imported struct's value reference qualifies through its `mod`'s import
/// binding (`sqlite.Database`).
#[must_use]
pub fn extern_struct_namespaces_from_items(
    items: &[ExternItem],
) -> std::collections::HashMap<Ident, Ident> {
    let mut out = std::collections::HashMap::new();
    collect_extern_struct_namespaces(items, None, &mut out);
    out
}

fn collect_extern_struct_namespaces(
    items: &[ExternItem],
    ns: Option<&Ident>,
    out: &mut std::collections::HashMap<Ident, Ident>,
) {
    for ext in items {
        match &ext.kind {
            ExternItemKind::Struct(s) => {
                if let Some(ns) = ns {
                    out.insert(s.name.clone(), ns.clone());
                }
            }
            // A `mod`'s neoc binding is its declared name (`#[extern(path)]`
            // only retargets the import specifier, `#[extern(name)]` is not a
            // `mod` attribute). Nested mods are reached through the top-level
            // binding, so the namespace stays the outermost mod's name.
            ExternItemKind::Mod(m) => {
                let inner = ns.unwrap_or(&m.name);
                collect_extern_struct_namespaces(&m.items, Some(inner), out);
            }
            _ => {}
        }
    }
}

/// Maps each extern `fn` declared inside an extern `mod` to the neoc-side
/// binding its enclosing `mod` resolves through (the `mod`'s name — its
/// [`ExternImport::local_name`]).
///
/// A free extern `fn` in a `mod` is reached through the enclosing module's
/// import, not a bare name (`lang/EXTERN.md` §Lowering): a call `serve(opts)` to
/// a `fn serve` declared in `mod Bun` lowers to `Bun.serve(opts)` — the `mod`'s
/// binding qualifies the call, exactly as an extern struct's constructor /
/// associated calls qualify through [`extern_struct_namespaces`]. A fn declared
/// directly under an `extern` block (no enclosing `mod`) is absent from the map
/// and keeps its bare name.
#[must_use]
pub fn extern_fn_namespaces(module: &Module) -> std::collections::HashMap<Ident, Ident> {
    let mut out = std::collections::HashMap::new();
    for item in &module.items {
        if let ItemKind::Extern(block) = &item.kind {
            collect_extern_fn_namespaces(&block.items, None, &mut out);
        }
    }
    out
}

/// The extern-fn-to-`mod`-binding map among `items`, the item-list form of
/// [`extern_fn_namespaces`]. A backend feeds it `use`-imported extern items so a
/// `use`d free fn's call qualifies through its `mod`'s import binding
/// (`Bun.serve`), matching the locally-declared case.
#[must_use]
pub fn extern_fn_namespaces_from_items(
    items: &[ExternItem],
) -> std::collections::HashMap<Ident, Ident> {
    let mut out = std::collections::HashMap::new();
    collect_extern_fn_namespaces(items, None, &mut out);
    out
}

fn collect_extern_fn_namespaces(
    items: &[ExternItem],
    ns: Option<&Ident>,
    out: &mut std::collections::HashMap<Ident, Ident>,
) {
    for ext in items {
        match &ext.kind {
            ExternItemKind::Fn(sig) => {
                if let Some(ns) = ns {
                    out.insert(sig.name.clone(), ns.clone());
                }
            }
            // A `mod`'s neoc binding is its declared name; nested mods are
            // reached through the top-level binding, so the namespace stays the
            // outermost mod's name (mirrors `collect_extern_struct_namespaces`).
            ExternItemKind::Mod(m) => {
                let inner = ns.unwrap_or(&m.name);
                collect_extern_fn_namespaces(&m.items, Some(inner), out);
            }
            _ => {}
        }
    }
}

/// The parsed `#[extern(...)]` attribute on an extern item
/// (`lang/EXTERN.md` §Attributes). A closed set of params; absent params are
/// `false`/`None`. Built from an item's [`Attribute`] list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternAttr {
    /// `#[extern(ambient)]` — no import; referenced from the target global root.
    pub ambient: bool,
    /// `#[extern(path = "...")]` — a `mod`'s target path.
    pub path: Option<String>,
    /// `#[extern(name = "...")]` — an `fn`/`struct`'s target identifier.
    pub name: Option<String>,
    /// `#[extern(constructor)]` — marks a constructor `fn` in an `impl`.
    pub constructor: bool,
}

impl ExternAttr {
    /// Parses the single `#[extern(...)]` attribute in `attrs`, if present.
    ///
    /// Recognises the closed param set (`ambient`, `path = "..."`,
    /// `name = "..."`, `constructor`). Unknown params and malformed shapes are
    /// surfaced by the type checker; this accessor reads what is well-formed and
    /// ignores the rest, so codegen can lower without re-validating.
    #[must_use]
    pub fn parse(attrs: &[Attribute]) -> Option<Self> {
        let entry = attrs
            .iter()
            .flat_map(|a| &a.items)
            .find(|i| i.name == "extern")?;
        let mut out = ExternAttr::default();
        for arg in &entry.args {
            match arg {
                AttrArg::Item(inner) if inner.name == "ambient" => out.ambient = true,
                AttrArg::Item(inner) if inner.name == "constructor" => out.constructor = true,
                AttrArg::Named { name, value } => {
                    if let AttrArg::Str(s) = value.as_ref() {
                        match name.as_str() {
                            "path" => out.path = Some(s.clone()),
                            "name" => out.name = Some(s.clone()),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        Some(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseDecl {
    pub path: Vec<Ident>,
    pub alias: Option<Ident>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub name: Ident,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAlias {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub value: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub body: StructBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructBody {
    Unit,
    Tuple(Type),
    Named(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    /// Byte span of the field name at its declaration, for member resolution.
    pub name_span: Span,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    /// Byte span of the variant name at its declaration, for member resolution.
    pub name_span: Span,
    pub body: VariantBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantBody {
    Unit,
    Tuple(Vec<Type>),
    Named(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    /// Supertraits the trait requires of its implementors (`trait A: B + C`).
    /// Each is the path of a required trait; an `impl A for T` must also
    /// provide an `impl B for T`.
    pub supertraits: Vec<TypePath>,
    /// Associated types the trait declares (`type Item;`). An implementor binds
    /// each in its `impl`, and trait methods may refer to them as `Self::Item`.
    pub assoc_types: Vec<Ident>,
    pub items: Vec<TraitItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitItem {
    pub sig: FnSig,
    /// Present when the trait method has a default body.
    pub default: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub generics: Vec<GenericParam>,
    /// Present for a trait impl (`impl Trait for Type`), absent for inherent.
    pub trait_: Option<TypePath>,
    pub ty: Type,
    pub items: Vec<FnDecl>,
    /// Associated-type bindings (`type Item = number;`) declared in the impl
    /// body, pairing each name with its concrete type.
    pub assoc_bindings: Vec<(Ident, Type)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub attrs: Vec<Attribute>,
    pub sig: FnSig,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub name: Ident,
    /// Byte span of the function/method name at its declaration, for member
    /// resolution (goto a method from a call site).
    pub name_span: Span,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub where_clause: Vec<WherePredicate>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// The `self` receiver — always by value (no references in neoc).
    Receiver,
    Named {
        name: Ident,
        ty: Type,
        /// The span of the parameter name — its binding site, for scope-aware
        /// resolution.
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub name: Ident,
    pub bounds: Vec<TypePath>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WherePredicate {
    pub ty: Ident,
    pub bounds: Vec<TypePath>,
}

impl FnSig {
    /// The generic parameters with every `where` predicate folded into the
    /// matching parameter's bounds. `where T: Bound` is equivalent to an inline
    /// `<T: Bound>` — the checker already unifies the two
    /// (`neoc-types::collect::param_bounds`), but codegen reads bounds off the
    /// generic params, so a `where`-form bound must be folded here to reach the
    /// emitted type surface (TS `extends`, Python `Protocol`). A predicate
    /// naming no declared parameter is dropped, as the checker does.
    /// (`STATIC.md` §3.4)
    #[must_use]
    pub fn effective_generics(&self) -> Vec<GenericParam> {
        let mut generics = self.generics.clone();
        for pred in &self.where_clause {
            if let Some(g) = generics.iter_mut().find(|g| g.name == pred.ty) {
                g.bounds.extend(pred.bounds.iter().cloned());
            }
        }
        // Bounds resolve as a set (`STATIC.md` §3.4: a parameter's methods are
        // the union of its bounds), so an inline `<A: Doubler>` and a redundant
        // `where A: Doubler` name one bound, not two. The checker tolerates the
        // duplicate — method resolution is set-like — but the emitted surface
        // counts elements: a dup emits TS `extends Doubler & Doubler` and slips
        // the single-bound Python projector (keyed on exactly one bound),
        // dropping the `Protocol`. Dedup the folded view so every backend reads
        // the set. (refs #84)
        for g in &mut generics {
            let mut seen: Vec<TypePath> = Vec::new();
            g.bounds.retain(|b| {
                if seen.contains(b) {
                    false
                } else {
                    seen.push(b.clone());
                    true
                }
            });
        }
        generics
    }

    /// This signature with every associated-type projection in a parameter or
    /// return type replaced by its concrete binding (see [`Type::project_assoc`]).
    /// Applied at `impl`-method emit. (`STATIC.md` §3.4)
    #[must_use]
    pub fn project_assoc(&self, bindings: &[(Ident, Type)]) -> FnSig {
        self.map_types(&|t| t.project_assoc(bindings))
    }

    /// This signature with every associated-type projection erased to `top` (see
    /// [`Type::erase_assoc`]). Applied at the abstract `trait` declaration.
    /// (`STATIC.md` §3.4)
    #[must_use]
    pub fn erase_assoc(&self, top: &Type) -> FnSig {
        self.map_types(&|t| t.erase_assoc(top))
    }

    /// Whether any parameter or return type mentions a `Self::Name` projection.
    #[must_use]
    pub fn mentions_assoc(&self) -> bool {
        self.ret.as_ref().is_some_and(Type::mentions_assoc)
            || self.params.iter().any(|p| match p {
                Param::Receiver => false,
                Param::Named { ty, .. } => ty.mentions_assoc(),
            })
    }

    /// This signature with `f` applied to every parameter and return type.
    fn map_types(&self, f: &impl Fn(&Type) -> Type) -> FnSig {
        let mut sig = self.clone();
        for p in &mut sig.params {
            if let Param::Named { ty, .. } = p {
                *ty = f(ty);
            }
        }
        if let Some(ret) = &mut sig.ret {
            *ret = f(ret);
        }
        sig
    }
}

impl FnDecl {
    /// This declaration with associated-type projections in its signature
    /// replaced by their concrete bindings (see [`FnSig::project_assoc`]). The
    /// body is untouched — only the typed surface erases. (`STATIC.md` §3.4)
    #[must_use]
    pub fn project_assoc(&self, bindings: &[(Ident, Type)]) -> FnDecl {
        FnDecl {
            sig: self.sig.project_assoc(bindings),
            ..self.clone()
        }
    }
}

/// The trait names listed by every `#[derive(...)]` in `attrs`, in source order.
#[must_use]
pub fn derive_names(attrs: &[Attribute]) -> Vec<String> {
    let mut names = Vec::new();
    for attr in attrs {
        for item in &attr.items {
            if item.name != "derive" {
                continue;
            }
            for arg in &item.args {
                if let AttrArg::Item(inner) = arg {
                    names.push(inner.name.clone());
                }
            }
        }
    }
    names
}

/// Whether `attrs` carries a `#[test]` attribute (`lang/DYNAMIC.md` §6.5). Such a
/// function lowers normally and additionally registers itself on the module-level
/// `__tests` table the runtime walks (§7).
#[must_use]
pub fn is_test(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.items.iter().any(|item| item.name == "test"))
}

/// Maps each `struct`/`enum` to the set of traits it implements, transitively
/// closed over supertraits — the `__traits` runtime tag (`DYNAMIC.md`). A trait
/// impl (`impl Trait for Type`) and a `#[derive(Trait)]` both count; `trait A:
/// B` means a type implementing `A` also lists `B`. Types implementing no trait
/// are absent. The sets are sorted ([`BTreeSet`]) so codegen emits them
/// deterministically.
///
/// The supertrait closure is within this module: the `trait A: B` declarations
/// it can see. When `A` is implemented here but declared in another module
/// (with its `: B`), only `A` is recorded — closing that gap needs the
/// project-wide trait graph, which the runtime trait-enforcement work (#75)
/// must supply at its check site rather than relying on this per-module tag.
#[must_use]
pub fn trait_index(
    module: &Module,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    use std::collections::{BTreeMap, BTreeSet};

    // trait → its direct supertraits, by final path segment.
    let mut supertraits: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &module.items {
        if let ItemKind::Trait(t) = &item.kind {
            let ups = t
                .supertraits
                .iter()
                .filter_map(|p| p.segments.last().map(|s| s.name.clone()))
                .collect();
            supertraits.insert(t.name.clone(), ups);
        }
    }

    let mut index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Impl(b) => {
                if let (Some(trait_path), Type::Path(p)) = (&b.trait_, &b.ty) {
                    if let (Some(tr), Some(target)) =
                        (trait_path.segments.last(), p.segments.last())
                    {
                        add_trait(&mut index, &supertraits, &target.name, &tr.name);
                    }
                }
            }
            ItemKind::Struct(s) => {
                for d in derive_names(&item.attrs) {
                    add_trait(&mut index, &supertraits, &s.name, &d);
                }
            }
            ItemKind::Enum(e) => {
                for d in derive_names(&item.attrs) {
                    add_trait(&mut index, &supertraits, &e.name, &d);
                }
            }
            _ => {}
        }
    }
    index
}

/// Records that `ty` implements `trait_name`, then walks the supertrait chain so
/// the recorded set is transitively closed. Cycle-guarded via the set itself.
fn add_trait(
    index: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    supertraits: &std::collections::BTreeMap<String, Vec<String>>,
    ty: &str,
    trait_name: &str,
) {
    let set = index.entry(ty.to_string()).or_default();
    let mut stack = vec![trait_name.to_string()];
    while let Some(t) = stack.pop() {
        if set.insert(t.clone()) {
            if let Some(ups) = supertraits.get(&t) {
                stack.extend(ups.iter().cloned());
            }
        }
    }
}

// ---- types -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Unit,
    List(Box<Type>),
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
    },
    SelfType,
    Path(TypePath),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypePath {
    pub segments: Vec<TypeSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeSegment {
    pub name: Ident,
    pub args: Vec<Type>,
}

impl Type {
    /// The associated-type name when this type is a `Self::Name` projection.
    ///
    /// An associated type is spelled only through the `Self::` qualifier in a
    /// signature (see [`TraitDecl::assoc_types`]): a two-segment path led by a
    /// bare `Self`. A lone `Self` is [`Type::SelfType`], not this; a module path
    /// (`mod::Name`) leads with the module, never `Self`. (`STATIC.md` §3.4)
    #[must_use]
    pub fn assoc_projection(&self) -> Option<&str> {
        if let Type::Path(p) = self
            && let [head, tail] = p.segments.as_slice()
            && head.name == "Self"
            && head.args.is_empty()
            && tail.args.is_empty()
        {
            Some(tail.name.as_str())
        } else {
            None
        }
    }

    /// Rewrite every `Self::Name` projection through `replace`: a `Some(ty)`
    /// substitutes it, a `None` leaves it untouched. Recurses through lists,
    /// function types, and generic arguments. Pure — returns a fresh tree.
    fn rewrite_assoc(&self, replace: &impl Fn(&str) -> Option<Type>) -> Type {
        if let Some(name) = self.assoc_projection()
            && let Some(ty) = replace(name)
        {
            return ty;
        }
        match self {
            Type::List(inner) => Type::List(Box::new(inner.rewrite_assoc(replace))),
            Type::Fn { params, ret } => Type::Fn {
                params: params.iter().map(|p| p.rewrite_assoc(replace)).collect(),
                ret: ret.as_ref().map(|r| Box::new(r.rewrite_assoc(replace))),
            },
            Type::Path(p) => Type::Path(TypePath {
                segments: p
                    .segments
                    .iter()
                    .map(|s| TypeSegment {
                        name: s.name.clone(),
                        args: s.args.iter().map(|a| a.rewrite_assoc(replace)).collect(),
                    })
                    .collect(),
            }),
            Type::Unit | Type::SelfType => self.clone(),
        }
    }

    /// Replace every `Self::Name` projection with its concrete binding from an
    /// `impl`'s [`ImplBlock::assoc_bindings`]. A projection naming no binding is
    /// left as-is. Used at `impl`-method emit so `-> Self::Item` renders as the
    /// bound type, not a dangling associated name. (`STATIC.md` §3.4)
    #[must_use]
    pub fn project_assoc(&self, bindings: &[(Ident, Type)]) -> Type {
        self.rewrite_assoc(&|name| {
            bindings
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, ty)| ty.clone())
        })
    }

    /// Replace every `Self::Name` projection with `top` — the target's top type
    /// (`unknown` / `Any`). Used at the abstract `trait` declaration, where an
    /// associated type has no binding and so erases. (`STATIC.md` §3.4)
    #[must_use]
    pub fn erase_assoc(&self, top: &Type) -> Type {
        self.rewrite_assoc(&|_| Some(top.clone()))
    }

    /// Whether this type mentions a `Self::Name` projection anywhere.
    #[must_use]
    pub fn mentions_assoc(&self) -> bool {
        self.assoc_projection().is_some()
            || match self {
                Type::List(inner) => inner.mentions_assoc(),
                Type::Fn { params, ret } => {
                    params.iter().any(Type::mentions_assoc)
                        || ret.as_deref().is_some_and(Type::mentions_assoc)
                }
                Type::Path(p) => p
                    .segments
                    .iter()
                    .any(|s| s.args.iter().any(Type::mentions_assoc)),
                Type::Unit | Type::SelfType => false,
            }
    }
}

// ---- expressions -----------------------------------------------------------

/// An expression node: its [`ExprKind`] plus the byte [`Span`] it came from.
///
/// The checker reports diagnostics against the span of the expression it found
/// the problem in. Per [`Span`]'s `PartialEq`, two expressions with the same
/// kind compare equal regardless of span, so AST-equality tests are unaffected.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// Builds an expression node from its kind and the byte span it occupies in
    /// the source.
    #[must_use]
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
    Unit,
    /// A `::`-joined path: `x`, `Color::Red`, `module::f`.
    Path(Vec<Ident>),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    Try(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        /// Byte span of the method name at the call site, for member resolution.
        method_span: Span,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        field: FieldAccess,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    StructLit {
        path: Vec<Ident>,
        fields: Vec<FieldInit>,
        base: Option<Box<Expr>>,
    },
    List(Vec<Expr>),
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then: Block,
        /// A `Block` or another `If` (else-if chain).
        else_: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    While {
        cond: Box<Expr>,
        body: Block,
    },
    For {
        pattern: Pattern,
        iter: Box<Expr>,
        body: Block,
    },
    Loop {
        body: Block,
    },
    Closure {
        params: Vec<ClosureParam>,
        body: Box<Expr>,
    },
    Block(Block),
    Return(Option<Box<Expr>>),
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldAccess {
    /// A named field access `value.field`, with the byte span of the field name.
    Named(Ident, Span),
    /// Newtype / positional field: `value.0`.
    Tuple(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: Ident,
    /// `None` for field shorthand (`Point { x, y }`).
    pub value: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParam {
    pub name: Ident,
    pub ty: Option<Type>,
    /// The span of the parameter name — its binding site.
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// String concatenation. Not produced by the parser — `+` always parses as
    /// `Add`. The type checker's elaboration rewrites a string-typed `Add` to
    /// `Concat` so the emitter can pick Lua `..` over `+` without re-deriving
    /// types. (`lang/DYNAMIC.md` §4.2.)
    Concat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

// ---- statements ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The trailing expression that is the block's value, if any.
    pub value: Option<Box<Expr>>,
}

/// A statement node: its [`StmtKind`] plus the byte [`Span`] it came from.
///
/// As with [`Expr`], the span lets the checker locate a diagnostic precisely
/// while leaving AST-equality comparisons span-insensitive.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl Stmt {
    /// Builds a statement node from its kind and the byte span it occupies in
    /// the source.
    #[must_use]
    pub fn new(kind: StmtKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let(LetStmt),
    Item(Item),
    /// An expression evaluated for effect, terminated by `;`.
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    pub pattern: Pattern,
    pub ty: Option<Type>,
    pub value: Option<Expr>,
}

// ---- patterns --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Literal(LiteralPat),
    /// A lone identifier. The adapter leaves it as a binding; name resolution
    /// reinterprets it as a unit struct/variant pattern when it resolves to
    /// one (as Rust does). `span` is the binding site, for scope-aware
    /// resolution.
    Binding {
        name: Ident,
        span: Span,
    },
    /// A multi-segment path: a unit variant like `Color::Red`.
    Path(Vec<Ident>),
    Struct {
        path: Vec<Ident>,
        fields: Vec<FieldPat>,
        /// A trailing `..` ignoring the remaining fields.
        rest: bool,
    },
    Tuple {
        path: Vec<Ident>,
        elems: Vec<Pattern>,
    },
    Or(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralPat {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldPat {
    pub name: Ident,
    /// `None` for shorthand (`Point { x, y }`).
    pub pattern: Option<Pattern>,
    /// The span of the field name — the binding site for shorthand (`{ x }`).
    pub span: Span,
}

// ---- attributes ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub items: Vec<AttrItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttrItem {
    pub name: Ident,
    pub args: Vec<AttrArg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttrArg {
    Item(AttrItem),
    /// A Rust-faithful key-value argument: `name = value`, e.g.
    /// `#[link(name = "color")]`.
    Named {
        name: Ident,
        value: Box<AttrArg>,
    },
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_compare_equal_regardless_of_offset() {
        // Two spans with different offsets are `==`. AST nodes that carry a span
        // therefore compare structurally-equal regardless of position, which is
        // what keeps the parser's AST-equality tests stable.
        let a = Span { start: 0, end: 3 };
        let b = Span { start: 17, end: 42 };
        assert_eq!(a, b);

        // The same node with different spans compares equal too.
        let lhs = Expr::new(ExprKind::Int("1".into()), a);
        let rhs = Expr::new(ExprKind::Int("1".into()), b);
        assert_eq!(lhs, rhs);
    }

    #[test]
    fn effective_generics_dedups_redundant_inline_and_where_bound() {
        let bound = |n: &str| TypePath {
            segments: vec![TypeSegment {
                name: n.to_string(),
                args: Vec::new(),
            }],
        };
        let sig = |generics, where_clause| FnSig {
            name: "combine".to_string(),
            name_span: Span::default(),
            generics,
            params: Vec::new(),
            ret: None,
            where_clause,
        };

        // `<A: Doubler>(a: A) where A: Doubler` — the checker accepts the
        // redundant bound, but bounds are a set: the folded view is one
        // `Doubler`, not two. A dup would emit TS `extends Doubler & Doubler`
        // and slip the single-bound Python projector. (refs #84)
        let redundant = sig(
            vec![GenericParam {
                name: "A".to_string(),
                bounds: vec![bound("Doubler")],
            }],
            vec![WherePredicate {
                ty: "A".to_string(),
                bounds: vec![bound("Doubler")],
            }],
        );
        assert_eq!(
            redundant.effective_generics()[0].bounds,
            vec![bound("Doubler")]
        );

        // Dedup removes only repeats: a distinct bound survives, first-seen
        // order preserved even when the duplicate is non-consecutive.
        let mixed = sig(
            vec![GenericParam {
                name: "A".to_string(),
                bounds: vec![bound("Doubler")],
            }],
            vec![WherePredicate {
                ty: "A".to_string(),
                bounds: vec![bound("Tripler"), bound("Doubler")],
            }],
        );
        assert_eq!(
            mixed.effective_generics()[0].bounds,
            vec![bound("Doubler"), bound("Tripler")]
        );
    }

    #[test]
    fn assoc_projection_matches_only_self_qualified_paths() {
        let seg = |n: &str| TypeSegment {
            name: n.to_string(),
            args: Vec::new(),
        };
        let path = |segs: Vec<TypeSegment>| Type::Path(TypePath { segments: segs });

        // `Self::Item` — the sole spelling of an associated type — is recognised.
        assert_eq!(
            path(vec![seg("Self"), seg("Item")]).assoc_projection(),
            Some("Item")
        );
        // A lone `Self`, a bare name, and a module-qualified path are not.
        assert_eq!(Type::SelfType.assoc_projection(), None);
        assert_eq!(path(vec![seg("Item")]).assoc_projection(), None);
        assert_eq!(
            path(vec![seg("mods"), seg("Item")]).assoc_projection(),
            None
        );
    }

    #[test]
    fn project_assoc_substitutes_bindings_erase_replaces_with_top() {
        let seg = |n: &str| TypeSegment {
            name: n.to_string(),
            args: Vec::new(),
        };
        let path = |segs: Vec<TypeSegment>| Type::Path(TypePath { segments: segs });
        let number = || path(vec![seg("number")]);
        let item = || path(vec![seg("Self"), seg("Item")]);

        // Impl projection: `Self::Item` → its binding; nested through a list.
        let bindings = vec![("Item".to_string(), number())];
        assert_eq!(item().project_assoc(&bindings), number());
        assert_eq!(
            Type::List(Box::new(item())).project_assoc(&bindings),
            Type::List(Box::new(number()))
        );
        // An unbound projection (no binding) is left untouched.
        assert_eq!(item().project_assoc(&[]), item());

        // Trait erasure: `Self::Item` → the top type, others unchanged.
        let top = path(vec![seg("unknown")]);
        assert_eq!(item().erase_assoc(&top), top);
        assert_eq!(number().erase_assoc(&top), number());

        // `mentions_assoc` sees nested projections through a return list.
        let sig = FnSig {
            name: "peek".to_string(),
            name_span: Span::default(),
            generics: Vec::new(),
            params: vec![Param::Receiver],
            ret: Some(Type::List(Box::new(item()))),
            where_clause: Vec::new(),
        };
        assert!(sig.mentions_assoc());
        assert!(!sig.project_assoc(&bindings).mentions_assoc());
    }

    #[test]
    fn trait_index_closes_over_supertraits_and_derives() {
        let tpath = |n: &str| TypePath {
            segments: vec![TypeSegment {
                name: n.to_string(),
                args: Vec::new(),
            }],
        };
        let item = |kind, attrs| Item {
            attrs,
            vis: Visibility::Private,
            kind,
            span: Span::default(),
        };
        let trait_decl = |name: &str, supers: Vec<TypePath>| {
            ItemKind::Trait(TraitDecl {
                name: name.to_string(),
                generics: Vec::new(),
                supertraits: supers,
                assoc_types: Vec::new(),
                items: Vec::new(),
            })
        };
        let derive_clone = Attribute {
            items: vec![AttrItem {
                name: "derive".to_string(),
                args: vec![AttrArg::Item(AttrItem {
                    name: "Clone".to_string(),
                    args: Vec::new(),
                })],
            }],
        };
        let module = Module {
            items: vec![
                item(trait_decl("Show", Vec::new()), Vec::new()),
                item(trait_decl("Greet", vec![tpath("Show")]), Vec::new()),
                item(
                    ItemKind::Struct(StructDecl {
                        name: "P".to_string(),
                        generics: Vec::new(),
                        body: StructBody::Unit,
                    }),
                    vec![derive_clone],
                ),
                item(
                    ItemKind::Impl(ImplBlock {
                        generics: Vec::new(),
                        trait_: Some(tpath("Greet")),
                        ty: Type::Path(tpath("P")),
                        items: Vec::new(),
                        assoc_bindings: Vec::new(),
                    }),
                    Vec::new(),
                ),
            ],
        };
        let index = trait_index(&module);
        // `impl Greet for P` pulls in `Greet` and its supertrait `Show`;
        // `#[derive(Clone)]` adds `Clone`. Sorted by the `BTreeSet`.
        let p: Vec<&str> = index["P"].iter().map(String::as_str).collect();
        assert_eq!(p, vec!["Clone", "Greet", "Show"]);
    }

    /// An `extern` attribute with a single `#[extern(...)]` item.
    fn extern_attr(args: Vec<AttrArg>) -> Attribute {
        Attribute {
            items: vec![AttrItem {
                name: "extern".to_string(),
                args,
            }],
        }
    }

    fn extern_item(attrs: Vec<Attribute>, kind: ExternItemKind) -> ExternItem {
        ExternItem {
            attrs,
            vis: Visibility::Public,
            kind,
            span: Span::default(),
        }
    }

    #[test]
    fn extern_attr_parses_the_closed_param_set() {
        let attrs = vec![extern_attr(vec![
            AttrArg::Item(AttrItem {
                name: "ambient".to_string(),
                args: Vec::new(),
            }),
            AttrArg::Named {
                name: "path".to_string(),
                value: Box::new(AttrArg::Str("globalThis.crypto".to_string())),
            },
        ])];
        let parsed = ExternAttr::parse(&attrs).expect("extern attr present");
        assert!(parsed.ambient);
        assert_eq!(parsed.path.as_deref(), Some("globalThis.crypto"));
        assert!(parsed.name.is_none());
        assert!(!parsed.constructor);
    }

    #[test]
    fn extern_imports_compute_path_name_and_ambient() {
        // extern {
        //   #[extern(path = "vnd:rusqlite")] mod rusqlite { ... }
        //   #[extern(ambient)] mod console { ... }
        //   #[extern(path = "fs")] mod files { ... }
        // }
        let module = Module {
            items: vec![Item {
                attrs: Vec::new(),
                vis: Visibility::Private,
                kind: ItemKind::Extern(ExternBlock {
                    items: vec![
                        extern_item(
                            vec![extern_attr(vec![AttrArg::Named {
                                name: "path".to_string(),
                                value: Box::new(AttrArg::Str("vnd:rusqlite".to_string())),
                            }])],
                            ExternItemKind::Mod(ExternMod {
                                name: "rusqlite".to_string(),
                                items: Vec::new(),
                            }),
                        ),
                        extern_item(
                            vec![extern_attr(vec![AttrArg::Item(AttrItem {
                                name: "ambient".to_string(),
                                args: Vec::new(),
                            })])],
                            ExternItemKind::Mod(ExternMod {
                                name: "console".to_string(),
                                items: Vec::new(),
                            }),
                        ),
                        extern_item(
                            vec![extern_attr(vec![AttrArg::Named {
                                name: "path".to_string(),
                                value: Box::new(AttrArg::Str("fs".to_string())),
                            }])],
                            ExternItemKind::Mod(ExternMod {
                                name: "files".to_string(),
                                items: Vec::new(),
                            }),
                        ),
                    ],
                }),
                span: Span::default(),
            }],
        };
        let imports = extern_imports(&module, Target::Lua);
        assert_eq!(
            imports,
            vec![
                ExternImport {
                    local_name: "rusqlite".to_string(),
                    path: "vnd:rusqlite".to_string(),
                    ambient: false,
                },
                ExternImport {
                    local_name: "console".to_string(),
                    path: "console".to_string(),
                    ambient: true,
                },
                ExternImport {
                    local_name: "files".to_string(),
                    path: "fs".to_string(),
                    ambient: false,
                },
            ]
        );
    }

    #[test]
    fn extern_struct_names_and_constructors_are_collected() {
        // extern {
        //   struct Database;
        //   impl Database { #[extern(constructor)] fn new(p: string); fn q(self); }
        // }
        let module = Module {
            items: vec![Item {
                attrs: Vec::new(),
                vis: Visibility::Private,
                kind: ItemKind::Extern(ExternBlock {
                    items: vec![
                        extern_item(
                            Vec::new(),
                            ExternItemKind::Struct(StructDecl {
                                name: "Database".to_string(),
                                generics: Vec::new(),
                                body: StructBody::Unit,
                            }),
                        ),
                        extern_item(
                            Vec::new(),
                            ExternItemKind::Impl(ImplBlock {
                                generics: Vec::new(),
                                trait_: None,
                                ty: Type::Path(TypePath {
                                    segments: vec![TypeSegment {
                                        name: "Database".to_string(),
                                        args: Vec::new(),
                                    }],
                                }),
                                items: vec![
                                    FnDecl {
                                        attrs: vec![extern_attr(vec![AttrArg::Item(AttrItem {
                                            name: "constructor".to_string(),
                                            args: Vec::new(),
                                        })])],
                                        sig: FnSig {
                                            name: "new".to_string(),
                                            name_span: Span::default(),
                                            generics: Vec::new(),
                                            params: vec![Param::Named {
                                                name: "p".to_string(),
                                                ty: Type::Path(TypePath {
                                                    segments: vec![TypeSegment {
                                                        name: "string".to_string(),
                                                        args: Vec::new(),
                                                    }],
                                                }),
                                                span: Span::default(),
                                            }],
                                            ret: None,
                                            where_clause: Vec::new(),
                                        },
                                        body: Block::default(),
                                    },
                                    FnDecl {
                                        attrs: Vec::new(),
                                        sig: FnSig {
                                            name: "q".to_string(),
                                            name_span: Span::default(),
                                            generics: Vec::new(),
                                            params: vec![Param::Receiver],
                                            ret: None,
                                            where_clause: Vec::new(),
                                        },
                                        body: Block::default(),
                                    },
                                ],
                                assoc_bindings: Vec::new(),
                            }),
                        ),
                    ],
                }),
                span: Span::default(),
            }],
        };
        assert!(extern_struct_names(&module).contains("Database"));
        let ctors = extern_constructors(&module);
        assert!(ctors.contains(&("Database".to_string(), "new".to_string())));
        assert_eq!(ctors.len(), 1);
    }

    #[test]
    fn extern_struct_namespaces_map_in_mod_structs_to_their_mod_binding() {
        // extern {
        //   struct Top;                                  // no enclosing mod
        //   #[extern(path = "bun:sqlite")]
        //   mod sqlite { struct Database; struct Changes; }
        // }
        let struct_item = |name: &str| {
            extern_item(
                Vec::new(),
                ExternItemKind::Struct(StructDecl {
                    name: name.to_string(),
                    generics: Vec::new(),
                    body: StructBody::Unit,
                }),
            )
        };
        let module = Module {
            items: vec![Item {
                attrs: Vec::new(),
                vis: Visibility::Private,
                kind: ItemKind::Extern(ExternBlock {
                    items: vec![
                        struct_item("Top"),
                        extern_item(
                            vec![extern_attr(vec![AttrArg::Named {
                                name: "path".to_string(),
                                value: Box::new(AttrArg::Str("bun:sqlite".to_string())),
                            }])],
                            ExternItemKind::Mod(ExternMod {
                                name: "sqlite".to_string(),
                                items: vec![struct_item("Database"), struct_item("Changes")],
                            }),
                        ),
                    ],
                }),
                span: Span::default(),
            }],
        };
        let ns = extern_struct_namespaces(&module);
        // The path overrides the import specifier, not the neoc-side binding: a
        // struct's value qualifies through the `mod`'s name (`sqlite`).
        assert_eq!(ns.get("Database"), Some(&"sqlite".to_string()));
        assert_eq!(ns.get("Changes"), Some(&"sqlite".to_string()));
        // A struct with no enclosing `mod` keeps its bare name — absent here.
        assert!(!ns.contains_key("Top"));
    }

    #[test]
    fn extern_fn_namespaces_map_in_mod_fns_to_their_mod_binding() {
        // extern {
        //   fn top();                                     // no enclosing mod
        //   #[extern(ambient, path = "Bun")]
        //   mod Bun { fn serve(o: Unknown); fn sleep(ms: Unknown); }
        // }
        let fn_item = |name: &str| {
            extern_item(
                Vec::new(),
                ExternItemKind::Fn(FnSig {
                    name: name.to_string(),
                    name_span: Span::default(),
                    generics: Vec::new(),
                    params: vec![Param::Named {
                        name: "o".to_string(),
                        ty: Type::Unit,
                        span: Span::default(),
                    }],
                    ret: None,
                    where_clause: Vec::new(),
                }),
            )
        };
        let module = Module {
            items: vec![Item {
                attrs: Vec::new(),
                vis: Visibility::Private,
                kind: ItemKind::Extern(ExternBlock {
                    items: vec![
                        fn_item("top"),
                        extern_item(
                            vec![extern_attr(vec![
                                AttrArg::Item(AttrItem {
                                    name: "ambient".to_string(),
                                    args: Vec::new(),
                                }),
                                AttrArg::Named {
                                    name: "path".to_string(),
                                    value: Box::new(AttrArg::Str("Bun".to_string())),
                                },
                            ])],
                            ExternItemKind::Mod(ExternMod {
                                name: "Bun".to_string(),
                                items: vec![fn_item("serve"), fn_item("sleep")],
                            }),
                        ),
                    ],
                }),
                span: Span::default(),
            }],
        };
        let ns = extern_fn_namespaces(&module);
        // A fn inside a `mod` qualifies through the `mod`'s name (`Bun`).
        assert_eq!(ns.get("serve"), Some(&"Bun".to_string()));
        assert_eq!(ns.get("sleep"), Some(&"Bun".to_string()));
        // A fn with no enclosing `mod` keeps its bare name — absent here.
        assert!(!ns.contains_key("top"));
    }

    #[test]
    fn imported_extern_items_keeps_the_named_struct_its_mod_and_its_constructor() {
        // extern {
        //   #[extern(path = "bun:sqlite")]
        //   mod sqlite {
        //     struct Database;
        //     struct Other;
        //     impl Database { #[extern(constructor)] fn new(p: string); }
        //   }
        // }
        let new_sig = FnDecl {
            attrs: vec![extern_attr(vec![AttrArg::Item(AttrItem {
                name: "constructor".to_string(),
                args: Vec::new(),
            })])],
            sig: FnSig {
                name: "new".to_string(),
                name_span: Span::default(),
                generics: Vec::new(),
                params: vec![Param::Named {
                    name: "p".to_string(),
                    ty: Type::Unit,
                    span: Span::default(),
                }],
                ret: None,
                where_clause: Vec::new(),
            },
            body: Block::default(),
        };
        let struct_item = |name: &str| {
            extern_item(
                Vec::new(),
                ExternItemKind::Struct(StructDecl {
                    name: name.to_string(),
                    generics: Vec::new(),
                    body: StructBody::Unit,
                }),
            )
        };
        let module = Module {
            items: vec![Item {
                attrs: Vec::new(),
                vis: Visibility::Private,
                kind: ItemKind::Extern(ExternBlock {
                    items: vec![extern_item(
                        vec![extern_attr(vec![AttrArg::Named {
                            name: "path".to_string(),
                            value: Box::new(AttrArg::Str("bun:sqlite".to_string())),
                        }])],
                        ExternItemKind::Mod(ExternMod {
                            name: "sqlite".to_string(),
                            items: vec![
                                struct_item("Database"),
                                struct_item("Other"),
                                extern_item(
                                    Vec::new(),
                                    ExternItemKind::Impl(ImplBlock {
                                        generics: Vec::new(),
                                        trait_: None,
                                        ty: Type::Path(TypePath {
                                            segments: vec![TypeSegment {
                                                name: "Database".to_string(),
                                                args: Vec::new(),
                                            }],
                                        }),
                                        items: vec![new_sig],
                                        assoc_bindings: Vec::new(),
                                    }),
                                ),
                            ],
                        }),
                    )],
                }),
                span: Span::default(),
            }],
        };

        // `use db::Database` → extract only `Database`, but carry its enclosing
        // `mod` (for the import path) and the `impl` that holds its constructor.
        let names = std::collections::HashSet::from(["Database".to_string()]);
        let kept = imported_extern_items(&module, &names);

        // The native import survives (path metadata carried via the kept `mod`).
        let imports = extern_imports_from_items(&kept, Target::Js);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "bun:sqlite");
        assert_eq!(imports[0].local_name, "sqlite");

        // The struct and its constructor carry; the un-named sibling `Other`
        // does not.
        let structs = extern_struct_names_from_items(&kept);
        assert!(structs.contains("Database"));
        assert!(!structs.contains("Other"));
        let ctors = extern_constructors_from_items(&kept);
        assert!(ctors.contains(&("Database".to_string(), "new".to_string())));
        let ns = extern_struct_namespaces_from_items(&kept);
        assert_eq!(ns.get("Database"), Some(&"sqlite".to_string()));
    }
}
