# Emitter limitations: continue / let-destructure / ..base / enum methods

- **Status:** done
- **Area:** `crates/neocc-codegen-lua`
- **Commit:** `3c42e68`

## Goal

Finish four constructs that currently emit a "not yet implemented" diagnostic or
silently misbehave:

1. **`continue`** — Lua 5.1 has no `continue`/`goto`. Lower via the
   `repeat … until true` + outer-break-flag idiom so it coexists with `break`
   (only restructure loops that actually contain a `continue`).
2. **`let` destructuring** — irrefutable struct / newtype / tuple-struct patterns
   (`let Point { x, y } = p;`). Refutable enum-variant `let` stays a diagnostic.
3. **`..base` functional struct update** — runtime shallow-copy of `base`, then
   override the listed fields, constructed via the struct (so `_struct` + the
   metatable are correct).
4. **Enum methods** — give an enum with any `impl`/`#[derive]` a method table and
   `setmetatable` its variant constructions so `:method` dispatch resolves;
   remove the deriving-on-enums diagnostic.

## Acceptance

- [ ] Each lowers correctly; emit goldens + runnable conformance/e2e cases (a
      `for` with `continue`, one mixing `continue`+`break`, a `let` destructure,
      a `..base` update, an `impl`-method call on an enum).
- [ ] `lang/DYNAMIC.md` updated (esp. §3.2 enum table + metatable).
- [ ] `cargo test --workspace`, clippy, fmt all clean.

## Outcome

All four done. `continue` uses a `LoopFrame` stack + the `repeat … until true`
restructuring (with an outer-break flag so `break` still exits the outer loop;
nested loops keep their own `break`/`continue`). `let` destructuring binds via
`compile_pattern`. `..base` is a runtime shallow-copy IIFE through `.new`. Enums
with methods get a metatable table and `setmetatable`d variant construction.
Emit goldens + e2e + conformance cases added; `DYNAMIC.md` §3.2 rewritten.

**Spun off:** newtype *construction* `Wrapper(42)` doesn't route through `.new`
— see `tasks/newtype-construction.md`.
