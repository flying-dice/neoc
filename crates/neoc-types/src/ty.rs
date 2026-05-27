//! The neoc type representation used during checking.
//!
//! These are *not* the AST `Type` nodes. The AST carries syntax (paths,
//! segments, args); this module carries resolved, structural types that the
//! checker compares for equality and renders into diagnostics. Conversion from
//! an AST `Type` happens in [`crate::collect`] against the symbol table.

use std::fmt;

/// A resolved neoc type.
///
/// Equality on `Ty` is the type identity neoc uses: primitives match by kind,
/// nominal types (`struct`/`enum`/`ext type`) match by name and type
/// arguments, and `Unknown` matches anything so that one upstream error does
/// not cascade into a flood of downstream mismatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// `number`, the single numeric type.
    Number,
    /// `string`.
    Str,
    /// `bool`.
    Bool,
    /// The unit type `()`.
    Unit,
    /// A list `[T]`.
    List(Box<Ty>),
    /// A function type `fn(τ₁, …) -> τ`.
    Fn(Vec<Ty>, Box<Ty>),
    /// A nominal type: a `struct`, `enum`, `ext type`, or the built-in
    /// `Result`/`Option`. Carries any applied type arguments.
    Named { name: String, args: Vec<Ty> },
    /// An in-scope type parameter, e.g. `T`.
    Param(String),
    /// The placeholder used after a resolution or inference failure. Compares
    /// equal to every type so a single root error stays a single diagnostic.
    Unknown,
}

impl Ty {
    /// Constructs a nominal type with no type arguments.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Ty::Named {
            name: name.into(),
            args: Vec::new(),
        }
    }

    /// Constructs a `Result<T, E>`.
    #[must_use]
    pub fn result(ok: Ty, err: Ty) -> Self {
        Ty::Named {
            name: "Result".to_string(),
            args: vec![ok, err],
        }
    }

    /// Constructs an `Option<T>`.
    #[must_use]
    pub fn option(inner: Ty) -> Self {
        Ty::Named {
            name: "Option".to_string(),
            args: vec![inner],
        }
    }

    /// Returns `true` if this type is the error placeholder.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    /// Rebuilds the type, replacing any subterm for which `f` returns `Some`
    /// and recursing into list, function, and nominal-argument positions
    /// otherwise. The single structural-substitution primitive every
    /// `subst_*`/projection reuses, so the `List`/`Fn`/`Named` recursion lives
    /// in exactly one place rather than being re-written per substitution.
    #[must_use]
    pub(crate) fn map(&self, f: &impl Fn(&Ty) -> Option<Ty>) -> Ty {
        if let Some(replaced) = f(self) {
            return replaced;
        }
        match self {
            Ty::List(inner) => Ty::List(Box::new(inner.map(f))),
            Ty::Fn(params, ret) => Ty::Fn(
                params.iter().map(|p| p.map(f)).collect(),
                Box::new(ret.map(f)),
            ),
            Ty::Named { name, args } => Ty::Named {
                name: name.clone(),
                args: args.iter().map(|a| a.map(f)).collect(),
            },
            other => other.clone(),
        }
    }

    /// Replaces every `Self` with `target`, recursing through composites — how
    /// a trait or impl signature's `Self` is read as the implementing type.
    #[must_use]
    pub(crate) fn subst_self(&self, target: &Ty) -> Ty {
        self.map(&|t| match t {
            Ty::Named { name, args } if name == "Self" && args.is_empty() => Some(target.clone()),
            _ => None,
        })
    }

    /// Replaces every occurrence of the type parameter `param` with `with`,
    /// recursing through composites — how a blanket impl's binding, written in
    /// terms of its target parameter, is read for a concrete satisfying type.
    #[must_use]
    pub(crate) fn subst_param(&self, param: &str, with: &Ty) -> Ty {
        self.map(&|t| match t {
            Ty::Param(p) if p == param => Some(with.clone()),
            _ => None,
        })
    }

    /// Structural compatibility for the no-subtyping fragment.
    ///
    /// `Unknown` is compatible with anything (error recovery). A type
    /// parameter is compatible with anything (the checker does not yet solve
    /// bounds, so it treats parameters permissively). Otherwise compatibility
    /// is structural equality, recursing into lists, function types, and
    /// nominal type arguments.
    #[must_use]
    pub fn compatible(&self, other: &Ty) -> bool {
        self.relate(other, &|_, _| None)
    }

    /// The single structural relation between two types, parameterised by a
    /// per-node `hook`. At each node the hook is consulted first: `Some(result)`
    /// overrides the structural comparison there, `None` falls through to it.
    /// The structural comparison treats `Unknown` and `Param` as wildcards on
    /// either side and otherwise requires equal shape, recursing through lists,
    /// function types, and nominal type arguments — carrying the same hook down,
    /// so the override applies at every depth.
    ///
    /// [`Ty::compatible`] is this with no hook; the checker's `assignable`
    /// supplies a hook for the trait-as-type existential. One walker, so the two
    /// relations cannot drift apart.
    pub(crate) fn relate(&self, other: &Ty, hook: &impl Fn(&Ty, &Ty) -> Option<bool>) -> bool {
        if let Some(result) = hook(self, other) {
            return result;
        }
        match (self, other) {
            (Ty::Unknown | Ty::Param(_), _)
            | (_, Ty::Unknown | Ty::Param(_))
            | (Ty::Number, Ty::Number)
            | (Ty::Str, Ty::Str)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Unit, Ty::Unit) => true,
            (Ty::List(a), Ty::List(b)) => a.relate(b, hook),
            (Ty::Fn(pa, ra), Ty::Fn(pb, rb)) => {
                pa.len() == pb.len()
                    && pa.iter().zip(pb).all(|(x, y)| x.relate(y, hook))
                    && ra.relate(rb, hook)
            }
            (Ty::Named { name: na, args: aa }, Ty::Named { name: nb, args: ab }) => {
                na == nb
                    && aa.len() == ab.len()
                    && aa.iter().zip(ab).all(|(x, y)| x.relate(y, hook))
            }
            _ => false,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Number => f.write_str("number"),
            Ty::Str => f.write_str("string"),
            Ty::Bool => f.write_str("bool"),
            Ty::Unit => f.write_str("()"),
            Ty::List(inner) => write!(f, "[{inner}]"),
            Ty::Fn(params, ret) => {
                f.write_str("fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Ty::Named { name, args } => {
                f.write_str(name)?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Ty::Param(name) => f.write_str(name),
            Ty::Unknown => f.write_str("<unknown>"),
        }
    }
}
