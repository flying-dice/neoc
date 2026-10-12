//! Expression and pattern inference plus the well-formedness checks.
//!
//! The [`Checker`] owns the symbol table and a value-scope stack, walks every
//! function body, and pushes a [`Diagnostic`](crate::Diagnostic) for each
//! violation of `STATIC.md` §4–§8. Inference is the rank-1, no-subtyping
//! fragment described in §3.5: types flow from annotations and arguments, and
//! `Ty::Unknown` absorbs errors so one mistake produces one diagnostic.

use neoc_ast as ast;

use crate::Diagnostic;
use crate::collect::{generic_names, lower_type};
use crate::env::{
    ScopeLookup, Scopes, StructFields, StructInfo, SymbolTable, TraitRef, TypeDef, VariantFields,
};
use crate::ty::Ty;

mod impl_check;
mod pattern;
mod solve;
mod usefulness;

/// The walking type-checker. Holds the collected declarations, the lexical
/// scope stack, the enclosing function's return type (for `?` and bare
/// `return`), and the accumulated diagnostics.
pub struct Checker<'a> {
    table: &'a SymbolTable,
    pub(crate) scopes: Scopes,
    /// Return type of the function whose body is currently being checked.
    pub(crate) ret_ty: Ty,
    /// What `Self` names in the current function body: the enclosing impl's
    /// target, or `Self` itself in a trait default body. `None` outside an impl.
    /// Lets `Self { .. }` head a struct literal (`STATIC.md` §3.3).
    pub(crate) impl_target: Option<String>,
    /// Type parameters in scope for the current item.
    pub(crate) type_params: Vec<String>,
    /// Type parameters of the functions enclosing a nested `fn`: out of scope
    /// in its body, and an error to name there (`STATIC.md` §2).
    enclosing_type_params: Vec<String>,
    /// Bounds for the in-scope type parameters: `T` → the trait names it is
    /// bound by (from `<T: Trait>` and `where T: Trait`). Lets a method call on
    /// a bounded parameter resolve through the bound trait's methods
    /// (`STATIC.md` §method-resolution).
    pub(crate) type_bounds: std::collections::HashMap<String, Vec<String>>,
    /// Associated-type bindings in scope for the current `impl` body: `Item` →
    /// the type it is bound to. Lets `Self::Item` resolve to the concrete type
    /// the impl assigned it (`STATIC.md` §3.4). Empty outside a trait impl.
    pub(crate) assoc_bindings: std::collections::HashMap<String, Ty>,
    diagnostics: Vec<Diagnostic>,
    /// When set, every inferred expression's type is recorded in `expr_types`.
    /// Off by default so the compile path pays nothing; tooling turns it on for
    /// position→type queries.
    record_types: bool,
    expr_types: Vec<ExprType>,
    /// When set, every binding and single-segment identifier use is recorded in
    /// `resolution`, for scope-aware references/rename/goto. Off by default.
    record_resolution: bool,
    resolution: crate::resolve::Resolution,
    /// When set, every call that instantiates a generic function's type
    /// parameters is recorded in `calls`, for call-site signature hover. Off by
    /// default.
    record_calls: bool,
    calls: Vec<CallSite>,
    /// `(type, trait)` pairs implemented more than once, found by
    /// [`Checker::check_coherence`]. Their impls share one method table entry
    /// per name, so signature conformance is skipped for them — the conflict
    /// is the diagnostic.
    pub(crate) conflicting_impls: std::collections::HashSet<(String, String)>,
}

/// The inferred type of one expression, with the byte range it occupies. The
/// basis for tooling that maps a cursor position to a type (hover, type inlay
/// hints).
#[derive(Debug, Clone, PartialEq)]
pub struct ExprType {
    pub start: usize,
    pub end: usize,
    pub ty: Ty,
}

/// A call that instantiates a generic function's type parameters, captured at
/// the callee identifier's span together with the substitution its arguments
/// forced. The raw record; [`call_signatures`](crate::call_signatures) resolves
/// it against the callee's declaration into a renderable [`CallSignature`].
struct CallSite {
    start: usize,
    end: usize,
    name: String,
    subst: std::collections::HashMap<String, Ty>,
}

/// A generic function's signature *at one call site*: its type parameters
/// instantiated from the argument types, with the resulting parameter and
/// return types. The basis for hover at a call site
/// (`fn first<number>(a: number, b: number) -> number`) — the view the declared
/// signature cannot show. `start..end` is the callee identifier's byte range.
#[derive(Debug, Clone, PartialEq)]
pub struct CallSignature {
    pub start: usize,
    pub end: usize,
    /// Type arguments inferred for the callee's generics, in declaration order.
    pub type_args: Vec<Ty>,
    /// The callee's parameters as `(name, type)`, each type instantiated.
    pub params: Vec<(String, Ty)>,
    /// The callee's return type, instantiated.
    pub ret: Ty,
}

impl<'a> Checker<'a> {
    #[must_use]
    pub fn new(table: &'a SymbolTable) -> Self {
        Self {
            table,
            scopes: Scopes::new(),
            ret_ty: Ty::Unit,
            impl_target: None,
            type_params: Vec::new(),
            enclosing_type_params: Vec::new(),
            type_bounds: std::collections::HashMap::new(),
            assoc_bindings: std::collections::HashMap::new(),
            diagnostics: Vec::new(),
            record_types: false,
            expr_types: Vec::new(),
            record_resolution: false,
            resolution: crate::resolve::Resolution::default(),
            record_calls: false,
            calls: Vec::new(),
            conflicting_impls: std::collections::HashSet::new(),
        }
    }

    /// Records every inferred expression's type and span from here on, for
    /// tooling position→type queries. The compile path leaves this off.
    pub fn record_types(&mut self) {
        self.record_types = true;
    }

    /// The expression types recorded since [`Self::record_types`] was enabled.
    #[must_use]
    pub fn into_expr_types(self) -> Vec<ExprType> {
        self.expr_types
    }

    /// Records every binding and single-segment identifier use from here on, for
    /// scope-aware tooling (references, rename, goto). The compile path leaves
    /// this off.
    pub fn record_resolution(&mut self) {
        self.record_resolution = true;
    }

    /// The scope resolution recorded since [`Self::record_resolution`] was
    /// enabled.
    #[must_use]
    pub fn into_resolution(self) -> crate::resolve::Resolution {
        self.resolution
    }

    /// Records every generic-instantiating call from here on, for call-site
    /// signature hover. The compile path leaves this off.
    fn record_calls(&mut self) {
        self.record_calls = true;
    }

    /// The generic-instantiating calls recorded since [`Self::record_calls`].
    fn into_calls(self) -> Vec<CallSite> {
        self.calls
    }

    /// Records a generic-instantiating call against the callee's span. Only a
    /// direct named call with a non-empty substitution is recorded — an
    /// indirect or non-generic call has no instantiated signature to show. The
    /// single funnel call recording goes through, mirroring [`Self::record_member`].
    fn record_call(&mut self, callee: &ast::Expr, subst: &std::collections::HashMap<String, Ty>) {
        if !self.record_calls || subst.is_empty() {
            return;
        }
        if let ast::ExprKind::Path(segs) = &callee.kind
            && let Some(name) = segs.last()
        {
            self.calls.push(CallSite {
                start: callee.span.start,
                end: callee.span.end,
                name: name.clone(),
                subst: subst.clone(),
            });
        }
    }

    /// Binds `name` of type `ty` in the current scope and, when recording,
    /// notes the binding's def-span and kind. The single funnel every binding
    /// site goes through, so the resolution map and the type scope never drift.
    fn bind_local(
        &mut self,
        name: impl Into<String>,
        ty: Ty,
        def: ast::Span,
        kind: crate::resolve::BindingKind,
    ) {
        let name = name.into();
        let id = self.scopes.bind(name.clone(), ty);
        if self.record_resolution {
            self.resolution.note_binding(crate::resolve::Binding {
                id,
                name,
                def: (def.start, def.end),
                kind,
            });
        }
    }

    /// Records a `recv.member` access as resolving to `member` on `owner` (a
    /// type or trait), when recording is on and the member's def-span is known.
    /// The single funnel for member resolution, called at each point method or
    /// field resolution settles on an owner.
    fn record_member(&mut self, use_span: ast::Span, owner: &str, member: &str) {
        if !self.record_resolution {
            return;
        }
        if let Some(&def) = self
            .table
            .member_defs
            .get(&(owner.to_string(), member.to_string()))
        {
            self.resolution.note_member(
                use_span.start,
                use_span.end,
                crate::resolve::MemberRef {
                    owner: owner.to_string(),
                    member: member.to_string(),
                    def,
                },
            );
        }
    }

    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Whether `ty` is a nominal type carrying an `eq` method — i.e. a
    /// `#[derive(Eq)]` `struct`/`enum`. Used by elaboration to rewrite `==`/`!=`
    /// on such a type to a structural `.eq` call instead of Lua reference
    /// equality (`DYNAMIC.md` §4.2 / §6.2). The `Result`/`Option` prelude is not
    /// considered `Eq`; only user types that derive it are.
    #[must_use]
    pub fn type_derives_eq(&self, ty: &Ty) -> bool {
        let Ty::Named { name, .. } = ty else {
            return false;
        };
        if name == "Result" || name == "Option" {
            return false;
        }
        self.table
            .methods
            .contains_key(&(name.clone(), "eq".to_string()))
    }

    /// The collected declarations the checker resolves names against.
    pub(crate) fn table(&self) -> &SymbolTable {
        self.table
    }

    /// Records a diagnostic with no attached source range. The compiler maps a
    /// `0..0` span to the start of the file. Prefer [`Self::error_at`] whenever
    /// a node span is in hand.
    pub(crate) fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: 0,
            end: 0,
            labels: Vec::new(),
        });
    }

    /// Records a diagnostic anchored to a node's byte [`ast::Span`], so the LSP
    /// can underline the offending source rather than falling back to line 0.
    pub(crate) fn error_at(&mut self, span: ast::Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: span.start,
            end: span.end,
            labels: Vec::new(),
        });
    }

    /// Records a diagnostic with one or more secondary labels — related source
    /// locations (each a byte range plus its own message) that explain the
    /// primary one, such as where a trait method was declared. Surfaced as LSP
    /// related-information.
    pub(crate) fn error_labeled(
        &mut self,
        span: (usize, usize),
        message: impl Into<String>,
        labels: Vec<crate::Label>,
    ) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: span.0,
            end: span.1,
            labels,
        });
    }

    /// Lowers an AST type and expands any transparent `type` aliases through
    /// the symbol table, so an annotation written with an alias compares equal
    /// to its underlying type (`SYNTAX.md` §3.3). Inside a trait impl, a
    /// `Self::Item` projection resolves to the impl's binding for `Item`.
    pub(crate) fn lower(&self, ty: &ast::Type) -> Ty {
        let lowered = self
            .table
            .resolve_alias_ty(lower_type(ty, &self.type_params));
        self.project_assoc(lowered)
    }

    /// [`Self::lower`] for an annotation the checker owns, first reporting at
    /// `span` any generic `type` alias or trait in it applied to the wrong
    /// number of type arguments (`STATIC.md` D-ALIAS-ARITY, §3.4).
    fn lower_at(&mut self, ty: &ast::Type, span: ast::Span) -> Ty {
        let unexpanded = lower_type(ty, &self.type_params);
        self.report_arity(&unexpanded, span);
        self.report_enclosing_type_params(&unexpanded, span);
        self.project_assoc(self.table.resolve_alias_ty(unexpanded))
    }

    /// Reports at `span` each generic `type` alias or trait in `ty` applied to
    /// the wrong number of type arguments, with `params` the type parameters in
    /// scope — a parameter that shares an alias's name shadows it (`STATIC.md`
    /// D-ALIAS-ARITY, §3.4).
    fn check_arity(&mut self, ty: &ast::Type, params: &[String], span: ast::Span) {
        self.report_arity(&lower_type(ty, params), span);
    }

    /// Reports at `span` each type parameter of an enclosing function that
    /// `ty` names from inside a nested `fn` (`STATIC.md` §2).
    fn report_enclosing_type_params(&mut self, ty: &Ty, span: ast::Span) {
        let mut named = Vec::new();
        ty.for_each_named(&mut |name, _| {
            if self.enclosing_type_params.iter().any(|p| p == name) {
                named.push(name.to_string());
            }
        });
        for name in named {
            self.error_at(
                span,
                format!("nested `fn` cannot use type parameter `{name}` of the enclosing function"),
            );
        }
    }

    fn report_arity(&mut self, unexpanded: &Ty, span: ast::Span) {
        for message in self.table.alias_arity_errors(unexpanded) {
            self.error_at(span, message);
        }
        // Unexpanded, so a bare trait spelled inside an alias's value is
        // reported once, at the alias, not again at every use of the alias.
        self.report_trait_type_arity(unexpanded, span);
    }

    /// Reports at `span` every trait used as a type inside `ty` with the wrong
    /// number of arguments (`Source` where `trait Source<T>` needs one),
    /// recursing through lists, function types, and type arguments.
    fn report_trait_type_arity(&mut self, ty: &Ty, span: ast::Span) {
        match ty {
            Ty::List(inner) => self.report_trait_type_arity(inner, span),
            Ty::Fn(params, ret) => {
                for p in params {
                    self.report_trait_type_arity(p, span);
                }
                self.report_trait_type_arity(ret, span);
            }
            Ty::Named { name, args } => {
                for a in args {
                    self.report_trait_type_arity(a, span);
                }
                if let Some(TypeDef::Trait(t)) = self.table.types.get(name) {
                    let expected = t.generics.len();
                    let applied = TraitRef {
                        name: name.clone(),
                        args: args.clone(),
                    };
                    self.check_trait_arity(&applied, expected, span);
                }
            }
            _ => {}
        }
    }

    /// Whether `applied` supplies exactly the `expected` number of type
    /// arguments its trait declares; reports the mismatch at `span` when not.
    fn check_trait_arity(&mut self, applied: &TraitRef, expected: usize, span: ast::Span) -> bool {
        let found = applied.args.len();
        if found != expected {
            self.error_at(
                span,
                format!(
                    "trait `{}` expects {expected} type argument(s), found {found}",
                    applied.name
                ),
            );
        }
        found == expected
    }

    /// Checks the alias and trait arity of the signature `sig`, its own generics added to
    /// the enclosing `params`.
    fn check_sig_arity(&mut self, sig: &ast::FnSig, params: &[String]) {
        let mut params = params.to_vec();
        params.extend(generic_names(&sig.generics));
        for p in &sig.params {
            if let ast::Param::Named { ty, span, .. } = p {
                self.check_arity(ty, &params, *span);
            }
        }
        if let Some(r) = &sig.ret {
            self.check_arity(r, &params, sig.name_span);
        }
    }

    /// Checks the alias and trait arity of a struct's fields; a tuple body reports at
    /// `span`.
    fn check_struct_arity(&mut self, s: &ast::StructDecl, span: ast::Span) {
        let params = generic_names(&s.generics);
        match &s.body {
            ast::StructBody::Unit => {}
            ast::StructBody::Tuple(ty) => self.check_arity(ty, &params, span),
            ast::StructBody::Named(fields) => {
                for f in fields {
                    self.check_arity(&f.ty, &params, f.name_span);
                }
            }
        }
    }

    /// Checks the type annotations an item declares outside any function body —
    /// struct and enum fields, bodyless trait and `extern` signatures, impl
    /// headers and associated-type bindings, an alias's own value — for alias and
    /// trait arity, and rejects a recursive `type` alias (`STATIC.md`
    /// D-ALIAS-ARITY, D-ALIAS-RECURSIVE, §3.4). Signatures
    /// with a body are checked with that body.
    fn check_declared_types(&mut self, item: &ast::Item) {
        match &item.kind {
            ast::ItemKind::Struct(s) => self.check_struct_arity(s, item.span),
            ast::ItemKind::Enum(e) => {
                let params = generic_names(&e.generics);
                for v in &e.variants {
                    match &v.body {
                        ast::VariantBody::Unit => {}
                        ast::VariantBody::Tuple(tys) => {
                            for t in tys {
                                self.check_arity(t, &params, v.name_span);
                            }
                        }
                        ast::VariantBody::Named(fields) => {
                            for f in fields {
                                self.check_arity(&f.ty, &params, f.name_span);
                            }
                        }
                    }
                }
            }
            ast::ItemKind::Trait(t) => {
                let params = generic_names(&t.generics);
                for ti in t.items.iter().filter(|ti| ti.default.is_none()) {
                    self.check_sig_arity(&ti.sig, &params);
                }
            }
            ast::ItemKind::Impl(b) => {
                let params = generic_names(&b.generics);
                self.check_impl_header_arity(b, &params, item.span);
                for (_, ty) in &b.assoc_bindings {
                    self.check_arity(ty, &params, item.span);
                }
            }
            ast::ItemKind::Extern(block) => self.check_extern_arity(&block.items),
            ast::ItemKind::TypeAlias(a) => {
                self.check_arity(&a.value, &generic_names(&a.generics), item.span);
                if self.table.alias_is_recursive(&a.name) {
                    self.error_at(item.span, format!("type alias `{}` is recursive", a.name));
                }
            }
            _ => {}
        }
    }

    /// Checks the alias and trait arity of an impl header — its target type and its
    /// trait's type arguments — reporting at `span`.
    fn check_impl_header_arity(&mut self, b: &ast::ImplBlock, params: &[String], span: ast::Span) {
        self.check_arity(&b.ty, params, span);
        if let Some(t) = &b.trait_ {
            self.check_arity(&ast::Type::Path(t.clone()), params, span);
        }
    }

    /// [`Self::check_declared_types`] for the items of an `extern` block,
    /// recursing into nested `mod`s.
    fn check_extern_arity(&mut self, items: &[ast::ExternItem]) {
        for ext in items {
            match &ext.kind {
                ast::ExternItemKind::Fn(sig) => self.check_sig_arity(sig, &[]),
                ast::ExternItemKind::Struct(s) => self.check_struct_arity(s, ext.span),
                ast::ExternItemKind::Impl(b) => {
                    let params = generic_names(&b.generics);
                    self.check_impl_header_arity(b, &params, ext.span);
                    for m in &b.items {
                        self.check_sig_arity(&m.sig, &params);
                    }
                }
                ast::ExternItemKind::Mod(m) => self.check_extern_arity(&m.items),
            }
        }
    }

    /// Resolves any `Self::Item` projection sentinel in `ty` to the impl's
    /// binding for that associated type, recursing through composites. A no-op
    /// outside an impl (no bindings) or for an unbound name (left as-is, where
    /// it surfaces as a mismatch).
    fn project_assoc(&self, ty: Ty) -> Ty {
        crate::collect::project_assoc(ty, &self.assoc_bindings)
    }

    // ---- item walking ------------------------------------------------------

    /// Checks every item in the module.
    pub fn check_module(&mut self, module: &ast::Module) {
        self.check_coherence(module);
        self.check_method_clashes(module);
        for item in &module.items {
            self.check_reserved_module_names(item);
            self.check_field_names(item);
            self.check_declared_types(item);
            self.check_item(item);
        }
    }

    /// Rejects a module-level name that begins with `__` (`STATIC.md` §4.3,
    /// D-NAME-RESERVED). The `__` prefix is held for compiler-emitted
    /// module-level helpers; a user name spelled that way would shadow one, or
    /// resolve to one, and silently change behaviour.
    fn check_reserved_module_names(&mut self, item: &ast::Item) {
        for (name, span) in module_scope_names(item) {
            if name.starts_with("__") {
                self.error_at(
                    span,
                    format!(
                        "name `{name}` is reserved: module-level names may not begin with `__`"
                    ),
                );
            }
        }
    }

    /// Rejects any named field that begins with `__` (`STATIC.md` §3.3). A
    /// struct field or named-enum-variant field becomes an object key in the
    /// emitted data record, where the `__`-prefix is reserved for the runtime's
    /// own tag keys — a user field spelled that way would collide with them.
    /// Tuple slots (single-underscore `_0`, `_1`, …) are compiler-generated and
    /// never user-named, so they are unaffected; type and variant *names* become
    /// string values, not keys, so they are unaffected too.
    fn check_field_names(&mut self, item: &ast::Item) {
        match &item.kind {
            ast::ItemKind::Struct(s) => {
                if let ast::StructBody::Named(fields) = &s.body {
                    self.check_reserved_fields(fields);
                }
            }
            ast::ItemKind::Enum(e) => {
                for v in &e.variants {
                    if let ast::VariantBody::Named(fields) = &v.body {
                        self.check_reserved_fields(fields);
                    }
                }
            }
            _ => {}
        }
    }

    /// Reports each field in `fields` whose name begins with `__`.
    fn check_reserved_fields(&mut self, fields: &[ast::Field]) {
        for f in fields {
            if f.name.starts_with("__") {
                self.error_at(
                    f.name_span,
                    format!(
                        "field name `{}` is reserved: field names may not begin with `__`",
                        f.name
                    ),
                );
            }
        }
    }

    /// Rejects the impl overlaps a simple checker can detect: two impls of one
    /// trait for the same type (even at different arguments — a type's method
    /// names are unique), two blanket impls of the same trait (unorderable),
    /// and a blanket impl of a trait that a concrete type also implements
    /// directly (`STATIC.md` §3.5). neoc has no general coherence solver beyond
    /// these.
    fn check_coherence(&mut self, module: &ast::Module) {
        let mut impls: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for item in &module.items {
            let ast::ItemKind::Impl(b) = &item.kind else {
                continue;
            };
            let (Some(target), Some(trait_)) = (
                crate::collect::impl_target_name(&b.ty),
                b.trait_.as_ref().and_then(|p| p.segments.last()),
            ) else {
                continue;
            };
            if b.generics.iter().any(|g| g.name == target) {
                continue;
            }
            let key = (target.clone(), trait_.name.clone());
            if !impls.insert(key.clone()) {
                self.conflicting_impls.insert(key);
                self.error_at(
                    item.span,
                    format!(
                        "conflicting impls of `{}` for `{target}`: a type implements a trait at most once",
                        trait_.name
                    ),
                );
            }
        }
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for bl in &self.table.blanket_impls {
            if !seen.insert(bl.trait_ref.name.as_str()) {
                self.error(format!(
                    "conflicting blanket impls of `{}`",
                    bl.trait_ref.name
                ));
            }
        }
        for bl in &self.table.blanket_impls {
            for (ty, traits) in &self.table.trait_impls {
                if traits.contains(&bl.trait_ref.name) {
                    self.error(format!(
                        "`impl {} for {ty}` overlaps the blanket impl of `{}`",
                        bl.trait_ref.name, bl.trait_ref.name
                    ));
                }
            }
        }
    }

    /// Rejects a second explicit method of one name on a type (`STATIC.md` §8).
    /// A type has one method table, so an inherent method, a trait-impl method,
    /// an `extern` impl method, or a blanket impl's method that takes a name
    /// the type already has — in either declaration order — is an error at the
    /// later impl. Inherited defaults are checked by
    /// [`Checker::check_inherited_defaults`].
    fn check_method_clashes(&mut self, module: &ast::Module) {
        let mut impls: Vec<(&ast::ImplBlock, ast::Span, bool)> = Vec::new();
        // `(type, method)` → who first gave it: label, trait, impl index.
        let mut owners: std::collections::HashMap<(String, String), Owner> =
            std::collections::HashMap::new();
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::Impl(b) => impls.push((b, item.span, false)),
                ast::ItemKind::Extern(block) => collect_extern_impls(&block.items, &mut impls),
                _ => {}
            }
        }
        for (idx, (b, span, is_extern)) in impls.into_iter().enumerate() {
            let Some(target) = crate::collect::impl_target_name(&b.ty) else {
                continue;
            };
            let trait_name = b
                .trait_
                .as_ref()
                .and_then(|p| p.segments.last())
                .map(|s| s.name.clone());
            let header = match &trait_name {
                Some(t) => format!("impl {t} for {target}"),
                None => format!("impl {target}"),
            };
            let label = if is_extern {
                format!("`extern {header}`")
            } else {
                format!("`{header}`")
            };
            let types = if b.generics.iter().any(|g| g.name == target) {
                // A blanket impl lands on every type meeting its conditions.
                let conditions = crate::collect::generic_param_bounds(&b.generics)
                    .remove(&target)
                    .unwrap_or_default();
                self.blanket_targets(&conditions)
            } else {
                vec![target]
            };
            for ty in types {
                for f in &b.items {
                    match owners.entry((ty.clone(), f.sig.name.clone())) {
                        std::collections::hash_map::Entry::Vacant(e) => {
                            e.insert((label.clone(), trait_name.clone(), idx));
                        }
                        // Another impl of one trait is already a coherence error.
                        std::collections::hash_map::Entry::Occupied(e)
                            if trait_name.is_some()
                                && e.get().1 == trait_name
                                && e.get().2 != idx => {}
                        std::collections::hash_map::Entry::Occupied(e) => {
                            self.error_at(
                                span,
                                format!(
                                    "{label} gives `{ty}` a method `{}`, but {} already does; a type has one method per name",
                                    f.sig.name,
                                    e.get().0
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    fn check_item(&mut self, item: &ast::Item) {
        self.check_item_attrs(item);
        match &item.kind {
            ast::ItemKind::Fn(f) => self.check_fn(f, None),
            ast::ItemKind::Const(c) => {
                let declared = self.lower_at(&c.ty, item.span);
                let actual = self.infer(&c.value);
                self.expect(&declared, &actual, "const initialiser");
            }
            ast::ItemKind::Impl(b) => self.check_impl(b, item.span),
            ast::ItemKind::Trait(t) => {
                self.check_supertraits(t, item.span);
                self.check_method_generics_shadowing(t);
                self.check_trait_defaults(t);
            }
            ast::ItemKind::Extern(block) => self.check_extern_block(&block.items, false, item.span),
            _ => {}
        }
    }

    /// Brings `Self` into scope as a type parameter bounded by the trait
    /// `trait_name`, with the trait's own type parameters beside it, for
    /// checking one of that trait's default bodies — so a default of `trait
    /// Source<T>` reads `T` as the trait's parameter, fixed per implementor
    /// only when the default is inherited (`STATIC.md` §8). The caller saves
    /// and restores `type_params` / `type_bounds` around the body.
    pub(crate) fn enter_trait_self(&mut self, trait_name: &str) {
        if let Some(TypeDef::Trait(t)) = self.table.types.get(trait_name) {
            self.type_params.extend(t.generics.iter().cloned());
        }
        self.type_params.push("Self".to_string());
        self.type_bounds
            .entry("Self".to_string())
            .or_default()
            .push(trait_name.to_string());
    }

    /// The type `self` (and `Self`) denotes for target `name`: a type parameter
    /// when `name` is one in scope (a blanket impl's target, a trait's `Self`),
    /// otherwise the named type.
    pub(crate) fn self_target(&self, name: &str) -> Ty {
        if self.type_params.iter().any(|p| p == name) {
            Ty::Param(name.to_string())
        } else {
            Ty::named(name)
        }
    }

    /// Checks each default body of `t` with `Self` as a type parameter bounded
    /// by the trait itself, so `self.m()` resolves through the trait's own
    /// methods and its supertraits' (`STATIC.md` §3.4) exactly as it does for
    /// a `T: Trait` parameter. The body is checked once, abstractly; each
    /// implementor inherits it (see [`Checker::default_method`]).
    fn check_trait_defaults(&mut self, t: &ast::TraitDecl) {
        let prev_params = self.type_params.clone();
        let prev_bounds = self.type_bounds.clone();
        self.enter_trait_self(&t.name);
        for ti in &t.items {
            if let Some(body) = &ti.default {
                self.check_fn_body(&ti.sig, body, Some("Self"));
            }
        }
        self.type_params = prev_params;
        self.type_bounds = prev_bounds;
    }

    /// Validates a trait's supertrait list: each named supertrait must be a
    /// trait applied to its declared number of arguments, and the supertrait
    /// relation must be acyclic (`STATIC.md` §3.4).
    fn check_supertraits(&mut self, t: &ast::TraitDecl, span: ast::Span) {
        let generics = generic_names(&t.generics);
        for sup in &t.supertraits {
            let Some(applied) = crate::collect::lower_trait_ref(sup, &generics, None) else {
                continue;
            };
            let name = &applied.name;
            let Some(TypeDef::Trait(info)) = self.table.types.get(name) else {
                self.error(format!("unknown supertrait `{name}` on trait `{}`", t.name));
                continue;
            };
            let expected = info.generics.len();
            if self.trait_chain_includes(name, &t.name) {
                self.error(format!(
                    "supertrait cycle: `{}` and `{name}` are mutually dependent",
                    t.name
                ));
            } else {
                self.check_trait_arity(&applied, expected, span);
            }
        }
    }

    /// Rejects a trait method type parameter that reuses one of the trait's
    /// own type parameter names (`trait Source<T> { fn conv<T>(…) }`): inside
    /// the method the name would be ambiguous (`STATIC.md` §3.4).
    fn check_method_generics_shadowing(&mut self, t: &ast::TraitDecl) {
        for ti in &t.items {
            for g in &ti.sig.generics {
                if t.generics.iter().any(|tg| tg.name == g.name) {
                    self.error_at(
                        ti.sig.name_span,
                        format!(
                            "type parameter `{}` of method `{}` shadows a type parameter of trait `{}`; rename it",
                            g.name, ti.sig.name, t.name
                        ),
                    );
                }
            }
        }
    }

    fn check_fn(&mut self, f: &ast::FnDecl, self_ty: Option<&str>) {
        self.check_fn_body(&f.sig, &f.body, self_ty);
    }

    fn check_fn_body(&mut self, sig: &ast::FnSig, body: &ast::Block, self_ty: Option<&str>) {
        // Extend (not replace) the in-scope parameters/bounds with the
        // function's own, so an enclosing impl's generics stay visible.
        let prev_params = self.type_params.clone();
        let prev_bounds = self.type_bounds.clone();
        self.type_params
            .extend(sig.generics.iter().map(|g| g.name.clone()));
        for (k, v) in crate::collect::param_bounds(sig) {
            self.type_bounds.entry(k).or_default().extend(v);
        }
        // `self`'s type is the target — a type parameter (a blanket impl's
        // target) or a concrete type. `Self` in the signature is read the same.
        let self_target: Option<Ty> = self_ty.map(|t| self.self_target(t));

        self.scopes.push_fn();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => {
                    if let Some(st) = &self_target {
                        self.scopes.bind("self", st.clone());
                    }
                }
                ast::Param::Named { name, ty, span, .. } => {
                    let lowered = self.lower_at(ty, *span);
                    let pty = match &self_target {
                        Some(st) => lowered.subst_self(st),
                        None => lowered,
                    };
                    self.bind_local(name.clone(), pty, *span, crate::resolve::BindingKind::Param);
                }
            }
        }
        let lowered_ret = sig
            .ret
            .as_ref()
            .map_or(Ty::Unit, |r| self.lower_at(r, sig.name_span));
        let ret = match &self_target {
            Some(st) => lowered_ret.subst_self(st),
            None => lowered_ret,
        };
        let prev_ret = std::mem::replace(&mut self.ret_ty, ret.clone());
        let prev_target = std::mem::replace(&mut self.impl_target, self_ty.map(str::to_string));

        let actual = self.infer_block(body);
        self.expect(&ret, &actual, "function body");

        self.impl_target = prev_target;
        self.ret_ty = prev_ret;
        self.scopes.pop();
        self.type_params = prev_params;
        self.type_bounds = prev_bounds;
    }

    /// Checks a call's argument count and types against an already-projected
    /// parameter list, pushing a diagnostic per mismatch.
    fn check_call_args(
        &mut self,
        method: &str,
        params: &[Ty],
        arg_tys: &[Ty],
        args: &[ast::Expr],
        span: ast::Span,
    ) {
        if params.len() != arg_tys.len() {
            self.error_at(
                span,
                format!(
                    "method `{method}` takes {} argument(s) but {} were supplied",
                    params.len(),
                    arg_tys.len()
                ),
            );
            return;
        }
        for (i, (p, a)) in params.iter().zip(arg_tys).enumerate() {
            if !self.assignable(p, a) {
                self.error_at(
                    args[i].span,
                    format!(
                        "argument {} of `{method}` expects `{p}`, found `{a}`",
                        i + 1
                    ),
                );
            }
        }
    }

    /// The struct a literal headed by `path` constructs, with its declaration.
    /// A bare `Self` is the enclosing impl's struct target (`STATIC.md` §3.3,
    /// T-STRUCT-LIT-SELF); `None` after reporting when there is no such struct.
    fn struct_lit_target(
        &mut self,
        path: &[ast::Ident],
        span: ast::Span,
    ) -> Option<(String, &'a StructInfo)> {
        let name = path.last()?;
        let name = if name == "Self" {
            self.self_lit_target(span)?
        } else {
            name.clone()
        };
        let table = self.table;
        let Some(TypeDef::Struct(s)) = table.types.get(&name) else {
            self.error_at(span, format!("unknown struct `{name}`"));
            return None;
        };
        Some((name, s))
    }

    /// The type a `Self { .. }` literal names. Rejects a literal outside an impl,
    /// in a trait default body, in a blanket impl, on a non-struct target, and
    /// where a local shadows the target's name — the literal lowers through that
    /// name (`DYNAMIC.md` §4.4), so the local would capture it.
    fn self_lit_target(&mut self, span: ast::Span) -> Option<String> {
        let Some(target) = self.impl_target.clone() else {
            self.error_at(span, "`Self` is only available inside an impl");
            return None;
        };
        let reason = if target == "Self" {
            Some("`Self` in a trait default body is not a struct".to_string())
        } else if self.type_params.contains(&target) {
            Some(format!(
                "`Self` is the type parameter `{target}`, which is not a struct"
            ))
        } else if !matches!(self.table.types.get(&target), Some(TypeDef::Struct(_))) {
            Some(format!("`Self` is `{target}`, which is not a struct"))
        } else if self.scopes.lookup(&target).is_some() {
            Some(format!(
                "`Self` names `{target}`, which a local binding shadows here; rename the local"
            ))
        } else {
            None
        };
        if let Some(reason) = reason {
            self.error_at(span, reason);
            return None;
        }
        Some(target)
    }

    // ---- statements & blocks ----------------------------------------------

    fn infer_block(&mut self, block: &ast::Block) -> Ty {
        self.enter_block(&block.stmts);
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        let ty = block.value.as_ref().map_or(Ty::Unit, |e| self.infer(e));
        self.exit_block();
        ty
    }

    /// Opens a block's scopes. Item statements (`STATIC.md` §2) are hoisted:
    /// every nested `fn` is bound in an item frame before any statement runs,
    /// so it is visible across the whole block — before and after its
    /// declaration, and to its siblings — then a frame for the locals opens.
    pub(crate) fn enter_block(&mut self, stmts: &[ast::Stmt]) {
        self.scopes.push_items();
        for stmt in stmts {
            let ast::StmtKind::Item(item) = &stmt.kind else {
                continue;
            };
            let ast::ItemKind::Fn(f) = &item.kind else {
                continue;
            };
            if self.scopes.bound_here(&f.sig.name) {
                self.error_at(
                    f.sig.name_span,
                    format!("`{}` is declared twice in this block", f.sig.name),
                );
                continue;
            }
            let (params, ret, _) = crate::collect::fn_type(&f.sig, &[]);
            self.bind_local(
                f.sig.name.clone(),
                Ty::Fn(params, Box::new(ret)),
                f.sig.name_span,
                crate::resolve::BindingKind::Fn,
            );
        }
        self.scopes.push();
    }

    pub(crate) fn exit_block(&mut self) {
        self.scopes.pop();
        self.scopes.pop();
    }

    fn check_stmt(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Let(l) => self.check_let(l, stmt.span),
            ast::StmtKind::Expr(e) => {
                let _ = self.infer(e);
            }
            ast::StmtKind::Item(item) => self.check_item_stmt(item),
        }
    }

    /// Checks an item in statement position: only a non-generic `fn` is
    /// allowed, and its body sees no type parameter of the enclosing item
    /// (`STATIC.md` §2).
    fn check_item_stmt(&mut self, item: &ast::Item) {
        let ast::ItemKind::Fn(f) = &item.kind else {
            self.error_at(
                item.span,
                "only a `fn` may be declared inside a block".to_string(),
            );
            return;
        };
        if !f.sig.generics.is_empty() {
            self.error_at(
                f.sig.name_span,
                format!("nested `fn {}` cannot be generic", f.sig.name),
            );
        }
        let outer_params = std::mem::take(&mut self.type_params);
        let outer_bounds = std::mem::take(&mut self.type_bounds);
        let enclosing_len = self.enclosing_type_params.len();
        self.enclosing_type_params
            .extend(outer_params.iter().cloned());
        self.check_item(item);
        self.enclosing_type_params.truncate(enclosing_len);
        self.type_params = outer_params;
        self.type_bounds = outer_bounds;
    }

    fn check_let(&mut self, l: &ast::LetStmt, span: ast::Span) {
        let declared = l.ty.as_ref().map(|t| self.lower_at(t, span));
        let value_ty = l.value.as_ref().map(|e| self.infer(e));
        let ty = match (&declared, &value_ty) {
            (Some(d), Some(v)) => {
                self.expect(d, v, "let binding");
                d.clone()
            }
            (Some(d), None) => d.clone(),
            (None, Some(v)) => v.clone(),
            (None, None) => Ty::Unknown,
        };
        self.bind_pattern(&l.pattern, &ty, crate::resolve::BindingKind::Let);
    }

    // ---- expression inference ---------------------------------------------

    /// Infers the type of an expression, emitting diagnostics on the way.
    pub fn infer(&mut self, expr: &ast::Expr) -> Ty {
        let span = expr.span;
        let ty = match &expr.kind {
            ast::ExprKind::Int(_) | ast::ExprKind::Float(_) => Ty::Number,
            ast::ExprKind::Str(_) => Ty::Str,
            ast::ExprKind::Bool(_) => Ty::Bool,
            ast::ExprKind::Unit => Ty::Unit,
            ast::ExprKind::Path(segs) => self.infer_path(segs, span),
            ast::ExprKind::Binary { op, lhs, rhs } => self.infer_binary(*op, lhs, rhs, span),
            ast::ExprKind::Unary { op, operand } => self.infer_unary(*op, operand),
            ast::ExprKind::Call { callee, args } => self.infer_call(callee, args, span),
            ast::ExprKind::MethodCall {
                receiver,
                method,
                method_span,
                args,
            } => self.infer_method_call(receiver, method, *method_span, args, span),
            ast::ExprKind::Field { base, field } => self.infer_field(base, field, span),
            ast::ExprKind::StructLit { path, fields, base } => {
                self.infer_struct_lit(path, fields, base.as_deref(), span)
            }
            ast::ExprKind::If { cond, then, else_ } => self.infer_if(cond, then, else_.as_deref()),
            ast::ExprKind::Match { scrutinee, arms } => self.infer_match(scrutinee, arms, span),
            ast::ExprKind::Block(b) => self.infer_block(b),
            ast::ExprKind::Try(inner) => self.infer_try(inner),
            ast::ExprKind::List(items) => self.infer_list(items),
            ast::ExprKind::Index { base, index } => self.infer_index(base, index, span),
            ast::ExprKind::Range { start, end, .. } => {
                let s = self.infer(start);
                self.expect(&Ty::Number, &s, "range start");
                let e = self.infer(end);
                self.expect(&Ty::Number, &e, "range end");
                // Ranges drive `for`; treat their value as a list of numbers.
                Ty::List(Box::new(Ty::Number))
            }
            ast::ExprKind::Assign { target, value, .. } => {
                let t = self.infer(target);
                let v = self.infer(value);
                self.expect(&t, &v, "assignment");
                Ty::Unit
            }
            ast::ExprKind::While { cond, body } => {
                let c = self.infer(cond);
                self.expect(&Ty::Bool, &c, "while condition");
                let _ = self.infer_block(body);
                Ty::Unit
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                let it = self.infer(iter);
                let elem = match it {
                    Ty::List(inner) => *inner,
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.error_at(
                            iter.span,
                            format!("`for` expects a list to iterate, found `{other}`"),
                        );
                        Ty::Unknown
                    }
                };
                self.scopes.push();
                self.bind_pattern(pattern, &elem, crate::resolve::BindingKind::ForPattern);
                let _ = self.infer_block(body);
                self.scopes.pop();
                Ty::Unit
            }
            ast::ExprKind::Loop { body } => {
                let _ = self.infer_block(body);
                Ty::Unit
            }
            ast::ExprKind::Closure { params, body } => self.infer_closure(params, body),
            ast::ExprKind::Return(value) => {
                let actual = value.as_ref().map_or(Ty::Unit, |e| self.infer(e));
                let expected = self.ret_ty.clone();
                self.expect(&expected, &actual, "return value");
                Ty::Unknown
            }
            ast::ExprKind::Break | ast::ExprKind::Continue => Ty::Unknown,
        };
        if self.record_types {
            self.expr_types.push(ExprType {
                start: span.start,
                end: span.end,
                ty: ty.clone(),
            });
        }
        ty
    }

    fn infer_path(&mut self, segs: &[ast::Ident], span: ast::Span) -> Ty {
        if segs.is_empty() {
            return Ty::Unknown;
        }
        if segs.len() == 1 {
            let name = &segs[0];
            let declaring_block_local = match self.scopes.resolve(name) {
                ScopeLookup::Local(t, id) => {
                    let t = t.clone();
                    // Record the use → the local binding it resolves to. A
                    // shadow wins because `resolve` reads the innermost frame
                    // first.
                    if self.record_resolution {
                        self.resolution.note_use(
                            span.start,
                            span.end,
                            crate::resolve::Resolved::Local(id),
                        );
                    }
                    return t;
                }
                // A capture stays an error even when a module item shares its
                // name: the emitted nested function would close over the
                // binding, not the item (`STATIC.md` §2).
                ScopeLookup::Captured => {
                    self.report_capture(name, span);
                    return Ty::Unknown;
                }
                ScopeLookup::DeclaringBlockLocal => true,
                ScopeLookup::Unbound => false,
            };
            if let Some(t) = self.table.values.get(name) {
                // Resolves to a top-level item, not a local — record it as such
                // so a rename of the item skips a shadowing local elsewhere.
                if self.record_resolution {
                    self.resolution.note_use(
                        span.start,
                        span.end,
                        crate::resolve::Resolved::Item(name.clone()),
                    );
                }
                return t.clone();
            }
            // A bare uppercase name may be a unit struct or unit variant.
            if let Some(ty) = self.resolve_unit_value(name) {
                return ty;
            }
            if declaring_block_local {
                self.report_capture(name, span);
            } else {
                self.error_at(span, format!("unknown name `{name}`"));
            }
            return Ty::Unknown;
        }
        // Qualified path: `Type::assoc` / `Enum::Variant`.
        self.resolve_qualified(segs, span)
    }

    /// Type of a field-shorthand init (`P { x }`), which names `x` without an
    /// expression. A capture is reported (once, when `report`) like any other
    /// use of the name (`STATIC.md` §2).
    fn shorthand_ty(&mut self, name: &str, report: bool) -> Option<Ty> {
        match self.scopes.resolve(name) {
            ScopeLookup::Local(t, _) => Some(t.clone()),
            ScopeLookup::Captured => {
                if report {
                    self.report_capture_unspanned(name);
                }
                None
            }
            ScopeLookup::DeclaringBlockLocal => {
                let item = self.table.values.get(name).cloned();
                if item.is_none() && report {
                    self.report_capture_unspanned(name);
                }
                item
            }
            ScopeLookup::Unbound => None,
        }
    }

    fn report_capture_unspanned(&mut self, name: &str) {
        self.error(format!(
            "nested `fn` cannot use `{name}`, a binding of the enclosing function"
        ));
    }

    /// Reports a nested `fn` naming a binding of an enclosing function
    /// (`STATIC.md` §2).
    fn report_capture(&mut self, name: &str, span: ast::Span) {
        let what = if name == "self" {
            "`self` of the enclosing function".to_string()
        } else {
            format!("`{name}`, a binding of the enclosing function")
        };
        self.error_at(span, format!("nested `fn` cannot use {what}"));
    }

    /// Resolves a bare name to a unit struct or unit enum variant, if one
    /// exists. Returns the constructed type.
    fn resolve_unit_value(&self, name: &str) -> Option<Ty> {
        if let Some(TypeDef::Struct(s)) = self.table.types.get(name) {
            if matches!(s.fields, StructFields::Unit) {
                return Some(Ty::named(name));
            }
        }
        // A unit variant referenced unqualified (e.g. `None`).
        for def in self.table.types.values() {
            if let TypeDef::Enum(e) = def {
                if let Some(v) = e.variants.iter().find(|v| v.name == name) {
                    if matches!(v.fields, VariantFields::Unit) {
                        return Some(self.enum_instance(name, e));
                    }
                }
            }
        }
        None
    }

    fn enum_instance(&self, variant: &str, e: &crate::env::EnumInfo) -> Ty {
        // Build the enum type. Find which enum owns this variant by scanning.
        for (name, def) in &self.table.types {
            if let TypeDef::Enum(other) = def {
                if std::ptr::eq(other, e) || other.variants.iter().any(|v| v.name == variant) {
                    let args = other
                        .generics
                        .iter()
                        .map(|g| Ty::Param(g.clone()))
                        .collect();
                    return Ty::Named {
                        name: name.clone(),
                        args,
                    };
                }
            }
        }
        Ty::Unknown
    }

    fn resolve_qualified(&mut self, segs: &[ast::Ident], span: ast::Span) -> Ty {
        let type_name = &segs[segs.len() - 2];
        let member = &segs[segs.len() - 1];
        // `Self` is a type parameter only inside a trait's default body, which
        // is emitted once and shared by every implementor — no runtime handle
        // for `Self` exists there (`STATIC.md` §8).
        if type_name == "Self" && self.type_params.iter().any(|p| p == "Self") {
            self.error_at(
                span,
                format!(
                    "`Self::{member}` cannot be used in a default method body; it is shared by every implementor, so call a method on `self` instead"
                ),
            );
            return Ty::Unknown;
        }
        match self.table.types.get(type_name) {
            Some(TypeDef::Enum(e)) => {
                if let Some(v) = e.variants.iter().find(|v| v.name == *member) {
                    let enum_ty = Ty::Named {
                        name: type_name.clone(),
                        args: e.generics.iter().map(|g| Ty::Param(g.clone())).collect(),
                    };
                    return match &v.fields {
                        VariantFields::Tuple(tys) => Ty::Fn(tys.clone(), Box::new(enum_ty)),
                        VariantFields::Unit | VariantFields::Named(_) => enum_ty,
                    };
                }
                self.error_at(
                    span,
                    format!("enum `{type_name}` has no variant `{member}`"),
                );
                Ty::Unknown
            }
            Some(TypeDef::Struct(_) | TypeDef::Opaque) => {
                if let Some(m) = self.table.methods.get(&(type_name.clone(), member.clone())) {
                    return Ty::Fn(m.params.clone(), Box::new(m.ret.clone()));
                }
                if let Some((params, ret)) =
                    self.default_method(&Ty::named(type_name), member, false)
                {
                    return Ty::Fn(params, Box::new(ret));
                }
                self.error_at(
                    span,
                    format!("`{type_name}` has no associated function `{member}`"),
                );
                Ty::Unknown
            }
            _ => {
                // A leading segment that is not a known type is a module
                // qualifier (an extern `mod`, or a plain namespace path); it is
                // transparent, so the value is named by the final segment. This
                // resolves `rusqlite::open`, `console::log`, and
                // `fs::promises::read_file` to their collected `fn` bindings.
                if let Some(t) = self.table.values.get(member) {
                    return t.clone();
                }
                self.error_at(span, format!("unresolved path `{}`", segs.join("::")));
                Ty::Unknown
            }
        }
    }

    fn infer_binary(
        &mut self,
        op: ast::BinOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: ast::Span,
    ) -> Ty {
        use ast::BinOp;
        let l = self.infer(lhs);
        let r = self.infer(rhs);
        match op {
            BinOp::Add => {
                // `+` is arithmetic on number, concatenation on string.
                if l.compatible(&Ty::Str) && r.compatible(&Ty::Str) && !l.is_unknown() {
                    Ty::Str
                } else if l.compatible(&Ty::Number) && r.compatible(&Ty::Number) {
                    if l.is_unknown() && r.is_unknown() {
                        Ty::Unknown
                    } else {
                        Ty::Number
                    }
                } else {
                    self.error_at(
                        span,
                        format!(
                            "`+` expects two `number`s or two `string`s, found `{l}` and `{r}`"
                        ),
                    );
                    Ty::Unknown
                }
            }
            BinOp::Concat => {
                // Only produced by elaboration (the checker rewrites a
                // string-typed `Add` to `Concat`), so it won't appear during
                // checking — but keep the arm honest: both operands are string.
                self.expect(&Ty::Str, &l, "concatenation operand");
                self.expect(&Ty::Str, &r, "concatenation operand");
                Ty::Str
            }
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                self.expect(&Ty::Number, &l, "arithmetic operand");
                self.expect(&Ty::Number, &r, "arithmetic operand");
                Ty::Number
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                self.expect(&Ty::Number, &l, "comparison operand");
                self.expect(&Ty::Number, &r, "comparison operand");
                Ty::Bool
            }
            BinOp::Eq | BinOp::Ne => {
                if !l.compatible(&r) {
                    self.error_at(span, format!("cannot compare `{l}` with `{r}`"));
                }
                Ty::Bool
            }
            BinOp::And | BinOp::Or => {
                self.expect(&Ty::Bool, &l, "logical operand");
                self.expect(&Ty::Bool, &r, "logical operand");
                Ty::Bool
            }
        }
    }

    fn infer_unary(&mut self, op: ast::UnOp, operand: &ast::Expr) -> Ty {
        let t = self.infer(operand);
        match op {
            ast::UnOp::Not => {
                self.expect(&Ty::Bool, &t, "`!` operand");
                Ty::Bool
            }
            ast::UnOp::Neg => {
                self.expect(&Ty::Number, &t, "`-` operand");
                Ty::Number
            }
        }
    }

    fn infer_call(&mut self, callee: &ast::Expr, args: &[ast::Expr], span: ast::Span) -> Ty {
        let callee_ty = self.infer(callee);
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
        match callee_ty {
            Ty::Fn(params, ret) => {
                if params.len() != arg_tys.len() {
                    self.error_at(
                        span,
                        format!(
                            "this call passes {} argument(s) but the function takes {}",
                            arg_tys.len(),
                            params.len()
                        ),
                    );
                    return self.instantiate_return(&params, &arg_tys, &ret);
                }
                for (i, (p, a)) in params.iter().zip(&arg_tys).enumerate() {
                    if !self.assignable(p, a) {
                        self.error_at(
                            args[i].span,
                            format!("argument {} expects `{p}`, found `{a}`", i + 1),
                        );
                    }
                }
                let subst = self.param_subst(&params, &arg_tys);
                self.check_call_bounds(callee, &subst, span);
                self.record_call(callee, &subst);
                Self::apply_subst(&ret, &subst)
            }
            Ty::Unknown => Ty::Unknown,
            // A dynamic value is callable — this is how an imported `class X;`
            // is constructed (`X(args) : Unknown`). The call yields `Unknown`
            // again (`STATIC.md` §5).
            Ty::Dynamic => Ty::Dynamic,
            other => {
                self.error_at(callee.span, format!("`{other}` is not callable"));
                Ty::Unknown
            }
        }
    }

    /// Substitutes type parameters in a function's return type from the
    /// argument types (rank-1 inference for generic constructors like `Ok`).
    fn instantiate_return(&self, params: &[Ty], args: &[Ty], ret: &Ty) -> Ty {
        Self::apply_subst(ret, &self.param_subst(params, args))
    }

    /// The substitution binding each of a function's type parameters to the
    /// concrete type the matching argument supplied, inferred by unifying every
    /// parameter type against its argument.
    fn param_subst(&self, params: &[Ty], args: &[Ty]) -> std::collections::HashMap<String, Ty> {
        let mut subst = std::collections::HashMap::new();
        for (p, a) in params.iter().zip(args) {
            self.unify_param(p, a, &mut subst);
        }
        subst
    }

    /// Enforces the callee's declared trait bounds against the concrete types
    /// its type parameters were instantiated to (`STATIC.md` §3.5). Applies only
    /// to a direct call of a named function or constructor whose bounds were
    /// collected; an indirect call through a value carries no bounds to check.
    fn check_call_bounds(
        &mut self,
        callee: &ast::Expr,
        subst: &std::collections::HashMap<String, Ty>,
        span: ast::Span,
    ) {
        let ast::ExprKind::Path(segs) = &callee.kind else {
            return;
        };
        let Some(name) = segs.last() else { return };
        self.check_collected_bounds(name, subst, span);
    }

    /// Enforces the bounds collected under `name` (a function, constructor, or
    /// generic type) against the substitution that instantiated its parameters,
    /// pushing one diagnostic per parameter whose concrete type fails a bound.
    fn check_collected_bounds(
        &mut self,
        name: &str,
        subst: &std::collections::HashMap<String, Ty>,
        span: ast::Span,
    ) {
        let Some(bounds) = self.table.fn_bounds.get(name).cloned() else {
            return;
        };
        for (param, traits) in &bounds {
            let Some(concrete) = subst.get(param) else {
                continue;
            };
            for tr in traits {
                if !self.type_satisfies(concrete, tr) {
                    self.error_at(
                        span,
                        format!("the type `{concrete}` does not satisfy the bound `{tr}`"),
                    );
                }
            }
        }
    }

    /// Binds the type parameters in `param` from the matching parts of `arg`.
    /// A parameter typed as an applied generic trait (`Source<T>`) reads its
    /// arguments from the application `arg` has of that trait — directly, via
    /// a subtrait, or through an impl — so `pull(c)` with `impl Source<number>
    /// for C` binds `T` to `number` (`STATIC.md` §3.4).
    fn unify_param(&self, param: &Ty, arg: &Ty, subst: &mut std::collections::HashMap<String, Ty>) {
        match (param, arg) {
            (Ty::Param(name), a) if !a.is_unknown() => {
                subst.entry(name.clone()).or_insert_with(|| a.clone());
            }
            (Ty::List(p), Ty::List(a)) => self.unify_param(p, a, subst),
            (Ty::Named { name, args: pa }, a) if !pa.is_empty() && self.is_trait(name) => {
                if let Some(applied) = self.application_of(a, name, pa.len()) {
                    for (p, a) in pa.iter().zip(&applied.args) {
                        self.unify_param(p, a, subst);
                    }
                }
            }
            (Ty::Named { args: pa, .. }, Ty::Named { args: aa, .. }) if pa.len() == aa.len() => {
                for (p, a) in pa.iter().zip(aa) {
                    self.unify_param(p, a, subst);
                }
            }
            // Recurse through function types so a method-level generic in a
            // closure's return position binds — `map<U>(f: fn(T) -> U)` reads `U`
            // from the closure argument's inferred return type.
            (Ty::Fn(pp, pr), Ty::Fn(ap, ar)) if pp.len() == ap.len() => {
                for (p, a) in pp.iter().zip(ap) {
                    self.unify_param(p, a, subst);
                }
                self.unify_param(pr, ar, subst);
            }
            _ => {}
        }
    }

    fn apply_subst(ty: &Ty, subst: &std::collections::HashMap<String, Ty>) -> Ty {
        ty.map(&|t| match t {
            Ty::Param(name) => Some(subst.get(name).cloned().unwrap_or(Ty::Unknown)),
            _ => None,
        })
    }

    /// Substitute the type parameters in `ty` from `subst`, leaving any
    /// parameter not in the map unchanged. Unlike [`Self::apply_subst`] (which
    /// erases unmapped parameters to `Unknown` for call-return instantiation),
    /// this is for projecting a field or method-return type of a nominal type
    /// through its concrete type arguments, where a parameter outside the
    /// nominal type's own generics must survive.
    fn subst_named(ty: &Ty, subst: &std::collections::HashMap<String, Ty>) -> Ty {
        ty.map(&|t| match t {
            Ty::Param(name) => Some(subst.get(name).cloned().unwrap_or_else(|| t.clone())),
            _ => None,
        })
    }

    /// The substitution mapping a nominal type's generic parameter names to the
    /// concrete type arguments carried by `ty`. Returns an empty map when `ty`
    /// is not a generic nominal type or carries no arguments.
    fn nominal_subst(&self, ty: &Ty) -> std::collections::HashMap<String, Ty> {
        let mut subst = std::collections::HashMap::new();
        let Ty::Named { name, args } = ty else {
            return subst;
        };
        let generics = match self.table.types.get(name) {
            Some(TypeDef::Struct(s)) => &s.generics,
            Some(TypeDef::Enum(e)) => &e.generics,
            _ => return subst,
        };
        for (param, arg) in generics.iter().zip(args) {
            subst.insert(param.clone(), arg.clone());
        }
        subst
    }

    fn infer_method_call(
        &mut self,
        receiver: &ast::Expr,
        method: &str,
        method_span: ast::Span,
        args: &[ast::Expr],
        span: ast::Span,
    ) -> Ty {
        let recv = self.infer(receiver);
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
        // A method on a bounded type parameter resolves through its bounds:
        // `fn f<T: Display>(x: T) { x.show() }` finds `show` on `Display`.
        if let Ty::Param(p) = &recv {
            if let Some((params, ret)) = self.bound_method(p, method) {
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(
                span,
                format!("type parameter `{p}` has no method `{method}` from its bounds"),
            );
            return Ty::Unknown;
        }
        let Ty::Named {
            name,
            args: recv_args,
        } = &recv
        else {
            if recv.is_unknown() {
                return Ty::Unknown;
            }
            // A method call on a dynamic value is the foreign boundary: it is
            // permitted and yields `Unknown` again (`STATIC.md` §3).
            if recv.is_dynamic() {
                return Ty::Dynamic;
            }
            self.error_at(
                receiver.span,
                format!("type `{recv}` has no method `{method}`"),
            );
            return Ty::Unknown;
        };
        // A method on a trait used as a type (the existential) resolves through
        // the trait's own methods, with `Self` read as the trait type.
        if self.is_trait(name) {
            let applied = TraitRef {
                name: name.clone(),
                args: recv_args.clone(),
            };
            if let Some((params, ret)) = self.trait_method(&applied, method) {
                self.record_member(method_span, name, method);
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(span, format!("type `{name}` has no method `{method}`"));
            return Ty::Unknown;
        }
        self.record_member(method_span, name, method);
        let Some(m) = self.table.methods.get(&(name.clone(), method.to_string())) else {
            // A trait the type implements may supply the method as a default
            // the impl did not override.
            if let Some((params, ret)) = self.default_method(&recv, method, true) {
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            // A blanket impl may grant the method: if the receiver satisfies a
            // blanket's conditions and its trait declares `method`, resolve
            // through that trait with `Self` read as the concrete type.
            if let Some((params, ret)) = self.blanket_method(&recv, method) {
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(span, format!("type `{name}` has no method `{method}`"));
            return Ty::Unknown;
        };
        // Project the method signature through the receiver's concrete type
        // arguments, so `Box<number>::get(self) -> T` yields `number`.
        let nominal = self.nominal_subst(&recv);
        let mut params: Vec<Ty> = m
            .params
            .iter()
            .map(|p| Self::subst_named(p, &nominal))
            .collect();
        let mut ret = Self::subst_named(&m.ret, &nominal);
        // A method-level generic (one not among the receiver's type arguments —
        // `map`'s `U`, `map_err`'s `F`, `ok_or`'s `E`) survives nominal
        // projection as a free `Ty::Param`. Bind it by unifying the projected
        // parameters against the argument types, then apply to the parameters
        // (so the call type-checks) and the return (so `opt.map(f)` yields
        // `Option<U>` with `U` resolved, not the bare parameter).
        let method_subst = self.param_subst(&params, &arg_tys);
        if !method_subst.is_empty() {
            for p in &mut params {
                *p = Self::subst_named(p, &method_subst);
            }
            ret = Self::subst_named(&ret, &method_subst);
        }
        self.check_call_args(method, &params, &arg_tys, args, span);
        ret
    }

    fn infer_field(&mut self, base: &ast::Expr, field: &ast::FieldAccess, span: ast::Span) -> Ty {
        let base_ty = self.infer(base);
        let Ty::Named { name, .. } = &base_ty else {
            if base_ty.is_unknown() {
                return Ty::Unknown;
            }
            // Field access through a dynamic value yields `Unknown` again.
            if base_ty.is_dynamic() {
                return Ty::Dynamic;
            }
            self.error_at(base.span, format!("type `{base_ty}` has no fields"));
            return Ty::Unknown;
        };
        let Some(TypeDef::Struct(s)) = self.table.types.get(name) else {
            self.error_at(
                base.span,
                format!("type `{name}` is not a struct with fields"),
            );
            return Ty::Unknown;
        };
        // Project the field type through the base's concrete type arguments, so
        // `Box<number>.value` yields `number` rather than the parameter `T`.
        let subst = self.nominal_subst(&base_ty);
        match (field, &s.fields) {
            (ast::FieldAccess::Named(f, fspan), StructFields::Named(fields)) => {
                if let Some((_, ty)) = fields.iter().find(|(n, _)| n == f) {
                    let ty = Self::subst_named(ty, &subst);
                    self.record_member(*fspan, name, f);
                    ty
                } else {
                    self.error_at(span, format!("struct `{name}` has no field `{f}`"));
                    Ty::Unknown
                }
            }
            (ast::FieldAccess::Tuple(0), StructFields::Tuple(ty)) => Self::subst_named(ty, &subst),
            _ => {
                self.error_at(span, format!("invalid field access on `{name}`"));
                Ty::Unknown
            }
        }
    }

    fn infer_struct_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldInit],
        base: Option<&ast::Expr>,
        span: ast::Span,
    ) -> Ty {
        // Struct-variant literal `Enum::Variant { .. }`.
        if path.len() >= 2 {
            return self.infer_variant_lit(path, fields, span);
        }
        let Some((name, s)) = self.struct_lit_target(path, span) else {
            self.infer_field_inits(fields);
            return Ty::Unknown;
        };
        let StructFields::Named(decl) = &s.fields else {
            self.error_at(span, format!("`{name}` is not a struct with named fields"));
            return Ty::named(&name);
        };
        self.check_named_fields(&name, decl, fields, base.is_some());
        // Infer the struct's type arguments by unifying each declared field
        // type against its initialiser's type, so `Box { value: 99 }` yields
        // `Box<number>` and downstream field/method projection (§generics) sees
        // the concrete argument.
        let result = self.infer_nominal_args(&name, &s.generics, decl, fields, base, span);
        if let Some(b) = base {
            let bt = self.infer(b);
            self.expect(&Ty::named(&name), &bt, "struct update base");
        }
        result
    }

    /// Infers each initialiser of a literal that could not be resolved, so errors
    /// inside them still surface.
    fn infer_field_inits(&mut self, fields: &[ast::FieldInit]) {
        for f in fields {
            if let Some(v) = &f.value {
                let _ = self.infer(v);
            }
        }
    }

    /// Build the `Ty::Named` for a struct literal, inferring its type arguments
    /// (when the struct is generic) by unifying each declared field type against
    /// the initialiser's inferred type. A non-generic struct returns
    /// `Ty::named(name)`. A parameter with no initialiser to pin it stays
    /// `Unknown`.
    fn infer_nominal_args(
        &mut self,
        name: &str,
        generics: &[String],
        decl: &[(String, Ty)],
        inits: &[ast::FieldInit],
        base: Option<&ast::Expr>,
        span: ast::Span,
    ) -> Ty {
        if generics.is_empty() {
            return Ty::named(name);
        }
        let mut subst = std::collections::HashMap::new();
        for (fname, declared) in decl {
            let actual = inits.iter().find(|i| i.name == *fname).and_then(|init| {
                init.value
                    .as_ref()
                    .map(|e| self.infer(e))
                    .or_else(|| self.shorthand_ty(fname, false))
            });
            if let Some(actual) = actual {
                self.unify_param(declared, &actual, &mut subst);
            }
        }
        // A `..base` update copies the remaining fields from a value of this
        // same type, so its arguments fill any parameter the inits did not pin.
        if let Some(b) = base {
            if let Ty::Named { name: bn, args } = self.infer(b) {
                if bn == name {
                    for (param, arg) in generics.iter().zip(args) {
                        subst.entry(param.clone()).or_insert(arg);
                    }
                }
            }
        }
        // The named-literal construction site: a parameter's bound must be
        // satisfied by the type its field initialisers pinned it to (§3.5).
        self.check_collected_bounds(name, &subst, span);
        let args = generics
            .iter()
            .map(|g| subst.get(g).cloned().unwrap_or(Ty::Unknown))
            .collect();
        Ty::Named {
            name: name.to_string(),
            args,
        }
    }

    fn infer_variant_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldInit],
        span: ast::Span,
    ) -> Ty {
        let enum_name = &path[path.len() - 2];
        let variant = &path[path.len() - 1];
        let Some(TypeDef::Enum(e)) = self.table.types.get(enum_name) else {
            self.error_at(span, format!("unknown enum `{enum_name}`"));
            return Ty::Unknown;
        };
        let Some(v) = e.variants.iter().find(|v| v.name == *variant) else {
            self.error_at(
                span,
                format!("enum `{enum_name}` has no variant `{variant}`"),
            );
            return Ty::Unknown;
        };
        let VariantFields::Named(decl) = &v.fields else {
            self.error_at(
                span,
                format!("variant `{enum_name}::{variant}` does not have named fields"),
            );
            return Ty::named(enum_name);
        };
        let decl = decl.clone();
        self.check_named_fields(&format!("{enum_name}::{variant}"), &decl, fields, false);
        Ty::named(enum_name)
    }

    fn check_named_fields(
        &mut self,
        owner: &str,
        decl: &[(String, Ty)],
        inits: &[ast::FieldInit],
        has_base: bool,
    ) {
        for init in inits {
            match decl.iter().find(|(n, _)| *n == init.name) {
                Some((_, expected)) => {
                    let actual = match &init.value {
                        Some(e) => self.infer(e),
                        None => self.shorthand_ty(&init.name, true).unwrap_or(Ty::Unknown),
                    };
                    if !self.assignable(expected, &actual) {
                        self.error(format!(
                            "field `{}` of `{owner}` expects `{expected}`, found `{actual}`",
                            init.name
                        ));
                    }
                }
                None => self.error(format!("`{owner}` has no field `{}`", init.name)),
            }
        }
        if !has_base {
            for (fname, _) in decl {
                if !inits.iter().any(|i| i.name == *fname) {
                    self.error(format!("missing field `{fname}` in `{owner}`"));
                }
            }
        }
    }

    fn infer_if(&mut self, cond: &ast::Expr, then: &ast::Block, else_: Option<&ast::Expr>) -> Ty {
        let c = self.infer(cond);
        self.expect(&Ty::Bool, &c, "`if` condition");
        let then_ty = self.infer_block(then);
        match else_ {
            Some(e) => {
                let else_ty = self.infer(e);
                if !then_ty.compatible(&else_ty) {
                    self.error(format!(
                        "`if` branches disagree: `{then_ty}` vs `{else_ty}`"
                    ));
                    Ty::Unknown
                } else if then_ty.is_unknown() {
                    else_ty
                } else {
                    then_ty
                }
            }
            // An `if` with no `else` is a statement of type `()`.
            None => Ty::Unit,
        }
    }

    fn infer_list(&mut self, items: &[ast::Expr]) -> Ty {
        let tys: Vec<Ty> = items.iter().map(|i| self.infer(i)).collect();
        // An empty list needs an annotation; with none, its element is unknown.
        let Some(first) = tys.first() else {
            return Ty::List(Box::new(Ty::Unknown));
        };
        // Homogeneous (modulo `Unknown`/`Param`): the first type is the element.
        if tys.iter().all(|t| first.compatible(t)) {
            return Ty::List(Box::new(first.clone()));
        }
        // Heterogeneous: if every element's concrete type implements exactly one
        // common trait, the list is of that trait — the existential element type
        // (`STATIC.md` §3.4). A shared set of zero or several traits is ambiguous
        // and reported as a disagreement (the author annotates to choose).
        if let Some(trait_ty) = self.common_trait(&tys) {
            return Ty::List(Box::new(trait_ty));
        }
        let clash = tys.iter().find(|t| !first.compatible(t)).unwrap_or(first);
        self.error(format!("list elements disagree: `{first}` vs `{clash}`"));
        Ty::List(Box::new(first.clone()))
    }

    /// The single trait every element type implements, when exactly one exists
    /// — the element type of a heterogeneous list of implementors. `None` when
    /// the element types share no trait, share more than one (ambiguous), or
    /// any element is not a concrete nominal type.
    fn common_trait(&self, tys: &[Ty]) -> Option<Ty> {
        let mut common: Option<Vec<TraitRef>> = None;
        for t in tys {
            ty_impl_name(t)?;
            let traits = self.direct_applications(t);
            common = Some(match common {
                None => traits,
                Some(mut c) => {
                    c.retain(|tr| traits.contains(tr));
                    c
                }
            });
        }
        match common?.as_slice() {
            [only] => Some(only.to_ty()),
            _ => None,
        }
    }

    fn infer_index(&mut self, base: &ast::Expr, index: &ast::Expr, span: ast::Span) -> Ty {
        let b = self.infer(base);
        let i = self.infer(index);
        self.expect(&Ty::Number, &i, "list index");
        match b {
            Ty::List(inner) => *inner,
            Ty::Unknown => Ty::Unknown,
            // Indexing through a dynamic value yields `Unknown` again.
            Ty::Dynamic => Ty::Dynamic,
            other => {
                self.error_at(span, format!("cannot index into `{other}`"));
                Ty::Unknown
            }
        }
    }

    fn infer_closure(&mut self, params: &[ast::ClosureParam], body: &ast::Expr) -> Ty {
        self.scopes.push();
        let mut ptys = Vec::new();
        for p in params {
            let ty =
                p.ty.as_ref()
                    .map_or(Ty::Unknown, |t| self.lower_at(t, p.span));
            self.bind_local(
                p.name.clone(),
                ty.clone(),
                p.span,
                crate::resolve::BindingKind::ClosureParam,
            );
            ptys.push(ty);
        }
        let ret = self.infer(body);
        self.scopes.pop();
        Ty::Fn(ptys, Box::new(ret))
    }

    // ---- `?` (STATIC.md §6) -----------------------------------------------

    fn infer_try(&mut self, inner: &ast::Expr) -> Ty {
        let t = self.infer(inner);
        match &t {
            Ty::Named { name, args } if name == "Result" && args.len() == 2 => match &self.ret_ty {
                Ty::Named { name: rn, args: ra } if rn == "Result" && ra.len() == 2 => {
                    if !args[1].compatible(&ra[1]) {
                        self.error(format!(
                            "`?` error type `{}` does not match the function's error type `{}`",
                            args[1], ra[1]
                        ));
                    }
                    args[0].clone()
                }
                _ => {
                    self.error(
                        "`?` on a `Result` is only valid in a function returning `Result`"
                            .to_string(),
                    );
                    args[0].clone()
                }
            },
            Ty::Named { name, args } if name == "Option" && args.len() == 1 => match &self.ret_ty {
                Ty::Named { name: rn, .. } if rn == "Option" => args[0].clone(),
                _ => {
                    self.error(
                        "`?` on an `Option` is only valid in a function returning `Option`"
                            .to_string(),
                    );
                    args[0].clone()
                }
            },
            Ty::Unknown => Ty::Unknown,
            other => {
                self.error(format!(
                    "`?` expects a `Result` or `Option`, found `{other}`"
                ));
                Ty::Unknown
            }
        }
    }

    // ---- match (STATIC.md §7) ---------------------------------------------

    fn infer_match(
        &mut self,
        scrutinee: &ast::Expr,
        arms: &[ast::MatchArm],
        span: ast::Span,
    ) -> Ty {
        let scrut = self.infer(scrutinee);
        let mut result: Option<Ty> = None;
        for arm in arms {
            self.scopes.push();
            self.bind_pattern(&arm.pattern, &scrut, crate::resolve::BindingKind::MatchArm);
            if let Some(g) = &arm.guard {
                let gt = self.infer(g);
                self.expect(&Ty::Bool, &gt, "match guard");
            }
            let body_ty = self.infer(&arm.body);
            self.scopes.pop();
            match &result {
                None => result = Some(body_ty),
                Some(r) => {
                    if !r.compatible(&body_ty) {
                        self.error_at(
                            arm.body.span,
                            format!("match arms disagree: `{r}` vs `{body_ty}`"),
                        );
                    }
                }
            }
        }
        self.check_exhaustive(&scrut, arms, span);
        result.unwrap_or(Ty::Unit)
    }

    // ---- shared --------------------------------------------------------------

    /// Records a mismatch diagnostic when `actual` may not be used where
    /// `expected` is required. `Unknown` on either side suppresses the report.
    fn expect(&mut self, expected: &Ty, actual: &Ty, context: &str) {
        if !self.assignable(expected, actual) {
            self.error(format!("{context} expects `{expected}`, found `{actual}`"));
        }
    }

    /// Whether a value of type `actual` may be used where `expected` is
    /// required. This is [`Ty::compatible`] plus the existential rule: when
    /// `expected` is a trait name used as a type, any concrete type that
    /// implements the trait — or the same trait itself — is admissible (the
    /// trait-as-type existential, `STATIC.md` §3.4). The relation is
    /// directional: an implementor flows into the trait, never the reverse
    /// (neoc has no implicit downcast). It recurses through lists, function
    /// types, and nominal type arguments, so `[Drawable]` accepts `[Circle]`.
    fn assignable(&self, expected: &Ty, actual: &Ty) -> bool {
        expected.relate(actual, &|expected, actual| {
            self.existential(expected, actual)
        })
    }

    /// The trait-as-type existential rule, as a [`Ty::relate`] hook. When
    /// `expected` is a trait name used directly as a type, any concrete type
    /// that implements it — or the same trait or a subtrait — is admissible
    /// (`STATIC.md` §3.4), directional: an implementor flows into the trait,
    /// never the reverse. Returns `None` (defer to the structural relation)
    /// when `expected` is not a trait-as-type, or when `actual` is a wildcard
    /// (`Unknown`/`Param`), which `relate` accepts unconditionally — so a value
    /// of unbounded parameter type is still passable where a trait is expected.
    fn existential(&self, expected: &Ty, actual: &Ty) -> Option<bool> {
        let Ty::Named { name, args } = expected else {
            return None;
        };
        if !self.is_trait(name) {
            return None;
        }
        if matches!(actual, Ty::Unknown | Ty::Param(_)) {
            return None;
        }
        let wanted = TraitRef {
            name: name.clone(),
            args: args.clone(),
        };
        Some(match actual {
            // A trait value flows into the same trait or a supertrait, at the
            // arguments the supertrait chain maps its own to.
            Ty::Named {
                name: a,
                args: a_args,
            } if self.is_trait(a) => {
                let from = TraitRef {
                    name: a.clone(),
                    args: a_args.clone(),
                };
                self.trait_reaches(&from, &wanted)
            }
            // An implementor flows into `Trait<A, …>` only through an impl (or
            // a subtrait's impl) applying the trait to those same arguments.
            _ => self
                .trait_applications(actual)
                .iter()
                .any(|tr| self.trait_reaches(tr, &wanted)),
        })
    }

    /// Whether `name` names a trait in the type namespace.
    fn is_trait(&self, name: &str) -> bool {
        matches!(self.table.types.get(name), Some(TypeDef::Trait(_)))
    }
}

/// The name a type registers under in the trait-impl table: a nominal type's
/// own name, or a primitive's spelling. List and function types carry no
/// nominal identity, so they cannot be the target of an `impl` — `None`.
fn ty_impl_name(ty: &Ty) -> Option<String> {
    match ty {
        Ty::Named { name, .. } => Some(name.clone()),
        Ty::Number => Some("number".to_string()),
        Ty::Str => Some("string".to_string()),
        Ty::Bool => Some("bool".to_string()),
        _ => None,
    }
}

/// The instantiated signatures of every generic call in `module`, each keyed by
/// the callee identifier's byte range. Runs the checker with call recording on,
/// then resolves each recorded call against its callee's declaration. Backs
/// [`call_signatures`](crate::call_signatures).
pub(crate) fn call_signatures_of(module: &ast::Module, table: &SymbolTable) -> Vec<CallSignature> {
    let mut checker = Checker::new(table);
    checker.record_calls();
    checker.check_module(module);
    checker
        .into_calls()
        .into_iter()
        .filter_map(|site| build_call_signature(module, table, &site))
        .collect()
}

/// Resolves a recorded [`CallSite`] into a renderable [`CallSignature`]: pairs
/// the callee's declared parameter *names* (from the AST) with their *types*
/// (from the symbol table) instantiated through the call's substitution, and
/// orders the type arguments by the callee's declared generics. `None` when the
/// callee is not a top-level function — a constructor or enum variant has no
/// parameter names to render.
fn build_call_signature(
    module: &ast::Module,
    table: &SymbolTable,
    site: &CallSite,
) -> Option<CallSignature> {
    let Some(Ty::Fn(param_tys, ret)) = table.values.get(&site.name) else {
        return None;
    };
    let sig = find_fn_sig(module, &site.name)?;
    // The names and `param_tys` are both `sig.params`' `Named` entries in
    // declaration order — `collect::fn_type` lowered the same params into the
    // stored `Ty::Fn`, and a top-level `fn` has no receiver to drop — so the zip
    // pairs each name with its own type.
    let params = sig
        .params
        .iter()
        .filter_map(|p| match p {
            ast::Param::Named { name, .. } => Some(name),
            ast::Param::Receiver => None,
        })
        .zip(param_tys)
        .map(|(name, ty)| (name.clone(), Checker::apply_subst(ty, &site.subst)))
        .collect();
    let type_args = sig
        .generics
        .iter()
        .map(|g| {
            site.subst
                .get(&g.name)
                .cloned()
                .unwrap_or_else(|| Ty::Param(g.name.clone()))
        })
        .collect();
    Some(CallSignature {
        start: site.start,
        end: site.end,
        type_args,
        params,
        ret: Checker::apply_subst(ret, &site.subst),
    })
}

/// The signature of the top-level function named `name`, if `module` declares
/// one.
fn find_fn_sig<'a>(module: &'a ast::Module, name: &str) -> Option<&'a ast::FnSig> {
    module.items.iter().find_map(|item| match &item.kind {
        ast::ItemKind::Fn(decl) if decl.sig.name == name => Some(&decl.sig),
        _ => None,
    })
}

/// The bare names `item` binds at module scope, each with the span to report
/// it at (`STATIC.md` §4.3): an item's own name, the name a `use` binds (its
/// alias, else the path's last segment), and each `fn`/`struct`/`mod` at the
/// top level of an `extern` block. An `extern` item nested in a `mod` is
/// reached as `m.f`, never bare, so it is not one.
fn module_scope_names(item: &ast::Item) -> Vec<(&str, ast::Span)> {
    let span = item.span;
    match &item.kind {
        ast::ItemKind::Fn(f) => vec![(f.sig.name.as_str(), f.sig.name_span)],
        ast::ItemKind::Const(c) => vec![(c.name.as_str(), span)],
        ast::ItemKind::TypeAlias(t) => vec![(t.name.as_str(), span)],
        ast::ItemKind::Struct(s) => vec![(s.name.as_str(), span)],
        ast::ItemKind::Enum(e) => vec![(e.name.as_str(), span)],
        ast::ItemKind::Trait(t) => vec![(t.name.as_str(), span)],
        ast::ItemKind::Use(u) => u
            .alias
            .as_ref()
            .or(u.path.last())
            .map_or_else(Vec::new, |bound| vec![(bound.as_str(), span)]),
        ast::ItemKind::Extern(block) => block
            .items
            .iter()
            .filter_map(|ext| match &ext.kind {
                ast::ExternItemKind::Fn(sig) => Some((sig.name.as_str(), sig.name_span)),
                ast::ExternItemKind::Struct(decl) => Some((decl.name.as_str(), span)),
                ast::ExternItemKind::Mod(module) => Some((module.name.as_str(), span)),
                ast::ExternItemKind::Impl(_) => None,
            })
            .collect(),
        ast::ItemKind::Impl(_) => Vec::new(),
    }
}

/// Every `impl` in an `extern` block's items, recursing through `mod`, in
/// source order.
fn collect_extern_impls<'a>(
    items: &'a [ast::ExternItem],
    out: &mut Vec<(&'a ast::ImplBlock, ast::Span, bool)>,
) {
    for ext in items {
        match &ext.kind {
            ast::ExternItemKind::Impl(b) => out.push((b, ext.span, true)),
            ast::ExternItemKind::Mod(m) => collect_extern_impls(&m.items, out),
            _ => {}
        }
    }
}

/// Who first gave a type a method name: the impl's label, its trait, and its
/// position among the module's impls.
type Owner = (String, Option<String>, usize);
