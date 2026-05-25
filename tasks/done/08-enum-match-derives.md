# Enum / match / patterns, `?`, method dispatch, derives

- **Status:** done
- **Area:** `crates/neocc-codegen-lua`, `crates/neocc-macros`
- **Commits:** `d85948e` (enum/match/?/methods + derives), `591df87` (derive wiring)

## What was done

- **Enum construction** — unit/tuple/struct variants to tagged tables
  (`{ _enum, tag, … }`), variants told apart from ordinary calls via an
  AST-built enum index seeded with the `Result`/`Option` prelude.
- **`match` + patterns** (`pat.rs`) — IIFE in value position; each pattern
  compiles to a test + binds (wildcard, binding, literal, unit/tuple/struct
  variant, or-patterns; guards nest inside the pattern test).
- **`?`** — IIFE short-circuit on `Err`/`None`, yields the wrapped value.
- **Method dispatch** — metatable + colon call (no type info needed).
- **Derives** (`neocc-macros`) — `Clone`/`Eq`/`Debug`/`Display`, structural over
  the AST, spliced onto the type table by the emitter. Derived `clone`
  re-attaches the metatable so the copy is a full value.
