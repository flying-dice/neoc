# Attribute well-formedness

- **Status:** done — checker-stage attribute well-formedness (derive/link/test/field + unknown-attr). Commits `44ee780` `acb1d6c`.
- **Area:** `crates/neocc-types`
- **Priority:** P2

## Goal

Finish the attribute well-formedness checks in `STATIC.md` §9 (currently partial):

- `#[derive(…)]` only on a `struct`/`enum`, names limited to the derivable traits
  (`Clone`, `Eq`, `Debug`, `Display`); `Eq` requires equatable fields; `Display`
  requires every field to satisfy `Display`.
- `#[test]` only on a zero-parameter `fn` returning `()` or `Result<(), E>`.
- Field-validation attributes only on a matching field type.

## Acceptance

- [x] Misapplied attributes produce a clear diagnostic; valid ones don't.
- [x] Tests for each rule (well-formed + ill-formed).

## Notes

- Diagnostics must stay in neoc terms (golden rule).
