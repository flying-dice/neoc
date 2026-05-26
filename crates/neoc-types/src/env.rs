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
    /// Names of the supertraits this trait requires (`trait A: B + C`). An
    /// implementor of this trait must also implement each, and a bound by this
    /// trait grants access to their methods.
    pub supertraits: Vec<String>,
    /// Names of the associated types this trait declares (`type Item;`). Each
    /// must be bound by every implementor, and trait methods may refer to them
    /// as `Self::Item`.
    pub assoc_types: Vec<String>,
}

/// One method signature of a trait.
#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
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
    /// Trait implementations as `(type name, trait name)` pairs — one per
    /// `impl Trait for Type` and per `#[derive(Trait)]`. The checker consults
    /// this to decide whether a concrete type satisfies a type parameter's
    /// bound at a call site (`STATIC.md` §3.5).
    pub trait_impls: HashSet<(String, String)>,
    /// Per-function declared bounds: function name → (type parameter → the
    /// trait names bounding it), from both `<T: …>` and `where T: …`. Lets a
    /// call site enforce that each argument's concrete type satisfies the bound
    /// the callee declared.
    pub fn_bounds: HashMap<String, HashMap<String, Vec<String>>>,
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
            .contains(&(type_name.to_string(), trait_name.to_string()))
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

    /// Pulls in the *structure* of every type this table's symbols reference but
    /// does not yet define, drawing from `universe`, transitively.
    ///
    /// A multi-file driver uses this after resolving a module's `use` imports.
    /// When module `c` imports a function `make` whose return type is a `struct`
    /// `Config` declared in module `a` — but `c` never names `Config` in a `use`
    /// — field and method access on a `Config` value (`c.level`) needs the
    /// type's shape. The direct import brings in `make`'s signature; this brings
    /// in the shapes that signature (and theirs, recursively) refer to.
    ///
    /// Only the type namespace and its methods are copied, not the visibility
    /// sets: a transitively-known type is resolvable, not re-exported. The
    /// `universe` is the project-wide pool of `pub` type definitions; a type
    /// name is taken to be project-unique (neoc has no module-qualified type
    /// identity), so a name already defined here is left untouched.
    ///
    /// # Arguments
    ///
    /// * `universe` — the aggregate of every module's `pub` type definitions and
    ///   their methods. The caller owns it; this table borrows it for the call.
    pub fn import_referenced_types(&mut self, universe: &SymbolTable) {
        let mut pending: Vec<String> = Vec::new();
        for ty in self.values.values() {
            collect_ty_names(ty, &mut pending);
        }
        for def in self.types.values() {
            collect_typedef_names(def, &mut pending);
        }
        for info in self.methods.values() {
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

    /// Resolves a type name through alias chains to its underlying definition
    /// name. Returns the input unchanged if it is not an alias.
    #[must_use]
    pub fn resolve_alias(&self, name: &str) -> Ty {
        match self.types.get(name) {
            Some(TypeDef::Alias { value, .. }) => self.resolve_alias_ty(value.clone()),
            _ => Ty::named(name),
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
        let names: Vec<String> = self.types.keys().cloned().collect();
        for name in names {
            if let Some(def) = self.types.get_mut(&name) {
                normalize_typedef(def, &snapshot);
            }
        }
    }

    /// Recursively expands aliases inside a type.
    #[must_use]
    pub fn resolve_alias_ty(&self, ty: Ty) -> Ty {
        match ty {
            Ty::Named { name, args } => {
                if let Some(TypeDef::Alias { value, .. }) = self.types.get(&name) {
                    if args.is_empty() {
                        return self.resolve_alias_ty(value.clone());
                    }
                }
                let args = args.into_iter().map(|a| self.resolve_alias_ty(a)).collect();
                Ty::Named { name, args }
            }
            Ty::List(inner) => Ty::List(Box::new(self.resolve_alias_ty(*inner))),
            Ty::Fn(params, ret) => Ty::Fn(
                params
                    .into_iter()
                    .map(|p| self.resolve_alias_ty(p))
                    .collect(),
                Box::new(self.resolve_alias_ty(*ret)),
            ),
            other => other,
        }
    }
}

/// Pushes every nominal type name a [`Ty`] mentions onto `out` (recursing
/// through lists, function params/returns, and type arguments). Primitives,
/// `Unit`, type parameters, and `Unknown` carry no name.
fn collect_ty_names(ty: &Ty, out: &mut Vec<String>) {
    match ty {
        Ty::List(inner) => collect_ty_names(inner, out),
        Ty::Fn(params, ret) => {
            for p in params {
                collect_ty_names(p, out);
            }
            collect_ty_names(ret, out);
        }
        Ty::Named { name, args } => {
            out.push(name.clone());
            for a in args {
                collect_ty_names(a, out);
            }
        }
        Ty::Number | Ty::Str | Ty::Bool | Ty::Unit | Ty::Param(_) | Ty::Unknown => {}
    }
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
        }
        TypeDef::Alias { .. } | TypeDef::Opaque => {}
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
/// other (`tasks/scope-aware-resolution.md`).
pub type BindingId = u32;

/// The lexical scope stack for value bindings (`let`, params, closure params,
/// pattern bindings). Innermost scope is last. Each binding carries a
/// [`BindingId`] minted on `bind`, so a later `lookup` can report *which*
/// binding a name resolved to, not just its type.
#[derive(Debug, Default)]
pub struct Scopes {
    frames: Vec<HashMap<String, (Ty, BindingId)>>,
    next_id: BindingId,
}

impl Scopes {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frames: vec![HashMap::new()],
            next_id: 0,
        }
    }

    pub fn push(&mut self) {
        self.frames.push(HashMap::new());
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
            frame.insert(name.into(), (ty, id));
        }
        id
    }

    /// Looks a name's type up from the innermost scope outward.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Ty> {
        self.frames
            .iter()
            .rev()
            .find_map(|f| f.get(name))
            .map(|(ty, _)| ty)
    }

    /// The [`BindingId`] a name resolves to from the innermost scope outward —
    /// the binding a use of `name` refers to. `None` when no binding is in
    /// scope (the name is a top-level item, a prelude name, or unresolved).
    #[must_use]
    pub fn binding_of(&self, name: &str) -> Option<BindingId> {
        self.frames
            .iter()
            .rev()
            .find_map(|f| f.get(name))
            .map(|(_, id)| *id)
    }
}
