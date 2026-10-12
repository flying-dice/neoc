//! Pattern semantics — binding the names a pattern introduces against the
//! scrutinee type, and `match` exhaustiveness/redundancy checking (`STATIC.md`
//! §4.2, §7). Kept distinct from expression inference per the type-system
//! charter (exhaustiveness ≠ inference). These are methods on [`Checker`]; this
//! module is a child of `infer`, so it shares the checker's fields and helpers.

use neoc_ast as ast;

use super::{Checker, usefulness};
use crate::env::{StructFields, TypeDef, VariantFields};
use crate::ty::Ty;

impl Checker<'_> {
    /// Checks `match` exhaustiveness and redundant arms (`STATIC.md` §7). The
    /// `span` is the whole `match` expression, used to anchor exhaustiveness
    /// diagnostics.
    pub(super) fn check_exhaustive(&mut self, scrut: &Ty, arms: &[ast::MatchArm], span: ast::Span) {
        // Redundant-arm check: anything after an unguarded catch-all is dead.
        let catch_all = arms
            .iter()
            .position(|a| a.guard.is_none() && self.is_catch_all(&a.pattern));
        if let Some(idx) = catch_all {
            if idx + 1 < arms.len() {
                // Anchor on the first dead arm; label the catch-all that shadows it.
                let dead = arms[idx + 1].body.span;
                let cover = arms[idx].body.span;
                self.error_labeled(
                    (dead.start, dead.end),
                    "this match arm is unreachable; an earlier arm already matches every value"
                        .to_string(),
                    vec![crate::Label {
                        message: "this arm already matches every value".to_string(),
                        start: cover.start,
                        end: cover.end,
                    }],
                );
            }
            return;
        }

        match scrut {
            Ty::Bool => {
                let covers_true = arms.iter().any(|a| pattern_matches_bool(&a.pattern, true));
                let covers_false = arms.iter().any(|a| pattern_matches_bool(&a.pattern, false));
                if !(covers_true && covers_false) {
                    self.error_at(
                        span,
                        "this match on `bool` is not exhaustive; cover both `true` and `false` or add a `_` arm"
                            .to_string(),
                    );
                }
            }
            Ty::Named { name, .. } => match self.table.types.get(name) {
                // Usefulness decomposes constructors recursively, so a payload
                // gap (`Some(true)` present, `Some(false)` and `None` covered)
                // is reported with a nested witness, not just a missing variant.
                Some(TypeDef::Enum(_)) => {
                    let missing = usefulness::missing_patterns(self.table, scrut, arms);
                    if !missing.is_empty() {
                        self.error_at(
                            span,
                            format!(
                                "this match on `{name}` is not exhaustive; missing variant(s): {}",
                                missing.join(", ")
                            ),
                        );
                    }
                }
                Some(TypeDef::Struct(_)) => {
                    // A struct has a single shape, so one unguarded irrefutable
                    // struct/tuple pattern covers every value — but a refutable
                    // sub-pattern on a field leaves a gap usefulness catches.
                    if !usefulness::missing_patterns(self.table, scrut, arms).is_empty() {
                        self.error_at(
                            span,
                            format!(
                                "this match on `{name}` is not exhaustive; bind its fields with a struct pattern or add a `_` arm"
                            ),
                        );
                    }
                }
                _ => self.error_at(
                    span,
                    format!("this match on `{name}` is not exhaustive; add a `_` arm"),
                ),
            },
            Ty::Number | Ty::Str => {
                self.error_at(
                    span,
                    format!(
                        "this match on `{scrut}` needs a binding or `_` arm; literal arms can never be exhaustive"
                    ),
                );
            }
            _ => {}
        }
    }

    /// `true` for a pattern that matches every value of the scrutinee: a `_`
    /// wildcard or a bare binding that does *not* resolve to a unit variant or
    /// unit struct (those are coverage of one case, not a catch-all).
    fn is_catch_all(&self, pattern: &ast::Pattern) -> bool {
        match pattern {
            ast::Pattern::Wildcard => true,
            ast::Pattern::Binding { name, .. } => self.resolve_unit_value(name).is_none(),
            ast::Pattern::Or(alts) => alts.iter().any(|a| self.is_catch_all(a)),
            _ => false,
        }
    }

    /// Binds the names a pattern introduces against the scrutinee type. Also
    /// validates structural patterns against the scrutinee's declaration.
    pub(crate) fn bind_pattern(
        &mut self,
        pattern: &ast::Pattern,
        ty: &Ty,
        kind: crate::resolve::BindingKind,
    ) {
        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::Path(_) => {}
            ast::Pattern::Binding { name, span } => {
                // An uppercase bare ident may be a unit variant/struct, not a
                // binding. Treat as a binding unless it resolves to one.
                if self.resolve_unit_value(name).is_some() {
                    return;
                }
                self.bind_local(name.clone(), ty.clone(), *span, kind);
            }
            ast::Pattern::Or(alts) => {
                for alt in alts {
                    self.bind_pattern(alt, ty, kind);
                }
            }
            ast::Pattern::Tuple { path, elems } => self.bind_tuple_pattern(path, elems, ty, kind),
            ast::Pattern::Struct { path, fields, .. } => {
                self.bind_struct_pattern(path, fields, ty, kind);
            }
        }
    }

    fn bind_tuple_pattern(
        &mut self,
        path: &[ast::Ident],
        elems: &[ast::Pattern],
        ty: &Ty,
        kind: crate::resolve::BindingKind,
    ) {
        // Element types come from the variant or newtype the path names.
        let elem_tys = self.tuple_pattern_field_tys(path, ty, elems.len());
        for (pat, ety) in elems.iter().zip(elem_tys) {
            self.bind_pattern(pat, &ety, kind);
        }
    }

    fn tuple_pattern_field_tys(&self, path: &[ast::Ident], ty: &Ty, arity: usize) -> Vec<Ty> {
        let unknowns = vec![Ty::Unknown; arity];
        let Some(ctor) = path.last() else {
            return unknowns;
        };
        // Newtype struct pattern `UserId(x)`.
        if let Some(TypeDef::Struct(s)) = self.table.types.get(ctor) {
            if let StructFields::Tuple(inner) = &s.fields {
                return vec![inner.clone()];
            }
        }
        // Enum tuple variant `Ok(v)`, `Shape::Move(a, b)`.
        let enum_name = if path.len() >= 2 {
            Some(path[path.len() - 2].clone())
        } else {
            self.owning_enum(ctor)
        };
        if let Some(en) = enum_name {
            if let Some(TypeDef::Enum(e)) = self.table.types.get(&en) {
                if let Some(v) = e.variants.iter().find(|v| v.name == *ctor) {
                    if let VariantFields::Tuple(tys) = &v.fields {
                        return Self::instantiate_variant_fields(ty, e, tys);
                    }
                }
            }
        }
        unknowns
    }

    /// Substitutes the enum's type arguments (from the scrutinee `ty`) into a
    /// variant's declared field types, so `Ok(v)` binds `v` at the scrutinee's
    /// `T` rather than the bare parameter.
    fn instantiate_variant_fields(ty: &Ty, e: &crate::env::EnumInfo, fields: &[Ty]) -> Vec<Ty> {
        let mut subst = std::collections::HashMap::new();
        if let Ty::Named { args, .. } = ty {
            for (g, a) in e.generics.iter().zip(args) {
                subst.insert(g.clone(), a.clone());
            }
        }
        fields
            .iter()
            .map(|f| Self::apply_subst(f, &subst))
            .collect()
    }

    fn bind_struct_pattern(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldPat],
        ty: &Ty,
        kind: crate::resolve::BindingKind,
    ) {
        let decl = self.struct_pattern_field_tys(path, ty);
        for fp in fields {
            let fty = decl
                .iter()
                .find(|(n, _)| *n == fp.name)
                .map_or(Ty::Unknown, |(_, t)| t.clone());
            if self.is_host_option_field(ty, &fty)
                && !matches!(fp.pattern, Some(ast::Pattern::Wildcard))
            {
                self.error_at(
                    fp.span,
                    format!(
                        "a pattern cannot read `Option` field `{0}` of host-implemented `{ty}`; \
                         read it with `.{0}`, which marshals it",
                        fp.name
                    ),
                );
            }
            match &fp.pattern {
                Some(p) => self.bind_pattern(p, &fty, kind),
                // Shorthand `Point { x }` binds the field name itself.
                None => self.bind_local(fp.name.clone(), fty, fp.span, kind),
            }
        }
    }

    fn struct_pattern_field_tys(&self, path: &[ast::Ident], ty: &Ty) -> Vec<(String, Ty)> {
        let Some(mut ctor) = path.last() else {
            return Vec::new();
        };
        // `Self { .. }` inside an impl is the impl's target struct.
        if let ([head], Some(target)) = (path, &self.impl_target) {
            if head == "Self" {
                ctor = target;
            }
        }
        // `Enum::Variant { .. }` names a variant even when a struct shares its name.
        let names_variant = path.len() >= 2
            && matches!(
                self.table.types.get(&path[path.len() - 2]),
                Some(TypeDef::Enum(_))
            );
        if !names_variant {
            if let Some(TypeDef::Struct(s)) = self.table.types.get(ctor) {
                if let StructFields::Named(fields) = &s.fields {
                    return fields.clone();
                }
            }
        }
        let enum_name = if path.len() >= 2 {
            Some(path[path.len() - 2].clone())
        } else {
            self.owning_enum(ctor)
        };
        if let Some(en) = enum_name {
            if let Some(TypeDef::Enum(e)) = self.table.types.get(&en) {
                if let Some(v) = e.variants.iter().find(|v| v.name == *ctor) {
                    if let VariantFields::Named(fields) = &v.fields {
                        let subst_fields: Vec<(String, Ty)> = fields
                            .iter()
                            .map(|(n, t)| {
                                let mut subst = std::collections::HashMap::new();
                                if let Ty::Named { args, .. } = ty {
                                    for (g, a) in e.generics.iter().zip(args) {
                                        subst.insert(g.clone(), a.clone());
                                    }
                                }
                                (n.clone(), Self::apply_subst(t, &subst))
                            })
                            .collect();
                        return subst_fields;
                    }
                }
            }
        }
        Vec::new()
    }

    /// Finds the enum that declares a variant of the given name.
    fn owning_enum(&self, variant: &str) -> Option<String> {
        self.table.types.iter().find_map(|(name, def)| match def {
            TypeDef::Enum(e) if e.variants.iter().any(|v| v.name == variant) => Some(name.clone()),
            _ => None,
        })
    }
}

fn pattern_matches_bool(pattern: &ast::Pattern, value: bool) -> bool {
    match pattern {
        ast::Pattern::Literal(ast::LiteralPat::Bool(b)) => *b == value,
        ast::Pattern::Or(alts) => alts.iter().any(|a| pattern_matches_bool(a, value)),
        _ => false,
    }
}
