//! Trait and impl well-formedness — the checks that an `impl` (and the trait
//! it implements) is complete and consistent: required methods present, no
//! extras, signatures conform, associated types bound, supertraits satisfied.
//! These emit the trait/impl diagnostics. A child module of `infer`, so it
//! shares the checker's fields and helpers; distinct from inference per the
//! type-system charter.

use neoc_ast as ast;

use super::{Checker, subst_self_param};
use crate::env::TypeDef;
use crate::ty::Ty;

impl Checker<'_> {
    pub(super) fn check_impl(&mut self, b: &ast::ImplBlock) {
        let Some(target) = (match &b.ty {
            ast::Type::Path(p) => p.segments.last().map(|s| s.name.clone()),
            _ => None,
        }) else {
            return;
        };
        // Trait-impl completeness (`STATIC.md` §8).
        if let Some(trait_path) = &b.trait_ {
            self.check_trait_impl(b, &target, trait_path);
        }
        // Bring the impl's own generic parameters and their bounds into scope,
        // so a generic impl's `T` (and, for a blanket impl, the target param's
        // bounds) is visible in every method body.
        let prev_params = std::mem::take(&mut self.type_params);
        let prev_bounds = std::mem::take(&mut self.type_bounds);
        self.type_params = b.generics.iter().map(|g| g.name.clone()).collect();
        self.type_bounds = crate::collect::generic_param_bounds(&b.generics);
        // Bring the impl's associated-type bindings into scope so `Self::Item`
        // in method signatures and bodies resolves to the bound type.
        let prev_assoc = std::mem::take(&mut self.assoc_bindings);
        self.assoc_bindings = b
            .assoc_bindings
            .iter()
            .map(|(name, ty)| (name.clone(), self.lower(ty)))
            .collect();
        for f in &b.items {
            self.check_fn(f, Some(&target));
        }
        self.assoc_bindings = prev_assoc;
        self.type_params = prev_params;
        self.type_bounds = prev_bounds;
    }

    fn check_trait_impl(&mut self, b: &ast::ImplBlock, target: &str, trait_path: &ast::TypePath) {
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
        // Supertrait completeness: an implementor must also implement every
        // supertrait the trait requires (`STATIC.md` §3.4).
        for sup in &t.supertraits {
            let covered = if is_blanket {
                crate::collect::generic_param_bounds(&b.generics)
                    .get(target)
                    .is_some_and(|conds| conds.iter().any(|c| self.trait_chain_includes(c, sup)))
            } else {
                self.table.implements(target, sup)
            };
            if !covered {
                self.error(format!(
                    "`{target}` implements `{trait_name}` but not its supertrait `{sup}`; add `impl {sup} for {target}`"
                ));
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
        self.check_sig_conformance(b, target, trait_name, t, is_blanket);
    }

    /// Checks each provided method's signature against the trait's declared one
    /// — parameter types, return type, and receiver — with `Self` read as the
    /// target type (a parameter for a blanket impl) and `Self::Item` projected
    /// through the impl's associated-type bindings on both sides. An `Unknown`
    /// on either side is left to the error that produced it.
    fn check_sig_conformance(
        &mut self,
        b: &ast::ImplBlock,
        target: &str,
        trait_name: &str,
        t: &crate::env::TraitInfo,
        is_blanket: bool,
    ) {
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
        for m in &t.methods {
            let Some(info) = self
                .table
                .methods
                .get(&(target.to_string(), m.name.clone()))
            else {
                continue;
            };
            let project = |ty: Ty| crate::collect::project_assoc(ty, &impl_bindings);
            let want_params: Vec<Ty> = m
                .params
                .iter()
                .map(|p| project(subst_self_param(p, &self_ty)))
                .collect();
            let want_ret = project(subst_self_param(&m.ret, &self_ty));
            let got_params: Vec<Ty> = info.params.iter().map(|p| project(p.clone())).collect();
            let got_ret = project(info.ret.clone());
            let exact = |a: &Ty, b: &Ty| a == b || a.is_unknown() || b.is_unknown();
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
                self.error(format!(
                    "method `{}` does not match trait `{trait_name}`: expected `{want}`, found `{got}`",
                    m.name
                ));
            }
        }
    }
}
