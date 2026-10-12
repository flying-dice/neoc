//! Trait and impl well-formedness — the checks that an `impl` (and the trait
//! it implements) is complete and consistent: required methods present, no
//! extras, signatures conform, associated types bound, supertraits satisfied.
//! These emit the trait/impl diagnostics. A child module of `infer`, so it
//! shares the checker's fields and helpers; distinct from inference per the
//! type-system charter.

use neoc_ast as ast;

use super::Checker;
use crate::env::{TraitInfo, TraitMethod, TraitRef, TypeDef};
use crate::ty::Ty;

impl Checker<'_> {
    pub(super) fn check_impl(&mut self, b: &ast::ImplBlock, span: ast::Span) {
        let Some(target) = crate::collect::impl_target_name(self.table, b) else {
            return;
        };
        // An alias that does not name a type declared here (imported, or a
        // list/function type) has no table to attach methods to.
        if let ast::Type::Path(p) = &b.ty
            && let Some(written) = p.segments.last().map(|s| &s.name)
            && !b.generics.iter().any(|g| g.name == *written)
            && matches!(self.table.types.get(written), Some(TypeDef::Alias { .. }))
            && self.table.alias_head(written).is_none()
        {
            self.error_at(
                span,
                format!(
                    "`impl {written}` targets a type alias that does not name a struct or enum declared in this module; write the type itself"
                ),
            );
            return;
        }
        self.check_impl_params_constrained(b, span);
        // Trait-impl completeness (`STATIC.md` §8).
        if let Some(trait_path) = &b.trait_ {
            self.check_trait_impl(b, &target, trait_path, span);
        }
        // Bring the impl's own generic parameters and their bounds into scope,
        // so a generic impl's `T` (and, for a blanket impl, the target param's
        // bounds) is visible in every method body.
        let prev_params = std::mem::take(&mut self.type_params);
        let prev_bounds = std::mem::take(&mut self.type_bounds);
        self.type_params = b.generics.iter().map(|g| g.name.clone()).collect();
        self.type_bounds =
            self.scoped_bounds(crate::collect::generic_param_bounds(&b.generics, &[]));
        // Bring the impl's associated-type bindings into scope so `Self::Item`
        // in method signatures and bodies resolves to the bound type.
        let prev_assoc = std::mem::take(&mut self.assoc_bindings);
        self.assoc_bindings = b
            .assoc_bindings
            .iter()
            .map(|(name, ty)| (name.clone(), self.lower(ty)))
            .collect();
        let prev_self = self.impl_self_ty.replace(self.lower(&b.ty));
        for f in &b.items {
            self.check_fn(f, Some(&target));
        }
        self.impl_self_ty = prev_self;
        self.assoc_bindings = prev_assoc;
        self.type_params = prev_params;
        self.type_bounds = prev_bounds;
    }

    fn check_trait_impl(
        &mut self,
        b: &ast::ImplBlock,
        target: &str,
        trait_path: &ast::TypePath,
        span: ast::Span,
    ) {
        let Some(trait_name) = trait_path.segments.last().map(|s| &s.name) else {
            return;
        };
        let Some(TypeDef::Trait(t)) = self.table.types.get(trait_name) else {
            self.error(format!(
                "unknown trait `{trait_name}` in impl for `{target}`"
            ));
            return;
        };
        // A blanket impl `impl<T: …> Trait for T` targets one of its own
        // generics; `Self` reads as that parameter, and supertrait coverage is
        // checked against the parameter's bounds rather than a concrete impl.
        let is_blanket = b.generics.iter().any(|g| g.name == target);
        // The application collection recorded for this impl (`impl
        // Source<number> for X` → `Source<number>`); its argument count must
        // match the trait's parameters.
        let trait_ref = self.recorded_application(target, trait_name, is_blanket);
        // A wrong count is reported with the impl header's other type arity.
        let arity_ok = trait_ref.args.len() == t.generics.len();
        let impl_generics = crate::collect::generic_names(&b.generics);
        // Supertrait completeness: an implementor must also implement every
        // supertrait the trait requires, at the arguments the trait's
        // declaration maps this impl's to (`STATIC.md` §3.4).
        let target_ty = self
            .table
            .resolve_alias_ty(crate::collect::lower_type(&b.ty, &impl_generics));
        for sup in &t.supertraits {
            // A supertrait spelled with the wrong arity is reported on the
            // trait; coverage against it would only repeat that error.
            if !self.has_trait_arity(sup) {
                continue;
            }
            let sup = sup.instantiate(&t.generics, &trait_ref.args);
            let covered = if is_blanket {
                crate::collect::generic_param_bounds(&b.generics, &[])
                    .get(target)
                    .is_some_and(|conds| conds.iter().any(|c| self.trait_reaches(c, &sup)))
            } else {
                self.trait_applications(&target_ty)
                    .iter()
                    .any(|tr| tr.name != trait_ref.name && self.trait_reaches(tr, &sup))
            };
            if !covered {
                self.error_at(
                    span,
                    format!(
                        "`{target}` implements `{trait_name}` but not its supertrait `{sup}`; add `impl {sup} for {target}`"
                    ),
                );
            }
        }
        let provided: Vec<&String> = b.items.iter().map(|f| &f.sig.name).collect();
        // Missing required methods.
        for m in &t.methods {
            if !m.has_default && !provided.iter().any(|p| **p == m.name) {
                self.error(format!(
                    "`impl {trait_name} for {target}` is missing method `{}`",
                    m.name
                ));
            }
        }
        // Extraneous methods not declared by the trait.
        for f in &b.items {
            if !t.methods.iter().any(|m| m.name == f.sig.name) {
                self.error(format!(
                    "method `{}` is not a member of trait `{trait_name}`",
                    f.sig.name
                ));
            }
        }
        let inherited: Vec<&str> = t
            .methods
            .iter()
            .filter(|m| m.has_default && !provided.iter().any(|p| **p == m.name))
            .map(|m| m.name.as_str())
            .collect();
        if is_blanket {
            // A blanket impl lands on every satisfying type's table
            // (`DYNAMIC.md` §3.5), so each is checked as its own impl.
            let conditions = crate::collect::generic_param_bounds(&b.generics, &[])
                .remove(target)
                .unwrap_or_default();
            for ty in self.blanket_targets(target, &conditions) {
                let label = format!("`impl {trait_name} for {target}` (for `{ty}`)");
                self.check_inherited_defaults(&label, &ty, trait_name, &inherited);
                self.check_blanket_methods(&label, &ty, trait_name, &provided);
            }
        } else {
            let label = format!("`impl {trait_name} for {target}`");
            self.check_inherited_defaults(&label, target, trait_name, &inherited);
        }
        // Associated-type completeness: every declared `type Item;` must be
        // bound, and a binding must name a declared associated type.
        for at in &t.assoc_types {
            if !b.assoc_bindings.iter().any(|(n, _)| n == at) {
                self.error(format!(
                    "`impl {trait_name} for {target}` is missing associated type `{at}`"
                ));
            }
        }
        for (n, _) in &b.assoc_bindings {
            if !t.assoc_types.iter().any(|at| at == n) {
                self.error(format!(
                    "`{n}` is not an associated type of trait `{trait_name}`"
                ));
            }
        }
        let conflicting = self
            .conflicting_impls
            .contains(&(target.to_string(), trait_ref.name.clone()));
        if arity_ok && !conflicting {
            self.check_sig_conformance(b, target, &trait_ref, t, is_blanket);
        }
    }

    /// Rejects each default the impl `label` of `trait_name` gives concrete
    /// type `target` whose name the type already has — an inherent or
    /// trait-impl method, or another implemented trait's default. The type's
    /// one method table can hold only one of them, so the call would resolve
    /// differently statically and at runtime (`STATIC.md` §8).
    fn check_inherited_defaults(
        &mut self,
        label: &str,
        target: &str,
        trait_name: &str,
        inherited: &[&str],
    ) {
        for &m in inherited {
            let clash = if self
                .table
                .methods
                .contains_key(&(target.to_string(), m.to_string()))
                || self.blanket_method_source(target, m, trait_name).is_some()
            {
                format!("`{target}` already has a method `{m}`")
            } else if let Some(other) = self
                .default_sources(target, m)
                .into_iter()
                .find(|t| t != trait_name)
            {
                format!("trait `{other}` also gives `{target}` a default `{m}`")
            } else {
                continue;
            };
            self.error(format!(
                "{label} inherits default method `{m}`, but {clash}; override `{m}` in the impl"
            ));
        }
    }

    /// Rejects each method the blanket impl `label` of `trait_name` provides
    /// itself to concrete type `target` when another trait gives `target` an
    /// inherited default of the same name — the two land on one table
    /// (`STATIC.md` §8).
    fn check_blanket_methods(
        &mut self,
        label: &str,
        target: &str,
        trait_name: &str,
        provided: &[&String],
    ) {
        for &m in provided {
            let Some(other) = self
                .default_sources(target, m)
                .into_iter()
                .find(|t| t != trait_name && self.inherits_default(target, t, m))
            else {
                continue;
            };
            self.error(format!(
                "{label} gives `{target}` a method `{m}`, but trait `{other}` also gives `{target}` a default `{m}`; override `{m}` in that trait's impl"
            ));
        }
    }

    /// Rejects an impl type parameter its target does not mention (`impl<U>
    /// Source<U> for Counter`): nothing at a use site fixes it, so it would
    /// reach callers as an unconstrained parameter (`STATIC.md` §3.5).
    fn check_impl_params_constrained(&mut self, b: &ast::ImplBlock, span: ast::Span) {
        let generics = crate::collect::generic_names(&b.generics);
        let target = crate::collect::lower_type(&b.ty, &generics);
        for g in generics.iter().filter(|g| !mentions_param(&target, g)) {
            self.error_at(
                span,
                format!("type parameter `{g}` of the impl is not used by its target `{target}`"),
            );
        }
    }

    /// The trait application collection recorded for the impl of `trait_name`
    /// on `target` — the blanket's for a blanket impl — with aliases already
    /// expanded. A non-generic trait records none: it applies to no arguments.
    fn recorded_application(&self, target: &str, trait_name: &str, is_blanket: bool) -> TraitRef {
        let recorded = if is_blanket {
            self.table
                .blanket_impls
                .iter()
                .find(|bl| bl.param == target && bl.trait_ref.name == trait_name)
                .map(|bl| bl.trait_ref.clone())
        } else {
            self.table
                .trait_impl_applications
                .get(&(target.to_string(), trait_name.to_string()))
                .map(|app| TraitRef {
                    name: trait_name.to_string(),
                    args: app.trait_args.clone(),
                })
        };
        recorded.unwrap_or_else(|| TraitRef::bare(trait_name))
    }

    /// Checks each provided method's signature against the trait's declared
    /// one: parameter types, return type, and receiver. The trait's signature
    /// is read at this impl's application — its type parameters as `trait_ref`'s
    /// arguments, `Self` as the target type (a parameter for a blanket impl) —
    /// and `Self::Item` is projected through the impl's associated-type
    /// bindings on both sides. An `Unknown` on either side is left to the error
    /// that produced it, and a method whose own generics shadow the trait's is
    /// left to the trait's diagnostic.
    fn check_sig_conformance(
        &mut self,
        b: &ast::ImplBlock,
        target: &str,
        trait_ref: &TraitRef,
        t: &crate::env::TraitInfo,
        is_blanket: bool,
    ) {
        let trait_name = trait_ref.name.as_str();
        let impl_bindings: std::collections::HashMap<String, Ty> = b
            .assoc_bindings
            .iter()
            .map(|(name, ty)| (name.clone(), self.lower(ty)))
            .collect();
        let self_ty = if is_blanket {
            Ty::Param(target.to_string())
        } else {
            Ty::named(target)
        };
        for m in t.methods.iter().filter(|m| !shadows_trait_generic(m, t)) {
            let Some(info) = self
                .table
                .methods
                .get(&(target.to_string(), m.name.clone()))
            else {
                continue;
            };
            let project = |ty: Ty| crate::collect::project_assoc(ty, &impl_bindings);
            let read =
                |ty: &Ty| project(ty.subst_applied(&t.generics, &trait_ref.args, Some(&self_ty)));
            let want_params: Vec<Ty> = m.params.iter().map(read).collect();
            let want_ret = read(&m.ret);
            let got_params: Vec<Ty> = info.params.iter().map(|p| project(p.clone())).collect();
            let got_ret = project(info.ret.clone());
            let exact = Ty::same_or_unknown;
            let matches = info.has_receiver == m.has_receiver
                && got_params.len() == want_params.len()
                && got_params
                    .iter()
                    .zip(&want_params)
                    .all(|(a, b)| exact(a, b))
                && exact(&got_ret, &want_ret);
            if !matches {
                let want = Ty::Fn(want_params, Box::new(want_ret));
                let got = Ty::Fn(got_params, Box::new(got_ret));
                // Anchor on the offending impl method; point a label at the
                // trait's declaration so the editor links the two.
                let primary = self
                    .table
                    .member_defs
                    .get(&(target.to_string(), m.name.clone()))
                    .copied()
                    .unwrap_or((0, 0));
                let labels = self
                    .table
                    .member_defs
                    .get(&(trait_name.to_string(), m.name.clone()))
                    .map(|&(start, end)| {
                        vec![crate::Label {
                            message: format!(
                                "`{}` is declared here in trait `{trait_name}`",
                                m.name
                            ),
                            start,
                            end,
                        }]
                    })
                    .unwrap_or_default();
                self.error_labeled(
                    primary,
                    format!(
                        "method `{}` does not match trait `{trait_name}`: expected `{want}`, found `{got}`",
                        m.name
                    ),
                    labels,
                );
            }
        }
    }
}

/// Whether one of trait method `m`'s own type parameters reuses a name the
/// trait declares — rejected on the trait (`STATIC.md` §3.4).
fn shadows_trait_generic(m: &TraitMethod, t: &TraitInfo) -> bool {
    m.generics.iter().any(|g| t.generics.contains(g))
}

/// Whether the type parameter `param` occurs anywhere in `ty`.
fn mentions_param(ty: &Ty, param: &str) -> bool {
    match ty {
        Ty::Param(p) => p == param,
        Ty::List(inner) => mentions_param(inner, param),
        Ty::Fn(params, ret) => {
            params.iter().any(|p| mentions_param(p, param)) || mentions_param(ret, param)
        }
        Ty::Named { args, .. } => args.iter().any(|a| mentions_param(a, param)),
        _ => false,
    }
}
