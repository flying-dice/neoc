---
status: Superseded
date: 2026-05-29
---

# 0010. Independent codegen backends — no shared crate

Date: 2026-05-29

## Status

Superseded by [ADR 0012](/adrs/0012-drop-the-neoc-language) (NEOC-106): the codegen crates were removed with the neoc language. Preserved for history.

## Context

`neoc-codegen-lua` and `neoc-codegen-js` carry structurally-identical pieces:
the `EnumIndex` / `TupleStructIndex` use-site resolution tables, and the
pattern-lowering `Compiled` shape in each crate's `pat.rs`. The bodies differ
only in target syntax (`and` vs `&&`, `local` vs `let`, `[1]` vs `._0`).

The DRY pressure is to extract the shared logic into a common crate both
backends depend on. Two forces push back:

- **Crate isolation.** The workspace enforces per-crate boundaries in
  `Cargo.toml`, not convention. A `neoc-codegen-common` crate adds a new node to
  the graph that both backends must track; hoisting the pieces into `neoc-ast`
  widens the frozen AST contract's responsibility beyond pure data.
- **Backends diverge by design.** Lua and JS are different targets with
  different runtime models (per-value metatables vs a prototype registry). The
  shared-looking pieces are shared *today*; coupling them through a dependency
  bets that they stay identical, and pays an indirection cost at every site if
  they don't.

The duplication is small (two index structs and one pattern module) and the
backends are each well under a maintainable size.

## Decision

The two codegen backends are kept **independent**. The duplicated index and
pattern-lowering logic is accepted rather than extracted into a shared crate,
and is **not** hoisted into `neoc-ast`. Each backend owns its full lowering and
may diverge as its target requires. The intentional parallel is documented on
both `EnumIndex` structs, and the two are kept in sync by hand.

## Consequences

- **Easier:** each backend evolves freely per target with no shared dependency
  to coordinate; crate isolation is preserved; no premature abstraction over a
  similarity that may not hold.
- **Harder:** the index/pattern logic is duplicated — a bug fixed in one backend
  must be mirrored in the other, and the hand-sync expectation relies on
  discipline (and the documenting comments), not the compiler.
- **Revisit when:** the backends prove the shared pieces are stable across a few
  language-feature additions. If the duplication stops drifting, the cost/benefit
  of a shared crate flips and this can be superseded.
