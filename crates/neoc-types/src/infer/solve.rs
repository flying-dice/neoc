//! Trait solving — the queries the checker answers about traits: does a type
//! satisfy a trait, and what signature does a method resolve to through a
//! parameter's bounds, a trait-as-type receiver, a supertrait, or a blanket
//! impl. Kept distinct from expression inference per the type-system charter
//! (resolution ≠ inference). These are methods on [`Checker`]; this module is a
//! child of `infer`, so it shares the checker's fields and helpers.

use std::collections::{HashMap, HashSet};

use super::{Checker, ty_impl_name};
use crate::env::{BlanketImpl, ImplApplication, TraitRef, TypeDef};
use crate::ty::Ty;

impl Checker<'_> {
    /// A receiver method `method` declared on the applied trait `tr` or one of
    /// its supertraits, read at the arguments `tr` applies — and each
    /// supertrait at the arguments its declaration maps them to — with the
    /// trait's `Self` projected to `self_ty` (`STATIC.md` §3.4). `None` when no
    /// such receiver method exists — only receiver methods resolve through
    /// method syntax, an associated fn (no `self`) is not callable as `x.m()`.
    pub(super) fn resolve_trait_method(
        &self,
        tr: &TraitRef,
        method: &str,
        self_ty: &Ty,
    ) -> Option<(Vec<Ty>, Ty)> {
        // First match wins across the trait and its transitive supertraits, so
        // a `Greet: Show` bound resolves `show` declared on `Show`.
        self.trait_closure(tr).into_iter().find_map(|applied| {
            let Some(TypeDef::Trait(t)) = self.table().types.get(&applied.name) else {
                return None;
            };
            let m = t
                .methods
                .iter()
                .find(|m| m.name == method && m.has_receiver)?;
            let read = |ty: &Ty| ty.subst_applied(&t.generics, &applied.args, Some(self_ty));
            Some((m.params.iter().map(read).collect(), read(&m.ret)))
        })
    }

    /// The applied trait `tr` followed by every supertrait it reaches, each at
    /// the arguments its declaration maps `tr`'s arguments to (`trait Sub<T>:
    /// Base<T>` read at `Sub<number>` reaches `Base<number>`). Depth-first, in
    /// declaration order; cycle-guarded by trait name, so a malformed `trait A:
    /// B` / `trait B: A` pair terminates.
    pub(super) fn trait_closure(&self, tr: &TraitRef) -> Vec<TraitRef> {
        let mut out = Vec::new();
        let mut stack = vec![tr.clone()];
        let mut seen = HashSet::new();
        while let Some(applied) = stack.pop() {
            let applied = self.well_formed(applied);
            if !seen.insert(applied.name.clone()) {
                continue;
            }
            if let Some(TypeDef::Trait(t)) = self.table().types.get(&applied.name) {
                stack.extend(
                    t.supertraits
                        .iter()
                        .rev()
                        .map(|sup| sup.instantiate(&t.generics, &applied.args)),
                );
            }
            out.push(applied);
        }
        out
    }

    /// Whether the applied trait `from` is `to` or reaches it through its
    /// supertraits, arguments included — so `Sub<number>` reaches
    /// `Base<number>` but not `Base<string>`.
    pub(super) fn trait_reaches(&self, from: &TraitRef, to: &TraitRef) -> bool {
        self.trait_closure(from)
            .iter()
            .any(|applied| applied.name == to.name && args_agree(&applied.args, &to.args))
    }

    /// The application `ty` has of trait `name` at `arity` arguments: `ty`
    /// itself when it is that trait (or a subtrait reaching it), else one of
    /// its impls' (direct, blanket, or a subtrait's). Backs reading a generic
    /// trait's arguments at a call (`pull(c)` binds `pull<T>(s: Source<T>)`'s
    /// `T` from `impl Source<number> for C`).
    pub(super) fn application_of(&self, ty: &Ty, name: &str, arity: usize) -> Option<TraitRef> {
        let roots = match ty {
            Ty::Named { name: a, args } if self.is_trait(a) => vec![TraitRef {
                name: a.clone(),
                args: args.clone(),
            }],
            Ty::Unknown | Ty::Dynamic | Ty::Param(_) => return None,
            _ => self.trait_applications(ty),
        };
        roots
            .iter()
            .flat_map(|r| self.trait_closure(r))
            .find(|applied| applied.name == name && applied.args.len() == arity)
    }

    /// `tr` read at its trait's declared arity. An application written with
    /// the wrong number of arguments is already reported where it is spelled
    /// (`STATIC.md` §3.4); it reads as that trait at unknown arguments, so it
    /// neither admits nor rejects anything a second time.
    fn well_formed(&self, tr: TraitRef) -> TraitRef {
        match self.table().types.get(&tr.name) {
            Some(TypeDef::Trait(t)) if t.generics.len() != tr.args.len() => TraitRef {
                args: vec![Ty::Unknown; t.generics.len()],
                name: tr.name,
            },
            _ => tr,
        }
    }

    /// Whether `tr` names a trait applied to the number of arguments it
    /// declares. A name that is not a trait is left to its own diagnostic.
    pub(super) fn has_trait_arity(&self, tr: &TraitRef) -> bool {
        match self.table().types.get(&tr.name) {
            Some(TypeDef::Trait(t)) => t.generics.len() == tr.args.len(),
            _ => true,
        }
    }

    /// Every trait application concrete type `ty` has: its direct ones (see
    /// [`Self::direct_applications`]) plus one per blanket impl that applies
    /// to it.
    pub(super) fn trait_applications(&self, ty: &Ty) -> Vec<TraitRef> {
        let mut out = self.direct_applications(ty);
        out.extend(
            self.table()
                .blanket_impls
                .iter()
                .filter_map(|bl| self.blanket_application(bl, ty)),
        );
        out
    }

    /// The application blanket `bl` gives `ty` — its trait read with the target
    /// parameter as `ty` — when `ty` meets every one of its conditions.
    fn blanket_application(&self, bl: &BlanketImpl, ty: &Ty) -> Option<TraitRef> {
        bl.conditions
            .iter()
            .all(|c| self.type_satisfies(ty, c))
            .then(|| {
                self.well_formed(
                    bl.trait_ref
                        .instantiate(std::slice::from_ref(&bl.param), std::slice::from_ref(ty)),
                )
            })
    }

    /// The trait applications concrete type `ty` has through an `impl` or
    /// derive recorded for it. A generic trait's arguments are read by matching
    /// the impl's target against `ty` (`impl<U> Source<U> for Box<U>` at
    /// `Box<number>` applies `Source<number>`); an impl whose target `ty` does
    /// not match is omitted.
    pub(super) fn direct_applications(&self, ty: &Ty) -> Vec<TraitRef> {
        let Some(name) = ty_impl_name(ty) else {
            return Vec::new();
        };
        let traits = self.table().trait_impls.get(&name).into_iter().flatten();
        traits
            .filter_map(|trait_name| {
                let key = (name.clone(), trait_name.clone());
                let args = match self.table().trait_impl_applications.get(&key) {
                    Some(app) => impl_trait_args_for(app, ty)?,
                    None => Vec::new(),
                };
                Some(self.well_formed(TraitRef {
                    name: trait_name.clone(),
                    args,
                }))
            })
            .collect()
    }

    /// A method `method` provided by one of the trait bounds on type parameter
    /// `param`, with `Self` projected to the parameter itself. `None` when no
    /// bound supplies it. The cross-bound search is first-match in declaration
    /// order.
    pub(super) fn bound_method(&self, param: &str, method: &str) -> Option<(Vec<Ty>, Ty)> {
        let bounds = self.type_bounds.get(param)?;
        let self_param = Ty::Param(param.to_string());
        bounds.iter().find_map(|trait_name| {
            let tr = if param == "Self" {
                self.self_application(trait_name)
            } else {
                TraitRef::bare(trait_name)
            };
            self.resolve_trait_method(&tr, method, &self_param)
        })
    }

    /// The trait `trait_name` applied to its own type parameters — how `Self`
    /// is bounded inside one of its default bodies, where `Self` is a type
    /// parameter only (see `Checker::enter_trait_self`). So `self.next()` in a
    /// default of `trait Source<T>` returns the parameter `T`, not an unknown
    /// (`STATIC.md` §8).
    fn self_application(&self, trait_name: &str) -> TraitRef {
        match self.table().types.get(trait_name) {
            Some(TypeDef::Trait(t)) => TraitRef {
                name: trait_name.to_string(),
                args: t.generics.iter().cloned().map(Ty::Param).collect(),
            },
            _ => TraitRef::bare(trait_name),
        }
    }

    /// A method `method` on an applied trait used directly as a type (the
    /// existential), with the trait's `Self` projected to that trait type
    /// itself, arguments included — so `x.dup()` on `x: Source<number>` yields
    /// `Source<number>` (`STATIC.md` §3.4).
    pub(super) fn trait_method(&self, tr: &TraitRef, method: &str) -> Option<(Vec<Ty>, Ty)> {
        self.resolve_trait_method(tr, method, &tr.to_ty())
    }

    /// A default-bodied method `method` — a receiver method when `receiver`,
    /// else an associated function — that concrete type `recv` inherits from a
    /// trait it implements and did not override (`STATIC.md` §8). The
    /// signature is read at the impl's application: `Self` as `recv`, the
    /// trait's type parameters as the impl's arguments for `recv` (`impl
    /// Source<number> for C` reads a default's `T` as `number`), and
    /// `Self::Item` through the impl's bindings. Only the traits `recv`
    /// implements directly are searched — a supertrait's defaults reach the
    /// type through its own `impl` of that supertrait, which the conformance
    /// check requires. Two traits supplying the same default is rejected at
    /// the impl ([`Checker::default_sources`]); name order keeps the answer
    /// deterministic regardless.
    pub(super) fn default_method(
        &self,
        recv: &Ty,
        method: &str,
        receiver: bool,
    ) -> Option<(Vec<Ty>, Ty)> {
        let name = ty_impl_name(recv)?;
        let trait_name = self
            .direct_default_sources(&name, method)
            .into_iter()
            .next()?;
        let Some(TypeDef::Trait(t)) = self.table().types.get(&trait_name) else {
            return None;
        };
        let m = t
            .methods
            .iter()
            .find(|m| m.name == method && m.has_default && m.has_receiver == receiver)?;
        let applied = self
            .direct_applications(recv)
            .into_iter()
            .find(|tr| tr.name == trait_name)
            // A target the impl's header does not match (`Box::make()` against
            // `impl<U> Source<U> for Box<U>`) fixes no arguments.
            .unwrap_or_else(|| TraitRef {
                name: trait_name.clone(),
                args: vec![Ty::Unknown; t.generics.len()],
            });
        let empty = HashMap::new();
        let bindings = self
            .table()
            .impl_assoc
            .get(&(name, trait_name.clone()))
            .unwrap_or(&empty);
        let read = |ty: &Ty| {
            crate::collect::project_assoc(
                ty.subst_applied(&t.generics, &applied.args, Some(recv)),
                bindings,
            )
        };
        Some((m.params.iter().map(read).collect(), read(&m.ret)))
    }

    /// The traits concrete type `name` implements directly that declare a
    /// default-bodied `method`, in name order.
    fn direct_default_sources(&self, name: &str, method: &str) -> Vec<String> {
        let Some(impls) = self.table().trait_impls.get(name) else {
            return Vec::new();
        };
        let mut traits: Vec<String> = impls
            .iter()
            .filter(|trait_name| self.declares_default(trait_name, method))
            .cloned()
            .collect();
        traits.sort();
        traits
    }

    /// Every trait that gives concrete type `name` a default-bodied `method` —
    /// through a direct impl or a blanket impl whose conditions `name` meets —
    /// in name order, without repeats. Each lands on the type's one method
    /// table (`DYNAMIC.md` §3.4, §3.5).
    pub(super) fn default_sources(&self, name: &str, method: &str) -> Vec<String> {
        let ty = Ty::named(name);
        let mut traits = self.direct_default_sources(name, method);
        traits.extend(
            self.table()
                .blanket_impls
                .iter()
                .filter(|bl| self.declares_default(&bl.trait_ref.name, method))
                .filter(|bl| bl.conditions.iter().all(|c| self.type_satisfies(&ty, c)))
                .map(|bl| bl.trait_ref.name.clone()),
        );
        traits.sort();
        traits.dedup();
        traits
    }

    /// A trait other than `except` whose blanket impl, applying to concrete type
    /// `name`, provides `method` itself — first in name order.
    pub(super) fn blanket_method_source(
        &self,
        name: &str,
        method: &str,
        except: &str,
    ) -> Option<String> {
        let ty = Ty::named(name);
        let mut traits: Vec<&String> = self
            .table()
            .blanket_impls
            .iter()
            .filter(|bl| bl.trait_ref.name != except && bl.methods.iter().any(|m| m == method))
            .filter(|bl| bl.conditions.iter().all(|c| self.type_satisfies(&ty, c)))
            .map(|bl| &bl.trait_ref.name)
            .collect();
        traits.sort();
        traits.first().map(|t| (*t).clone())
    }

    /// Whether concrete type `name` inherits trait `trait_name`'s default
    /// `method`: the impl giving `name` the trait — direct or blanket — does
    /// not provide `method` itself.
    pub(super) fn inherits_default(&self, name: &str, trait_name: &str, method: &str) -> bool {
        if !self.declares_default(trait_name, method) {
            return false;
        }
        let direct = self
            .table()
            .trait_impls
            .get(name)
            .is_some_and(|impls| impls.contains(trait_name));
        if direct {
            return !self
                .table()
                .methods
                .contains_key(&(name.to_string(), method.to_string()));
        }
        let ty = Ty::named(name);
        self.table().blanket_impls.iter().any(|bl| {
            bl.trait_ref.name == trait_name
                && !bl.methods.iter().any(|m| m == method)
                && bl.conditions.iter().all(|c| self.type_satisfies(&ty, c))
        })
    }

    /// Whether trait `trait_name` declares a default-bodied `method`.
    fn declares_default(&self, trait_name: &str, method: &str) -> bool {
        matches!(self.table().types.get(trait_name), Some(TypeDef::Trait(t))
            if t.methods.iter().any(|m| m.name == method && m.has_default))
    }

    /// A method granted to concrete receiver `recv` by a blanket impl whose
    /// conditions it satisfies, resolved through the blanket's trait with the
    /// target parameter — and so `Self` — read as `recv` (`STATIC.md` §3.5).
    pub(super) fn blanket_method(&self, recv: &Ty, method: &str) -> Option<(Vec<Ty>, Ty)> {
        self.table().blanket_impls.iter().find_map(|bl| {
            let applied = self.blanket_application(bl, recv)?;
            let (params, ret) = self.resolve_trait_method(&applied, method, recv)?;
            // The blanket's bindings are written in terms of its target
            // parameter; read them for the concrete receiver, then project any
            // `Self::Item` in the resolved signature to the bound type.
            let bindings: HashMap<String, Ty> = bl
                .assoc_bindings
                .iter()
                .map(|(k, v)| {
                    let param = std::slice::from_ref(&bl.param);
                    (k.clone(), v.subst_params(param, std::slice::from_ref(recv)))
                })
                .collect();
            let project = |ty: Ty| crate::collect::project_assoc(ty, &bindings);
            Some((params.into_iter().map(project).collect(), project(ret)))
        })
    }

    /// The concrete types — every struct and enum in scope — that satisfy all
    /// of a blanket impl's `conditions`, in name order: the types the blanket
    /// lands on (`DYNAMIC.md` §3.5).
    pub(crate) fn blanket_targets(&self, conditions: &[String]) -> Vec<String> {
        let mut targets: Vec<String> = self
            .table()
            .types
            .iter()
            .filter(|(_, def)| matches!(def, TypeDef::Struct(_) | TypeDef::Enum(_)))
            .map(|(name, _)| name.clone())
            .filter(|name| {
                let ty = Ty::named(name);
                conditions.iter().all(|c| self.type_satisfies(&ty, c))
            })
            .collect();
        targets.sort();
        targets
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
            // Head-indexed: look up the type's own traits, then check each
            // reaches `trait_name` through the supertrait chain.
            _ => ty_impl_name(ty).is_some_and(|n| {
                self.table().trait_impls.get(&n).is_some_and(|traits| {
                    traits
                        .iter()
                        .any(|tr| self.trait_chain_includes(tr, trait_name))
                })
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
            self.trait_chain_includes(&bl.trait_ref.name, trait_name)
                && bl
                    .conditions
                    .iter()
                    .all(|c| self.type_satisfies_rec(ty, c, seen))
        })
    }

    /// Whether `trait_name` is `target` or reaches it through its transitive
    /// supertraits — so a `T: Greet` bound satisfies a `Show` requirement when
    /// `trait Greet: Show`. Cycle-guarded through [`Self::trait_closure`].
    pub(super) fn trait_chain_includes(&self, trait_name: &str, target: &str) -> bool {
        self.trait_closure(&TraitRef::bare(trait_name))
            .iter()
            .any(|t| t.name == target)
    }
}

/// Whether two trait argument lists name the same application under the
/// structural relation every other type comparison uses: equal shape, with an
/// unknown argument or a type parameter agreeing with anything — so a callee's
/// `Source<T>` admits `Source<number>`, its `T` fixed by inference.
fn args_agree(a: &[Ty], b: &[Ty]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.compatible(y))
}

/// The trait arguments a concrete impl applies for the value type `ty`: the
/// impl's generics are fixed by matching its target arguments against `ty`'s,
/// then read into its trait arguments. `None` when `ty` does not match the
/// target's shape.
fn impl_trait_args_for(app: &ImplApplication, ty: &Ty) -> Option<Vec<Ty>> {
    let actual: &[Ty] = match ty {
        Ty::Named { args, .. } => args,
        _ => &[],
    };
    if actual.len() != app.target_args.len() {
        return None;
    }
    let mut bound: Vec<Option<Ty>> = vec![None; app.generics.len()];
    for (pat, a) in app.target_args.iter().zip(actual) {
        if !match_ty(pat, a, &app.generics, &mut bound) {
            return None;
        }
    }
    let solved: Vec<Ty> = app
        .generics
        .iter()
        .zip(bound)
        .map(|(g, b)| b.unwrap_or_else(|| Ty::Param(g.clone())))
        .collect();
    Some(
        app.trait_args
            .iter()
            .map(|a| a.subst_params(&app.generics, &solved))
            .collect(),
    )
}

/// One-way match of an impl target pattern against a value type, binding each
/// of `generics` it meets to the corresponding part of `actual`. A generic met
/// twice must bind the same type both times.
fn match_ty(pat: &Ty, actual: &Ty, generics: &[String], bound: &mut [Option<Ty>]) -> bool {
    if let Ty::Param(p) = pat
        && let Some(i) = generics.iter().position(|g| g == p)
    {
        if let Some(prev) = &bound[i] {
            return prev == actual;
        }
        bound[i] = Some(actual.clone());
        return true;
    }
    match (pat, actual) {
        (Ty::List(p), Ty::List(a)) => match_ty(p, a, generics, bound),
        (Ty::Fn(pp, pr), Ty::Fn(ap, ar)) => {
            pp.len() == ap.len()
                && pp
                    .iter()
                    .zip(ap)
                    .all(|(p, a)| match_ty(p, a, generics, bound))
                && match_ty(pr, ar, generics, bound)
        }
        (Ty::Named { name: pn, args: pa }, Ty::Named { name: an, args: aa }) => {
            pn == an
                && pa.len() == aa.len()
                && pa
                    .iter()
                    .zip(aa)
                    .all(|(p, a)| match_ty(p, a, generics, bound))
        }
        _ => pat == actual || actual.is_unknown(),
    }
}
