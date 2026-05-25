# Non-goals (intentional, for now)

- **Status:** deferred
- **Not a task** — a reference so contributors don't "fix" these as bugs.

These are deliberately not implemented. They are acceptable gaps, documented as
such. Revisit only if a real need shows up.

## Advanced Rust features deliberately omitted

- **Lifetimes, ownership, borrowing, `&`/`&mut`.** The whole premise — "Rust
  without lifetimes". Methods take `self` by value; Lua values are
  reference-shared. Never coming.
- **`async`/`await`, `Future`.** Out of scope; the runtime is synchronous from
  neoc's view.
- **Trait objects (`dyn Trait`).** Method dispatch is static / metatable-based;
  no dynamic dispatch object type.
- **Const generics, higher-ranked / higher-kinded types.** Not planned.

## Implemented but intentionally shallow (not bugs)

- **Generic trait-bound enforcement.** `<T: Trait>` parses and type params are
  tracked, but a bound *violation* isn't reported — type params are permissive.
  Full bound solving is deferred.
- **Derived `Eq` is shallow.** It compares fields with Lua `==`, so a nested
  derived type compares by reference, not recursively. Deep structural equality
  across nested derives is deferred (documented in `neocc-macros`).

If any of these graduate from "deferred" to "wanted", give it its own task file.
