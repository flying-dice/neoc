# Conformance corpus + the bugs it caught

- **Status:** done (other corpus layers tracked in `tasks/conformance-breadth.md`)
- **Area:** `lang/CONFORMANCE/`, `crates/neocc-compiler/tests/conformance.rs`,
  `crates/neocc-types`, `crates/neocc-codegen-lua`
- **Commits:** `4612cbe` (corpus), `343295d` (Result/Option match fix),
  `4d611d8` (exhaustiveness fix)

## What was done

- A behavioural dynamic conformance corpus: `.neoc` + expected `.stdout` pairs
  under `lang/CONFORMANCE/dynamic/`, run by a glob-based runner that transpiles,
  executes under `lua`, and asserts stdout (skips cleanly with no interpreter).
- The corpus immediately caught **two real bugs**, both fixed:
  1. **`match` over `Result`/`Option` fell through** — pattern lowering tested
     `_struct` against carriers tagged `_enum`/`tag`. Fixed with a `pattern_variant`
     resolver (prelude variants by bare name, user variants by `Enum::Variant`);
     `None` (a binding) is recognised as a unit-variant constant pattern.
  2. **Exhaustiveness false-positive** — `collect_covered` ignored
     `Pattern::Path`, so a qualified unit variant (`Shape::Dot`) was reported
     missing. Fixed; regression test added.

This is the payoff of the per-stage discipline: the corpus surfaced emitter/
checker bugs the earlier-stage tests didn't.
