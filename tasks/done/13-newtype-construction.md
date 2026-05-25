# Newtype construction doesn't route through `.new`

- **Status:** done
- **Area:** `crates/neocc-codegen-lua`
- **Commit:** `f3b0d99`

## Outcome

`Wrapper(42)` now lowers to `Wrapper.new(42)` (a `TupleStructIndex` tells a
tuple-struct call from an ordinary one), so the value carries its `_struct` tag
and metatable. Also fixed a related mismatch: newtype field access `.0` lowered
to `.value` but the payload is at `[1]` — now reads `[n+1]`, agreeing with the
constructor and the derive/pattern lowering. Emit golden + conformance case +
an end-to-end `.0` test.

Surfaced a separate type-checker quirk → `tasks/match-exhaustiveness-struct.md`.

---

(original below)

## Problem

A newtype value constructed as `Wrapper(42)` lowers to a bare Lua call
`Wrapper(42)` instead of `Wrapper.new(42)`. So the value misses the `_struct`
tag and the metatable — `match`/methods/derives on it would not work. Newtype
*destructuring* (`let Wrapper(inner) = w`) and named-struct construction are
fine; only newtype *construction* is wrong.

Found while finishing the emitter limitations (`tasks/done/11-emitter-limitations.md`).

## Fix

In the emitter, a call whose callee resolves to a newtype struct should lower to
`N.new(arg)` (the constructor `emit_struct` generates for tuple structs), not a
bare call. Distinguish it from an ordinary function call via the declaration
info (the struct is a known tuple struct).

## Acceptance

- [ ] `Wrapper(42)` lowers to `Wrapper.new(42)`; the value carries `_struct` +
      metatable.
- [ ] An end-to-end test: construct a newtype, call a method / `match` on it.
- [ ] Existing tests stay green.
