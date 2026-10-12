//! The symbol table and lexical scope stack.
//!
//! Two namespaces per `STATIC.md` §2: the *type* namespace (structs, enums,
//! traits, aliases, type parameters, primitives, `ext type`) and the *value*
//! namespace (functions, constants, bindings, constructors, `ext` bindings).
//! Module-level declarations are collected up front so forward references
//! resolve; block-level `let` bindings push and pop a scope stack.

use std::collections::{HashMap, HashSet};

use crate::ty::Ty;

/// A collected `struct` declaration.
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub generics: Vec<String>,
    pub fields: StructFields,
}

/// The three struct shapes (`SYNTAX.md` §3.4).
#[derive(Debug, Clone)]
pub enum StructFields {
    /// `struct Marker;`
    Unit,
    /// `struct UserId(number);` — one positional field accessed as `.0`.
    Tuple(Ty),
    /// `struct Point { x: number, y: number }`
    Named(Vec<(String, Ty)>),
}

/// A collected `enum` declaration.
#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub generics: Vec<String>,
    pub variants: Vec<VariantInfo>,
}

/// One variant of an enum.
#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: String,
    pub fields: VariantFields,
}

/// The three variant shapes (`SYNTAX.md` §3.5).
#[derive(Debug, Clone)]
pub enum VariantFields {
    Unit,
    Tuple(Vec<Ty>),
    Named(Vec<(String, Ty)>),
}

/// A collected `trait` declaration.
#[derive(Debug, Clone)]
pub struct TraitInfo {
    pub generics: Vec<String>,
    pub methods: Vec<TraitMethod>,
    /// The supertraits this trait requires (`trait A: B + C`), each applied to
    /// arguments written in terms of this trait's generics (`trait Sub<T>:
    /// Base<T>` → `Base<T>`). An implementor of this trait must also implement
    /// each, and a bound by this trait grants access to their methods.
    pub supertraits: Vec<TraitRef>,
    /// Names of the associated types this trait declares (`type Item;`). Each
    /// must be bound by every implementor, and trait methods may refer to them
    /// as `Self::Item`.
    pub assoc_types: Vec<String>,
}

/// A trait applied to type arguments: `Source<number>` is the trait `Source`
/// applied to `[number]`. A non-generic trait applies to no arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitRef {
    pub name: String,
    pub args: Vec<Ty>,
}

impl TraitRef {
    /// The trait `name` applied to no arguments — a non-generic trait, or a
    /// generic one whose arguments are not recorded (a bound, until bounds
    /// carry them). Unapplied parameters read as themselves.
    #[must_use]
    pub fn bare(name: impl Into<String>) -> TraitRef {
        TraitRef {
            name: name.into(),
            args: Vec::new(),
        }
    }

    /// The trait type this application names when a trait is used as a type
    /// (`Source<number>`).
    #[must_use]
    pub fn to_ty(&self) -> Ty {
        Ty::Named {
            name: self.name.clone(),
            args: self.args.clone(),
        }
    }

    /// Reads every type parameter in `generics` as the matching argument in
    /// `args` throughout this reference's own arguments.
    #[must_use]
    pub fn instantiate(&self, generics: &[String], args: &[Ty]) -> TraitRef {
        TraitRef {
            name: self.name.clone(),
            args: self
                .args
                .iter()
                .map(|a| a.subst_params(generics, args))
                .collect(),
        }
    }
}

impl std::fmt::Display for TraitRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_ty())
    }
}

/// One method signature of a trait.
#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    /// The method's own type parameters (`fn conv<U>(…)`), distinct from the
    /// trait's.
    pub generics: Vec<String>,
    /// Parameter types excluding any `self` receiver.
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub has_receiver: bool,
    pub has_default: bool,
}

/// What a type name resolves to in the type namespace.
#[derive(Debug, Clone)]
pub enum TypeDef {
    Struct(StructInfo),
    Enum(EnumInfo),
    Trait(TraitInfo),
    /// A transparent `type` alias.
    Alias {
        generics: Vec<String>,
        value: Ty,
    },
    /// An opaque `ext type`.
    Opaque,
}

/// An inherent or trait-impl method attached to a type.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub has_receiver: bool,
}

/// The collected declarations of a module: the persistent half of `Γ`.
#[derive(Debug, Default)]
pub struct SymbolTable {
    /// Type namespace: name → definition.
    pub types: HashMap<String, TypeDef>,
    /// Value namespace: name → type (functions, consts, constructors, `ext`).
    pub values: HashMap<String, Ty>,
    /// Methods keyed by `(type name, method name)`.
    pub methods: HashMap<(String, String), MethodInfo>,
    /// Value-namespace names this module declares `pub`. A cross-module `use`
    /// is only allowed to import a name listed here (`STATIC.md` §2).
    pub pub_values: HashSet<String>,
    /// Type-namespace names this module declares `pub`.
    pub pub_types: HashSet<String>,
    /// Trait implementations, indexed by implementing type: type name → the set
    /// of traits it implements, one entry per `impl Trait for Type` and per
    /// `#[derive(Trait)]`. Keyed by head (the type) so the checker answers "does
    /// this type satisfy a bound" with a lookup rather than a scan of every
    /// impl (`STATIC.md` §3.5).
    pub trait_impls: HashMap<String, HashSet<String>>,
    /// The associated-type bindings (`type Item = …`) of each concrete
    /// `impl Trait for Type`, keyed `(type, trait)`. Lets an inherited default
    /// method's `Self::Item` project to the type's binding (`STATIC.md` §8).
    pub impl_assoc: HashMap<(String, String), HashMap<String, Ty>>,
    /// The application of each concrete `impl Trait<A, …> for Type<…>`, keyed
    /// `(type name, trait name)`. Lets a value of `Type` flow into the trait
    /// type `Trait<A, …>` only when the impl applies the trait to those same
    /// arguments (`STATIC.md` §3.4). Absent for a non-generic trait.
    pub trait_impl_applications: HashMap<(String, String), ImplApplication>,
    /// Per-function declared bounds: function name → (type parameter → the
    /// trait names bounding it), from both `<T: …>` and `where T: …`. Lets a
    /// call site enforce that each argument's concrete type satisfies the bound
    /// the callee declared.
    pub fn_bounds: HashMap<String, HashMap<String, Vec<String>>>,
    /// Blanket impls (`impl<T: Show> Describe for T`): every type satisfying
    /// the conditions also implements the trait (`STATIC.md` §3.5). Kept apart
    /// from `trait_impls` because the target is a parameter, not a concrete
    /// type — satisfaction is queried on demand against the conditions.
    pub blanket_impls: Vec<BlanketImpl>,
    /// Byte span of each member's declaration, keyed `(owning type or trait,
    /// member name)`. Covers struct fields and impl/trait methods; lets the
    /// checker record a `recv.member` use as resolving to its definition, for
    /// goto/hover/references (member resolution).
    pub member_defs: HashMap<(String, String), (usize, usize)>,
}

/// How a concrete `impl<P…> Trait<A…> for Type<B…>` applies its trait: the
/// trait arguments `A…`, written in terms of the impl's generics `P…`, which
/// are fixed by matching the target's arguments `B…` against a value's type.
#[derive(Debug, Clone)]
pub struct ImplApplication {
    pub generics: Vec<String>,
    pub target_args: Vec<Ty>,
    pub trait_args: Vec<Ty>,
}

/// A generic (blanket) trait implementation `impl<T: Conditions> Trait for T`.
#[derive(Debug, Clone)]
pub struct BlanketImpl {
    /// The impl's generic parameter that is the target type (`T`).
    pub param: String,
    /// Traits the target must satisfy for the impl to apply (the bounds on
    /// `param`); empty means it applies to every type.
    pub conditions: Vec<String>,
    /// The trait this blanket provides for every satisfying type, applied to
    /// arguments written in terms of `param` (`impl<T: Show> Source<number>
    /// for T` → `Source<number>`).
    pub trait_ref: TraitRef,
    /// The methods the blanket provides itself — required ones and overridden
    /// defaults. Every other default of the trait is inherited.
    pub methods: Vec<String>,
    /// The blanket's associated-type bindings (`type Item = …`), written in
    /// terms of `param`. Carried so a satisfying type's call site can project a
    /// method's `Self::Item` to the bound type, with `param` read as that type.
    pub assoc_bindings: HashMap<String, Ty>,
}

impl SymbolTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a value-namespace name is `pub` (visible to other modules). The
    /// prelude constructors (`Ok`, `Err`, `Some`, `None`) are always visible.
    #[must_use]
    pub fn value_is_public(&self, name: &str) -> bool {
        self.pub_values.contains(name)
    }

    /// Whether a type-namespace name is `pub` (visible to other modules).
    #[must_use]
    pub fn type_is_public(&self, name: &str) -> bool {
        self.pub_types.contains(name)
    }

    /// Whether `type_name` has an implementation of `trait_name` — an explicit
    /// `impl Trait for Type` or a `#[derive(Trait)]`.
    #[must_use]
    pub fn implements(&self, type_name: &str, trait_name: &str) -> bool {
        self.trait_impls
            .get(type_name)
            .is_some_and(|traits| traits.contains(trait_name))
    }

    /// Imports the symbol `name` from `source` into this table under `binding`.
    ///
    /// Copies whichever namespaces `source` defines `name` in: a value (a `fn`,
    /// `const`, or constructor), a type (`struct`/`enum`/`trait`/alias), and any
    /// methods attached to a type of that name. The imported symbol is bound
    /// under `binding` (the `use ... as` alias, or the item name itself). The
    /// import is *not* itself re-exported — it does not enter this table's
    /// visibility sets.
    ///
    /// Returns `true` if at least one namespace carried the name; `false` means
    /// `source` has no symbol called `name` (caller reports an unresolved
    /// import).
    pub fn import_from(&mut self, source: &SymbolTable, name: &str, binding: &str) -> bool {
        let mut found = false;
        if let Some(ty) = source.values.get(name) {
            self.values.insert(binding.to_string(), ty.clone());
            found = true;
        }
        if let Some(def) = source.types.get(name) {
            self.types.insert(binding.to_string(), def.clone());
            for ((ty_name, method), info) in &source.methods {
                if ty_name == name {
                    self.methods
                        .insert((binding.to_string(), method.clone()), info.clone());
                }
            }
            found = true;
        }
        found
    }

    /// Pulls in the *structure* of every type the `use`-imported symbols in
    /// `imports` reference but this table does not yet define, drawing from
    /// `universe`, transitively.
    ///
    /// A multi-file driver uses this after resolving a module's `use` imports.
    /// When module `c` imports a function `make` whose return type is a `struct`
    /// `Config` declared in module `a` — but `c` never names `Config` in a `use`
    /// — field and method access on a `Config` value (`c.level`) needs the
    /// type's shape. The direct import brings in `make`'s signature; this brings
    /// in the shapes that signature (and theirs, recursively) refer to.
    ///
    /// The worklist is seeded **only from the imported signatures** in
    /// `imports`, never from this table's own declarations. A type the focus
    /// module names *directly* — a field type, a constructed value — is therefore
    /// not satisfied here: it must be brought in by its own `use` or stay
    /// unresolved, exactly as the single-file and build paths require. Only types
    /// reached *transitively through* an imported signature are pulled. (The build
    /// path's `universe` is already `use`-scoped, so seeding from own declarations
    /// happened to be harmless there; an editor `universe` spans every workspace
    /// module, where it would silently resolve un-`use`d names.)
    ///
    /// Only the type namespace and its methods are copied, not the visibility
    /// sets: a transitively-known type is resolvable, not re-exported. The
    /// `universe` is the project-wide pool of `pub` type definitions; a type
    /// name is taken to be project-unique (neoc has no module-qualified type
    /// identity), so a name already defined here is left untouched.
    ///
    /// # Arguments
    ///
    /// * `imports` — a table holding only the symbols this module brought in via
    ///   `use` (values, types, methods). Its signatures seed the worklist. The
    ///   caller owns it; this method borrows it for the call.
    /// * `universe` — the aggregate of every module's `pub` type definitions and
    ///   their methods. The caller owns it; this table borrows it for the call.
    pub fn import_referenced_types(&mut self, imports: &SymbolTable, universe: &SymbolTable) {
        let mut pending: Vec<String> = Vec::new();
        for ty in imports.values.values() {
            collect_ty_names(ty, &mut pending);
        }
        for def in imports.types.values() {
            collect_typedef_names(def, &mut pending);
        }
        for info in imports.methods.values() {
            collect_method_names(info, &mut pending);
        }
        while let Some(name) = pending.pop() {
            if self.types.contains_key(&name) {
                continue;
            }
            let Some(def) = universe.types.get(&name) else {
                continue;
            };
            self.types.insert(name.clone(), def.clone());
            collect_typedef_names(def, &mut pending);
            for ((ty_name, method), info) in &universe.methods {
                if ty_name == &name {
                    self.methods
                        .insert((name.clone(), method.clone()), info.clone());
                    collect_method_names(info, &mut pending);
                }
            }
        }
    }

    /// Expands every transparent `type` alias stored in the table, so that
    /// downstream comparisons see underlying types. Run once after collection,
    /// when all aliases are known (forward references are allowed).
    pub fn normalize_aliases(&mut self) {
        let snapshot = Self {
            types: self.types.clone(),
            ..Self::default()
        };
        for ty in self.values.values_mut() {
            *ty = snapshot.resolve_alias_ty(ty.clone());
        }
        for m in self.methods.values_mut() {
            m.params = m
                .params
                .drain(..)
                .map(|p| snapshot.resolve_alias_ty(p))
                .collect();
            m.ret = snapshot.resolve_alias_ty(m.ret.clone());
        }
        for app in self.trait_impl_applications.values_mut() {
            normalize_tys(&mut app.target_args, &snapshot);
            normalize_tys(&mut app.trait_args, &snapshot);
        }
        for bl in &mut self.blanket_impls {
            normalize_tys(&mut bl.trait_ref.args, &snapshot);
        }
        let names: Vec<String> = self.types.keys().cloned().collect();
        for name in names {
            if let Some(def) = self.types.get_mut(&name) {
                normalize_typedef(def, &snapshot);
            }
        }
    }

    /// Recursively expands aliases inside a type. A generic alias applied to
    /// the right number of arguments is expanded with its parameters replaced
    /// by those arguments (`Pair<number>` with `type Pair<T> = [T]` is
    /// `[number]`). An application with the wrong number of arguments, or one
    /// that re-enters an alias already being expanded, expands to [`Ty::Unknown`]
    /// so the misuse stays a single diagnostic — the checker reports it
    /// ([`Self::alias_arity_errors`], [`Self::alias_is_recursive`]).
    #[must_use]
    pub fn resolve_alias_ty(&self, ty: Ty) -> Ty {
        self.expand_aliases(ty, &mut Vec::new())
    }

    /// [`Self::resolve_alias_ty`] with the stack of aliases currently being
    /// expanded, so a recursive alias terminates.
    fn expand_aliases(&self, ty: Ty, active: &mut Vec<String>) -> Ty {
        match ty {
            Ty::Named { name, args } => {
                let args: Vec<Ty> = args
                    .into_iter()
                    .map(|a| self.expand_aliases(a, active))
                    .collect();
                if let Some(TypeDef::Alias { generics, value }) = self.types.get(&name) {
                    if generics.len() != args.len() || active.contains(&name) {
                        return Ty::Unknown;
                    }
                    let body = value.map(&|t| match t {
                        Ty::Param(p) => generics
                            .iter()
                            .position(|g| g == p)
                            .map(|i| args[i].clone()),
                        _ => None,
                    });
                    active.push(name);
                    let expanded = self.expand_aliases(body, active);
                    active.pop();
                    return expanded;
                }
                Ty::Named { name, args }
            }
            Ty::List(inner) => Ty::List(Box::new(self.expand_aliases(*inner, active))),
            Ty::Fn(params, ret) => Ty::Fn(
                params
                    .into_iter()
                    .map(|p| self.expand_aliases(p, active))
                    .collect(),
                Box::new(self.expand_aliases(*ret, active)),
            ),
            other => other,
        }
    }

    /// Whether the alias `name` reaches itself through its own body, directly
    /// or through other aliases.
    #[must_use]
    pub(crate) fn alias_is_recursive(&self, name: &str) -> bool {
        let mut seen = Vec::new();
        let mut pending = vec![name.to_string()];
        while let Some(current) = pending.pop() {
            let Some(TypeDef::Alias { value, .. }) = self.types.get(&current) else {
                continue;
            };
            let mut names = Vec::new();
            collect_ty_names(value, &mut names);
            for n in names {
                if n == name {
                    return true;
                }
                if !seen.contains(&n) {
                    seen.push(n.clone());
                    pending.push(n);
                }
            }
        }
        false
    }

    /// One message per application in `ty` of a generic `type` alias to a
    /// different number of type arguments than it declares (`STATIC.md` §4.3).
    /// `ty` is lowered but not yet alias-expanded, so an in-scope type
    /// parameter or a `Self::Item` projection that shares an alias's name is
    /// not an application of it.
    #[must_use]
    pub(crate) fn alias_arity_errors(&self, ty: &Ty) -> Vec<String> {
        let mut out = Vec::new();
        ty.for_each_named(&mut |name, args| {
            if let Some(TypeDef::Alias { generics, .. }) = self.types.get(name)
                && generics.len() != args.len()
            {
                out.push(format!(
                    "type alias `{name}` expects {} type argument(s), found {}",
                    generics.len(),
                    args.len()
                ));
            }
        });
        out
    }
}

/// Pushes every nominal type name a [`Ty`] mentions onto `out` (recursing
/// through lists, function params/returns, and type arguments). Primitives,
/// `Unit`, type parameters, and `Unknown` carry no name.
fn collect_ty_names(ty: &Ty, out: &mut Vec<String>) {
    ty.for_each_named(&mut |name, _| out.push(name.to_string()));
}

/// Pushes every type name referenced by a definition's fields, variants, alias
/// target, or trait-method signatures onto `out`.
fn collect_typedef_names(def: &TypeDef, out: &mut Vec<String>) {
    match def {
        TypeDef::Struct(s) => collect_struct_field_names(&s.fields, out),
        TypeDef::Enum(e) => {
            for v in &e.variants {
                collect_variant_field_names(&v.fields, out);
            }
        }
        TypeDef::Trait(t) => {
            for m in &t.methods {
                for p in &m.params {
                    collect_ty_names(p, out);
                }
                collect_ty_names(&m.ret, out);
            }
        }
        TypeDef::Alias { value, .. } => collect_ty_names(value, out),
        TypeDef::Opaque => {}
    }
}

/// Pushes the type names referenced by a method's parameter and return types.
fn collect_method_names(info: &MethodInfo, out: &mut Vec<String>) {
    for p in &info.params {
        collect_ty_names(p, out);
    }
    collect_ty_names(&info.ret, out);
}

fn collect_struct_field_names(fields: &StructFields, out: &mut Vec<String>) {
    match fields {
        StructFields::Unit => {}
        StructFields::Tuple(ty) => collect_ty_names(ty, out),
        StructFields::Named(fs) => {
            for (_, ty) in fs {
                collect_ty_names(ty, out);
            }
        }
    }
}

fn collect_variant_field_names(fields: &VariantFields, out: &mut Vec<String>) {
    match fields {
        VariantFields::Unit => {}
        VariantFields::Tuple(tys) => {
            for ty in tys {
                collect_ty_names(ty, out);
            }
        }
        VariantFields::Named(fs) => {
            for (_, ty) in fs {
                collect_ty_names(ty, out);
            }
        }
    }
}

/// Expands aliases inside the field/variant types of a single definition.
fn normalize_typedef(def: &mut TypeDef, table: &SymbolTable) {
    match def {
        TypeDef::Struct(s) => normalize_struct_fields(&mut s.fields, table),
        TypeDef::Enum(e) => {
            for v in &mut e.variants {
                normalize_variant_fields(&mut v.fields, table);
            }
        }
        TypeDef::Trait(t) => {
            for m in &mut t.methods {
                m.params = m
                    .params
                    .drain(..)
                    .map(|p| table.resolve_alias_ty(p))
                    .collect();
                m.ret = table.resolve_alias_ty(m.ret.clone());
            }
            for sup in &mut t.supertraits {
                normalize_tys(&mut sup.args, table);
            }
        }
        TypeDef::Alias { .. } | TypeDef::Opaque => {}
    }
}

fn normalize_tys(tys: &mut [Ty], table: &SymbolTable) {
    for ty in tys {
        *ty = table.resolve_alias_ty(ty.clone());
    }
}

fn normalize_struct_fields(fields: &mut StructFields, table: &SymbolTable) {
    match fields {
        StructFields::Unit => {}
        StructFields::Tuple(ty) => *ty = table.resolve_alias_ty(ty.clone()),
        StructFields::Named(fs) => {
            for (_, ty) in fs.iter_mut() {
                *ty = table.resolve_alias_ty(ty.clone());
            }
        }
    }
}

fn normalize_variant_fields(fields: &mut VariantFields, table: &SymbolTable) {
    match fields {
        VariantFields::Unit => {}
        VariantFields::Tuple(tys) => {
            *tys = tys.drain(..).map(|t| table.resolve_alias_ty(t)).collect();
        }
        VariantFields::Named(fs) => {
            for (_, ty) in fs.iter_mut() {
                *ty = table.resolve_alias_ty(ty.clone());
            }
        }
    }
}

/// An opaque identity for one binding occurrence (one `let`/param/closure
/// param/pattern binding), unique within a [`Scopes`]. Two bindings of the same
/// name — a shadow, or siblings in different scopes — have distinct ids, which
/// is what lets scope-aware resolution tell a use of one from a use of the
/// other.
pub type BindingId = u32;

/// What a [`Frame`] holds, which decides whether a lookup from inside a nested
/// `fn` body may read it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    /// Locals: `let`s, closure params, pattern bindings.
    #[default]
    Locals,
    /// A block's item statements (nested `fn`s), which stay visible inside a
    /// nested item body.
    Items,
    /// The outermost frame of a `fn` body, holding its params and `self`:
    /// locals of the frames below it are out of reach (an item statement
    /// captures nothing).
    FnBody,
}

/// One lexical frame of a [`Scopes`] stack.
#[derive(Debug, Default)]
struct Frame {
    names: HashMap<String, (Ty, BindingId)>,
    kind: FrameKind,
}

/// The lexical scope stack for value bindings (`let`, params, closure params,
/// pattern bindings, item-statement `fn`s). Innermost scope is last. Each
/// binding carries a [`BindingId`] minted on `bind`, so a later `lookup` can
/// report *which* binding a name resolved to, not just its type.
///
/// A `fn` body is a capture boundary: lookups cross it only into item frames,
/// never into the locals of an enclosing block (`STATIC.md` §2).
#[derive(Debug, Default)]
pub struct Scopes {
    frames: Vec<Frame>,
    next_id: BindingId,
}

impl Scopes {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frames: vec![Frame::default()],
            next_id: 0,
        }
    }

    pub fn push(&mut self) {
        self.frames.push(Frame::default());
    }

    /// Opens the frame of a `fn` body — a capture boundary.
    pub fn push_fn(&mut self) {
        self.frames.push(Frame {
            kind: FrameKind::FnBody,
            ..Frame::default()
        });
    }

    /// Opens a block's item frame: the hoisted item-statement names, visible
    /// across the whole block and inside the items it declares.
    pub fn push_items(&mut self) {
        self.frames.push(Frame {
            kind: FrameKind::Items,
            ..Frame::default()
        });
    }

    pub fn pop(&mut self) {
        self.frames.pop();
    }

    /// Binds a name in the innermost scope and returns its fresh
    /// [`BindingId`]. Shadowing is allowed (`STATIC.md` §2) and silently
    /// overwrites within the same frame (the new id wins for later lookups).
    pub fn bind(&mut self, name: impl Into<String>, ty: Ty) -> BindingId {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(frame) = self.frames.last_mut() {
            frame.names.insert(name.into(), (ty, id));
        }
        id
    }

    /// Whether the innermost frame already binds `name`.
    #[must_use]
    pub fn bound_here(&self, name: &str) -> bool {
        self.frames
            .last()
            .is_some_and(|f| f.names.contains_key(name))
    }

    /// Resolves an unqualified `name` through the stack, innermost first
    /// (`STATIC.md` §2).
    ///
    /// An item statement is hoisted ahead of its declaring block's `let`s, so
    /// that block's locals frame — the frame just outside a `fn` body — is
    /// skipped. Past a `fn` boundary only item frames are readable; the nearest
    /// enclosing local, param, or `self` there is a capture, and it shadows any
    /// outer item or module name of the same spelling.
    #[must_use]
    pub fn resolve(&self, name: &str) -> ScopeLookup<'_> {
        let mut crossed = false;
        let mut outside_fn_body = false;
        let mut skipped = false;
        for f in self.frames.iter().rev() {
            let declaring_locals = outside_fn_body && f.kind == FrameKind::Locals;
            outside_fn_body = f.kind == FrameKind::FnBody;
            if declaring_locals {
                skipped |= f.names.contains_key(name);
            } else if let Some((ty, id)) = f.names.get(name) {
                return if !crossed || f.kind == FrameKind::Items {
                    ScopeLookup::Local(ty, *id)
                } else {
                    ScopeLookup::Captured
                };
            }
            crossed |= outside_fn_body;
        }
        if skipped {
            ScopeLookup::DeclaringBlockLocal
        } else {
            ScopeLookup::Unbound
        }
    }

    /// Looks a readable binding's type up from the innermost scope outward.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Ty> {
        match self.resolve(name) {
            ScopeLookup::Local(ty, _) => Some(ty),
            _ => None,
        }
    }
}

/// What [`Scopes::resolve`] found for an unqualified name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScopeLookup<'a> {
    /// A binding the current position may read.
    Local(&'a Ty, BindingId),
    /// A `let`, param, or `self` of an enclosing function, across a nested
    /// `fn` boundary: naming it is an error, and it hides outer names.
    Captured,
    /// Bound only by a `let` of the block that declares the current nested
    /// `fn`: out of reach, so outer names show through; naming it with nothing
    /// outer to resolve to is a capture error.
    DeclaringBlockLocal,
    /// No binding; the name is a module item, a prelude name, or unknown.
    Unbound,
}
