# `match` on a single-variant struct flagged non-exhaustive

- **Status:** done
- **Commit:** `f795b00`
- **Area:** `crates/neocc-types`


## Problem

`match w { Wrapper(n) => … }` on a newtype / tuple struct (`struct Wrapper(number);`)
is wrongly reported as non-exhaustive and demands a `_` arm. A struct has a
single "shape" — one tuple/struct pattern over it is irrefutable and therefore
exhaustive, like a struct `let` destructure. The exhaustiveness check only
treats `enum`/`bool` scrutinees as coverable and otherwise demands a catch-all.

Found while adding the newtype conformance case (`tasks/done/13-newtype-construction.md`),
which worked around it with a `let` destructure.

## Fix

In `neocc-types` exhaustiveness (`check_exhaustive`): when the scrutinee is a
`struct` type, a single irrefutable struct/tuple/newtype pattern (all sub-patterns
irrefutable) covers it — don't require a `_` arm. Keep requiring a catch-all for
`number`/`string` and for partial enum coverage.

## Acceptance

- [ ] `match w { Wrapper(n) => n }` on a newtype is clean (no diagnostic).
- [ ] A struct `match` that binds all fields is exhaustive; tests for both the
      clean case and that genuinely-unbounded scrutinees still need `_`.
