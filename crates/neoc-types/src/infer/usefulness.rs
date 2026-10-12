//! Match exhaustiveness via Maranget's usefulness algorithm ("Warnings for
//! pattern matching", J. Funct. Programming 17(3), 2007). A pattern matrix is
//! *useful* for a row when that row matches some value no existing row does;
//! a `match` is exhaustive exactly when a bare wildcard row is *not* useful
//! against its arms. When it is useful, the algorithm reconstructs the missing
//! values as witness patterns, which become the diagnostic's "missing" list.
//!
//! This decomposes constructors recursively, so it catches nested gaps a
//! per-shape check misses — `match opt { Some(true) => .., None => .. }` is
//! reported non-exhaustive with witness `Some(false)`. Distinct from inference
//! per the type-system charter (exhaustiveness ≠ inference). `STATIC.md` §7.

use std::collections::HashMap;

use neoc_ast as ast;

use crate::env::{StructFields, SymbolTable, TypeDef, VariantFields};
use crate::ty::Ty;

/// A value constructor with a fixed arity, the head of a matched value.
#[derive(Clone, PartialEq)]
enum Ctor {
    Bool(bool),
    /// An enum variant, or a struct / newtype constructor, by name.
    Named(String),
    /// A literal from an open domain (number/string). Distinct literals never
    /// collide and the domain is never complete, so only a wildcard covers it.
    Lit(String),
}

/// A pattern normalized for the matrix: a wildcard (also a bare binding) or a
/// constructor applied to sub-patterns. Or-patterns are expanded into separate
/// rows during normalization, so they never appear here.
#[derive(Clone)]
enum Pat {
    Wild,
    Ctor(Ctor, Vec<Pat>),
}

/// Reports the missing patterns of a `match` on `scrut` whose (unguarded) arms
/// are `arms`. Empty means exhaustive. Each witness is rendered as neoc pattern
/// text (`Red`, `Some(false)`), in the scrutinee's constructor-declaration
/// order. Only unguarded arms are considered — a guarded arm cannot guarantee
/// coverage.
pub(super) fn missing_patterns(
    table: &SymbolTable,
    scrut: &Ty,
    arms: &[ast::MatchArm],
) -> Vec<String> {
    let engine = Engine { table };
    let matrix: Vec<Vec<Pat>> = arms
        .iter()
        .filter(|a| a.guard.is_none())
        .flat_map(|a| engine.normalize(&a.pattern, scrut))
        .map(|p| vec![p])
        .collect();
    engine
        .missing(&matrix, std::slice::from_ref(scrut))
        .iter()
        .map(|w| render(&w[0]))
        .collect()
}

struct Engine<'a> {
    table: &'a SymbolTable,
}

impl Engine<'_> {
    /// A constructor name read through a `type` alias (`STATIC.md` §3.8).
    fn ctor_name(&self, name: &str) -> String {
        self.table
            .alias_head(name)
            .unwrap_or_else(|| name.to_string())
    }

    /// Normalizes an AST pattern against its column type, expanding or-patterns
    /// into one normalized pattern per alternative.
    fn normalize(&self, pat: &ast::Pattern, ty: &Ty) -> Vec<Pat> {
        match pat {
            ast::Pattern::Wildcard => vec![Pat::Wild],
            ast::Pattern::Literal(ast::LiteralPat::Bool(b)) => {
                vec![Pat::Ctor(Ctor::Bool(*b), vec![])]
            }
            ast::Pattern::Literal(
                ast::LiteralPat::Int(s) | ast::LiteralPat::Float(s) | ast::LiteralPat::Str(s),
            ) => vec![Pat::Ctor(Ctor::Lit(s.clone()), vec![])],
            ast::Pattern::Binding { name, .. } => {
                if self.is_unit_ctor(name, ty) {
                    vec![Pat::Ctor(Ctor::Named(name.clone()), vec![])]
                } else {
                    vec![Pat::Wild]
                }
            }
            ast::Pattern::Path(path) => path.last().map_or_else(
                || vec![Pat::Wild],
                |n| vec![Pat::Ctor(Ctor::Named(n.clone()), vec![])],
            ),
            ast::Pattern::Tuple { path, elems } => {
                let Some(ctor) = path.last().map(|c| self.ctor_name(c)) else {
                    return vec![Pat::Wild];
                };
                let ctor = &ctor;
                let field_tys = self.field_tys(ctor, ty);
                let cols: Vec<Vec<Pat>> = elems
                    .iter()
                    .enumerate()
                    .map(|(i, e)| self.normalize(e, field_tys.get(i).unwrap_or(&Ty::Unknown)))
                    .collect();
                product(&cols)
                    .into_iter()
                    .map(|args| Pat::Ctor(Ctor::Named(ctor.clone()), args))
                    .collect()
            }
            ast::Pattern::Struct { path, fields, .. } => {
                let Some(ctor) = path.last().map(|c| self.ctor_name(c)) else {
                    return vec![Pat::Wild];
                };
                let ctor = &ctor;
                let decl = self.named_field_tys(ctor, ty);
                // One column per declared field, in declaration order: the
                // matching sub-pattern, or a wildcard for a shorthand binding,
                // an absent field (`..`), or an unknown declaration.
                let cols: Vec<Vec<Pat>> = decl
                    .iter()
                    .map(
                        |(fname, fty)| match fields.iter().find(|fp| fp.name == *fname) {
                            Some(fp) => match &fp.pattern {
                                Some(p) => self.normalize(p, fty),
                                None => vec![Pat::Wild],
                            },
                            None => vec![Pat::Wild],
                        },
                    )
                    .collect();
                product(&cols)
                    .into_iter()
                    .map(|args| Pat::Ctor(Ctor::Named(ctor.clone()), args))
                    .collect()
            }
            ast::Pattern::Or(alts) => alts.iter().flat_map(|a| self.normalize(a, ty)).collect(),
        }
    }

    /// The witnesses for which a wildcard row is useful against `matrix` over
    /// the column types `types` — i.e. the values `matrix` fails to cover. An
    /// empty result means `matrix` is exhaustive over `types`.
    fn missing(&self, matrix: &[Vec<Pat>], types: &[Ty]) -> Vec<Vec<Pat>> {
        let Some(col_ty) = types.first() else {
            // No columns left: a wildcard row is useful only when nothing above
            // it matches, i.e. the matrix has no rows.
            return if matrix.is_empty() {
                vec![vec![]]
            } else {
                vec![]
            };
        };
        let present = present_ctors(matrix);
        let sig = self.all_ctors(col_ty);
        let complete = sig
            .as_ref()
            .is_some_and(|all| all.iter().all(|c| present.contains(c)));

        if complete {
            // Every constructor appears: a value is uncovered only inside one of
            // them, so specialize by each and recurse into its fields.
            let mut out = Vec::new();
            for c in sig.unwrap_or_default() {
                let field_tys = self.ctor_field_tys(&c, col_ty);
                let arity = field_tys.len();
                let spec = specialize(matrix, &c, arity);
                let sub_types: Vec<Ty> = field_tys
                    .into_iter()
                    .chain(types[1..].iter().cloned())
                    .collect();
                for w in self.missing(&spec, &sub_types) {
                    let (head_args, tail) = w.split_at(arity);
                    let mut row = vec![Pat::Ctor(c.clone(), head_args.to_vec())];
                    row.extend(tail.iter().cloned());
                    out.push(row);
                }
            }
            out
        } else {
            // A constructor is missing (or the type is open): a wildcard row is
            // useful iff the default matrix is useful for the remaining columns.
            let tails = self.missing(&default_matrix(matrix), &types[1..]);
            if tails.is_empty() {
                return Vec::new();
            }
            let heads: Vec<Pat> = match sig {
                Some(all) => all
                    .into_iter()
                    .filter(|c| !present.contains(c))
                    .map(|c| {
                        let arity = self.ctor_field_tys(&c, col_ty).len();
                        Pat::Ctor(c, vec![Pat::Wild; arity])
                    })
                    .collect(),
                None => vec![Pat::Wild],
            };
            let mut out = Vec::new();
            for h in &heads {
                for tail in &tails {
                    let mut row = vec![h.clone()];
                    row.extend(tail.iter().cloned());
                    out.push(row);
                }
            }
            out
        }
    }

    /// The complete constructor signature of `ty`, or `None` when the type is
    /// open (a primitive literal domain, list, function, parameter, …) and only
    /// a wildcard can cover it.
    fn all_ctors(&self, ty: &Ty) -> Option<Vec<Ctor>> {
        match ty {
            Ty::Bool => Some(vec![Ctor::Bool(true), Ctor::Bool(false)]),
            Ty::Named { name, .. } => match self.table.types.get(name) {
                Some(TypeDef::Enum(e)) => Some(
                    e.variants
                        .iter()
                        .map(|v| Ctor::Named(v.name.clone()))
                        .collect(),
                ),
                Some(TypeDef::Struct(_)) => Some(vec![Ctor::Named(name.clone())]),
                _ => None,
            },
            _ => None,
        }
    }

    /// The column types produced by specializing `ty` against constructor `c`:
    /// the constructor's field types, with the scrutinee's type arguments
    /// substituted into the declaration. Empty for a nullary constructor.
    fn ctor_field_tys(&self, c: &Ctor, ty: &Ty) -> Vec<Ty> {
        match c {
            Ctor::Bool(_) | Ctor::Lit(_) => Vec::new(),
            Ctor::Named(name) => self.field_tys(name, ty),
        }
    }

    /// The positional field types of the variant or struct `ctor` heads, with
    /// the scrutinee's generic arguments substituted in. Named fields are
    /// returned in declaration order.
    fn field_tys(&self, ctor: &str, ty: &Ty) -> Vec<Ty> {
        self.named_field_tys(ctor, ty)
            .into_iter()
            .map(|(_, t)| t)
            .collect()
    }

    /// As [`Self::field_tys`], but carrying each field's name (the empty string
    /// for a positional/tuple field), so struct patterns can match by name.
    fn named_field_tys(&self, ctor: &str, ty: &Ty) -> Vec<(String, Ty)> {
        // A struct or newtype named directly by the constructor.
        if let Some(TypeDef::Struct(s)) = self.table.types.get(ctor) {
            return match &s.fields {
                StructFields::Unit => Vec::new(),
                StructFields::Tuple(inner) => vec![(String::new(), inner.clone())],
                StructFields::Named(fields) => fields.clone(),
            };
        }
        // An enum variant: locate the owning enum (from the scrutinee type, or
        // by searching) and substitute its type arguments into the fields.
        let enum_name = match ty {
            Ty::Named { name, .. }
                if matches!(self.table.types.get(name), Some(TypeDef::Enum(_))) =>
            {
                Some(name.clone())
            }
            _ => self.owning_enum(ctor),
        };
        let Some(en) = enum_name else {
            return Vec::new();
        };
        let Some(TypeDef::Enum(e)) = self.table.types.get(&en) else {
            return Vec::new();
        };
        let Some(v) = e.variants.iter().find(|v| v.name == ctor) else {
            return Vec::new();
        };
        let subst = type_arg_subst(&e.generics, ty);
        match &v.fields {
            VariantFields::Unit => Vec::new(),
            VariantFields::Tuple(tys) => tys
                .iter()
                .map(|t| (String::new(), apply_subst(t, &subst)))
                .collect(),
            VariantFields::Named(fields) => fields
                .iter()
                .map(|(n, t)| (n.clone(), apply_subst(t, &subst)))
                .collect(),
        }
    }

    /// Whether the bare name `name` is a unit constructor (not a binding): a
    /// unit variant of the scrutinee's enum, or a unit struct.
    fn is_unit_ctor(&self, name: &str, ty: &Ty) -> bool {
        if let Ty::Named { name: tn, .. } = ty {
            if let Some(TypeDef::Enum(e)) = self.table.types.get(tn) {
                if e.variants
                    .iter()
                    .any(|v| v.name == name && matches!(v.fields, VariantFields::Unit))
                {
                    return true;
                }
            }
        }
        matches!(self.table.types.get(name), Some(TypeDef::Struct(s)) if matches!(s.fields, StructFields::Unit))
    }

    /// The enum that declares a variant of the given name.
    fn owning_enum(&self, variant: &str) -> Option<String> {
        self.table.types.iter().find_map(|(name, def)| match def {
            TypeDef::Enum(e) if e.variants.iter().any(|v| v.name == variant) => Some(name.clone()),
            _ => None,
        })
    }
}

/// The distinct constructors heading column 0 of the matrix.
fn present_ctors(matrix: &[Vec<Pat>]) -> Vec<Ctor> {
    let mut out: Vec<Ctor> = Vec::new();
    for row in matrix {
        if let Some(Pat::Ctor(c, _)) = row.first() {
            if !out.contains(c) {
                out.push(c.clone());
            }
        }
    }
    out
}

/// Specializes a matrix by constructor `c` of the given arity: keeps the rows
/// whose head matches `c` (their sub-patterns replace the head column) and the
/// wildcard rows (expanded to `arity` wildcards), dropping rows headed by a
/// different constructor.
fn specialize(matrix: &[Vec<Pat>], c: &Ctor, arity: usize) -> Vec<Vec<Pat>> {
    let mut out = Vec::new();
    for row in matrix {
        let Some((head, rest)) = row.split_first() else {
            continue;
        };
        match head {
            Pat::Ctor(rc, args) if rc == c => {
                let mut new = args.clone();
                new.extend(rest.iter().cloned());
                out.push(new);
            }
            Pat::Wild => {
                let mut new = vec![Pat::Wild; arity];
                new.extend(rest.iter().cloned());
                out.push(new);
            }
            Pat::Ctor(..) => {}
        }
    }
    out
}

/// The default matrix: the rows headed by a wildcard, with that column dropped.
fn default_matrix(matrix: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    matrix
        .iter()
        .filter_map(|row| match row.first() {
            Some(Pat::Wild) => Some(row[1..].to_vec()),
            _ => None,
        })
        .collect()
}

/// The cartesian product of per-column pattern alternatives, used to expand a
/// constructor whose sub-patterns contain or-patterns into one row each.
fn product(cols: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    let mut acc: Vec<Vec<Pat>> = vec![Vec::new()];
    for col in cols {
        let mut next = Vec::new();
        for prefix in &acc {
            for choice in col {
                let mut row = prefix.clone();
                row.push(choice.clone());
                next.push(row);
            }
        }
        acc = next;
    }
    acc
}

/// Renders a witness pattern as neoc source text for the diagnostic. A nullary
/// constructor (unit variant/struct, `bool`/literal) renders as its bare name —
/// `None`, `Red`; a payload-bearing one shows its slots — `Err(_)`, `Some(false)`,
/// `P(false, false)` — so a partially-covered variant reads unambiguously
/// (`N(_)` when `N(0)` is matched but other numbers are not), the way Rust
/// reports an uncovered pattern.
fn render(pat: &Pat) -> String {
    match pat {
        Pat::Wild => "_".to_string(),
        Pat::Ctor(Ctor::Bool(b), _) => b.to_string(),
        Pat::Ctor(Ctor::Lit(s), _) => s.clone(),
        Pat::Ctor(Ctor::Named(n), args) if args.is_empty() => n.clone(),
        Pat::Ctor(Ctor::Named(n), args) => {
            let inner: Vec<String> = args.iter().map(render).collect();
            format!("{n}({})", inner.join(", "))
        }
    }
}

/// Maps an enum's generic parameters to the scrutinee's type arguments.
fn type_arg_subst(generics: &[String], ty: &Ty) -> HashMap<String, Ty> {
    let mut subst = HashMap::new();
    if let Ty::Named { args, .. } = ty {
        for (g, a) in generics.iter().zip(args) {
            subst.insert(g.clone(), a.clone());
        }
    }
    subst
}

/// Substitutes type parameters by name, recursing through compound types.
/// An unmapped parameter (one outside this enum's own generics) is left intact.
fn apply_subst(ty: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    ty.map(&|t| match t {
        Ty::Param(name) => Some(subst.get(name).cloned().unwrap_or_else(|| t.clone())),
        _ => None,
    })
}
