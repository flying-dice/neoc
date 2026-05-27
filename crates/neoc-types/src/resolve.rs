//! Scope-aware name resolution: which binding each identifier *use* refers to.
//!
//! The type checker already walks the lexical scope structure (`Scopes`) to
//! type names; with recording enabled it also notes, per binding site, a
//! [`Binding`] (name + def-span + kind), and per single-segment identifier use,
//! the [`Resolved`] target it refers to — a local binding by [`BindingId`], or a
//! top-level item by name. A shadowing local and the name it shadows have
//! distinct ids, so a use after the shadow resolves to the local, not the item.
//!
//! This is what lets references / rename / goto operate on the *binding* rather
//! than the spelling (`tasks/scope-aware-resolution.md`). Built by
//! [`crate::resolution`]; the byte ranges are `(start, end)` pairs rather than
//! `Span`, because `Span`'s equality is position-insensitive.

use crate::env::BindingId;

/// What introduced a value binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    /// A function parameter.
    Param,
    /// The `self` receiver.
    SelfParam,
    /// A closure parameter.
    ClosureParam,
    /// A `let` binding.
    Let,
    /// A binding introduced by a `match` arm pattern.
    MatchArm,
    /// A binding introduced by a `for` loop pattern.
    ForPattern,
}

/// One value binding: its identity, name, the byte range of its defining
/// occurrence, and what introduced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: BindingId,
    pub name: String,
    /// The `[start, end)` byte range of the binding's name at its def site.
    pub def: (usize, usize),
    pub kind: BindingKind,
}

/// What an identifier use resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A local binding (param, `let`, closure param, pattern binding).
    Local(BindingId),
    /// A top-level item, by name (items are unique per module).
    Item(String),
}

/// What a member access `recv.member` resolves to: the owning type or trait,
/// the member name, and the byte range of the member's declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRef {
    /// The type or trait that declares the member.
    pub owner: String,
    /// The field or method name.
    pub member: String,
    /// The `[start, end)` byte range of the member's name at its def site.
    pub def: (usize, usize),
}

/// The scope-resolution of one module: every local binding it introduces, and
/// what each recorded identifier use resolves to.
#[derive(Debug, Default)]
pub struct Resolution {
    bindings: Vec<Binding>,
    uses: Vec<(usize, usize, Resolved)>,
    members: Vec<(usize, usize, MemberRef)>,
}

impl Resolution {
    pub(crate) fn note_binding(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }

    pub(crate) fn note_use(&mut self, start: usize, end: usize, target: Resolved) {
        self.uses.push((start, end, target));
    }

    pub(crate) fn note_member(&mut self, start: usize, end: usize, target: MemberRef) {
        self.members.push((start, end, target));
    }

    /// The member access a byte position falls on (a `recv.member` use site).
    /// `None` when the position is on no recorded member name.
    #[must_use]
    pub fn member_at(&self, pos: usize) -> Option<&MemberRef> {
        self.members
            .iter()
            .find(|(s, e, _)| pos >= *s && pos <= *e)
            .map(|(_, _, m)| m)
    }

    /// Every member use that resolves to the same `(owner, member)` — its
    /// def-site plus all uses, in source order, deduplicated. The basis for
    /// type-aware member references and rename.
    #[must_use]
    pub fn member_occurrences(&self, owner: &str, member: &str) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (s, e, m) in &self.members {
            if m.owner == owner && m.member == member {
                out.push((*s, *e));
                out.push(m.def);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The binding a byte position falls on — a use site, or a local binding's
    /// own def site. The end is inclusive, so a cursor touching either edge of
    /// the identifier resolves; identifier spans are always separated by a
    /// non-identifier character, so no two are adjacent. `None` when the
    /// position is on no resolved identifier.
    #[must_use]
    pub fn at(&self, pos: usize) -> Option<Resolved> {
        if let Some((_, _, target)) = self.uses.iter().find(|(s, e, _)| pos >= *s && pos <= *e) {
            return Some(target.clone());
        }
        self.bindings
            .iter()
            .find(|b| pos >= b.def.0 && pos <= b.def.1)
            .map(|b| Resolved::Local(b.id))
    }

    /// Every occurrence of a local binding — its def site plus all its uses, in
    /// source order, deduplicated.
    #[must_use]
    pub fn local_occurrences(&self, id: BindingId) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        if let Some(b) = self.binding(id) {
            out.push(b.def);
        }
        for (s, e, target) in &self.uses {
            if matches!(target, Resolved::Local(i) if *i == id) {
                out.push((*s, *e));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Every *use* that resolves to the top-level item `name` (not the item's
    /// own declaration, which this map does not record). Excludes any use that a
    /// local of the same name shadows.
    #[must_use]
    pub fn item_use_occurrences(&self, name: &str) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = self
            .uses
            .iter()
            .filter(|(_, _, target)| matches!(target, Resolved::Item(n) if n == name))
            .map(|(s, e, _)| (*s, *e))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Whether the local binding `id` is read anywhere — has at least one use.
    /// The basis for the unused-binding lints.
    #[must_use]
    pub fn is_used(&self, id: BindingId) -> bool {
        self.uses
            .iter()
            .any(|(_, _, target)| matches!(target, Resolved::Local(i) if *i == id))
    }

    /// The binding with the given id, if any.
    #[must_use]
    pub fn binding(&self, id: BindingId) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.id == id)
    }

    /// Every local binding the module introduces.
    #[must_use]
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
}
