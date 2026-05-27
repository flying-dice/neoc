//! Trait solving — the queries the checker answers about traits: does a type
//! satisfy a trait, and what signature does a method resolve to through a
//! parameter's bounds, a trait-as-type receiver, a supertrait, or a blanket
//! impl. Kept distinct from expression inference per the type-system charter
//! (resolution ≠ inference). These are methods on [`Checker`]; this module is a
//! child of `infer`, so it shares the checker's fields and helpers.

use std::collections::HashSet;

use super::{Checker, subst_self_param, ty_impl_name};
use crate::env::TypeDef;
use crate::ty::Ty;

impl Checker<'_> {
    /// A receiver method `method` declared on the named trait, with the
    /// trait's `Self` projected to `self_ty`. `None` when the trait has no such
    /// receiver method — only receiver methods resolve through method syntax,
    /// an associated fn (no `self`) is not callable as `x.m()`.
    pub(super) fn resolve_trait_method(
        &self,
        trait_name: &str,
        method: &str,
        self_ty: &Ty,
    ) -> Option<(Vec<Ty>, Ty)> {
        // Search the trait and its transitive supertraits, first match wins, so
        // a `Greet: Show` bound resolves `show` declared on `Show`.
        let mut stack = vec![trait_name.to_string()];
        let mut seen = HashSet::new();
        while let Some(tn) = stack.pop() {
            if !seen.insert(tn.clone()) {
                continue;
            }
            let Some(TypeDef::Trait(t)) = self.table().types.get(&tn) else {
                continue;
            };
            if let Some(m) = t
                .methods
                .iter()
                .find(|m| m.name == method && m.has_receiver)
            {
                let proj = |ty: &Ty| subst_self_param(ty, self_ty);
                return Some((m.params.iter().map(proj).collect(), proj(&m.ret)));
            }
            stack.extend(t.supertraits.iter().cloned());
        }
        None
    }

    /// A method `method` provided by one of the trait bounds on type parameter
    /// `param`, with `Self` projected to the parameter itself. `None` when no
    /// bound supplies it. The cross-bound search is first-match in declaration
    /// order.
    pub(super) fn bound_method(&self, param: &str, method: &str) -> Option<(Vec<Ty>, Ty)> {
        let bounds = self.type_bounds.get(param)?;
        let self_param = Ty::Param(param.to_string());
        bounds
            .iter()
            .find_map(|trait_name| self.resolve_trait_method(trait_name, method, &self_param))
    }

    /// A method `method` on a trait used directly as a type (the existential),
    /// with the trait's `Self` projected to the trait type itself, so
    /// `x.clone()` on `x: Drawable` yields `Drawable` (`STATIC.md` §3.4).
    pub(super) fn trait_method(&self, trait_name: &str, method: &str) -> Option<(Vec<Ty>, Ty)> {
        self.resolve_trait_method(trait_name, method, &Ty::named(trait_name))
    }

    /// A method granted to concrete type `name` (of type `recv`) by a blanket
    /// impl whose conditions `recv` satisfies, resolved through the blanket's
    /// trait with `Self` read as `name` (`STATIC.md` §3.5).
    pub(super) fn blanket_method(
        &self,
        name: &str,
        recv: &Ty,
        method: &str,
    ) -> Option<(Vec<Ty>, Ty)> {
        self.table().blanket_impls.iter().find_map(|bl| {
            if bl.conditions.iter().all(|c| self.type_satisfies(recv, c)) {
                self.resolve_trait_method(&bl.trait_name, method, &Ty::named(name))
            } else {
                None
            }
        })
    }

    /// Whether `ty` satisfies the bound `trait_name`. A concrete type satisfies
    /// it when an `impl` or derive records the pair; a type parameter satisfies
    /// it when its own in-scope bounds include the trait (bound propagation); a
    /// blanket impl whose conditions `ty` meets contributes its trait; an
    /// unknown type satisfies it vacuously, so an earlier error does not cascade.
    pub(crate) fn type_satisfies(&self, ty: &Ty, trait_name: &str) -> bool {
        self.type_satisfies_rec(ty, trait_name, &mut HashSet::new())
    }

    fn type_satisfies_rec(
        &self,
        ty: &Ty,
        trait_name: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        let direct = match ty {
            Ty::Unknown => return true,
            Ty::Param(p) => self
                .type_bounds
                .get(p)
                .is_some_and(|bs| bs.iter().any(|b| self.trait_chain_includes(b, trait_name))),
            _ => ty_impl_name(ty).is_some_and(|n| {
                self.table()
                    .trait_impls
                    .iter()
                    .any(|(t, tr)| *t == n && self.trait_chain_includes(tr, trait_name))
            }),
        };
        direct || self.satisfies_via_blanket(ty, trait_name, seen)
    }

    /// Whether a blanket impl of `trait_name` (or a subtrait of it) applies to
    /// `ty` — i.e. `ty` satisfies every one of the blanket's conditions. The
    /// condition checks recurse; `seen` guards against a blanket cycle
    /// (`impl<T: A> B for T` + `impl<T: B> A for T`).
    fn satisfies_via_blanket(
        &self,
        ty: &Ty,
        trait_name: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        let key_name = match ty {
            Ty::Param(p) => p.clone(),
            _ => match ty_impl_name(ty) {
                Some(n) => n,
                None => return false,
            },
        };
        if !seen.insert((key_name, trait_name.to_string())) {
            return false;
        }
        self.table().blanket_impls.iter().any(|bl| {
            self.trait_chain_includes(&bl.trait_name, trait_name)
                && bl
                    .conditions
                    .iter()
                    .all(|c| self.type_satisfies_rec(ty, c, seen))
        })
    }

    /// Whether `trait_name` is `target` or reaches it through its transitive
    /// supertraits — so a `T: Greet` bound satisfies a `Show` requirement when
    /// `trait Greet: Show`. Cycle-guarded: a malformed `trait A: B` / `trait B:
    /// A` pair terminates instead of looping.
    pub(super) fn trait_chain_includes(&self, trait_name: &str, target: &str) -> bool {
        let mut stack = vec![trait_name.to_string()];
        let mut seen = HashSet::new();
        while let Some(t) = stack.pop() {
            if t == target {
                return true;
            }
            if !seen.insert(t.clone()) {
                continue;
            }
            if let Some(TypeDef::Trait(info)) = self.table().types.get(&t) {
                stack.extend(info.supertraits.iter().cloned());
            }
        }
        false
    }
}
