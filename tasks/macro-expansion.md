# User-defined macros & macro expansion

- **Status:** plan — design-space framing (no design chosen yet); RFC-gated
- **Priority:** P2 — confirmed coming feature; `SYNTAX.md` §8 currently says
  "no general user-defined macro system in the first edition" — this lifts that
- **Depends on:** the parser/AST pipeline; informs the tooling architecture
- **Touches:** `lang/SYNTAX.md`, a new `lang/` expansion-semantics section,
  `lang/DYNAMIC.md`, the `neoc-parser` → `neoc-types` pipeline, `neoc-lsp`

## Goal

A user-defined macro system. `SYNTAX.md` §8's "fixed attribute set, no macros"
is a first-edition limit, not a design boundary. Macros generate neoc
constructs that then flow through the normal stages (parse → check → emit) — a
macro expands to *neoc*, never directly to target code.

## Where expansion sits in the pipeline

A new **expansion stage between parse and type-check**: `source → parse →
expand → check → elaborate → emit`. Expansion is a CST/AST→AST rewrite that
resolves macro invocations into the constructs they produce, then the existing
stages run unchanged on the expanded tree. Consequences:
- the type checker, codegen, and `#54` enforcement see only *expanded* neoc —
  no stage downstream of expansion needs to know macros exist;
- diagnostics and spans must carry **expansion provenance** (a span maps back to
  the invocation *and* the macro definition) so errors and goto land sensibly —
  this is the load-bearing hard part, same as rust-analyzer's.

## The design space (the RFC must choose)

1. **Declarative first** (`macro_rules`-style: match an input token/AST pattern,
   emit a template). Hygienic by construction, no arbitrary computation, easiest
   to make the tooling see through. Strong candidate for v1.
2. **Procedural later** (a macro is *code that runs at compile time* and returns
   a tree). More powerful, but needs to *execute* user code during compilation.
   - **neoc-specific opportunity:** neoc already embeds a runtime (`neoc-vm`,
     Luau). Procedural macros could **run on neoc's own VM** — self-hosting
     metaprogramming, Lisp-style — instead of Rust's separate proc-macro crates.
     Distinctive, and it reuses the sandbox we already have.
3. **Hygiene** — macro-introduced bindings must not capture or be captured by the
   call site. The scope-aware resolution model (binding identities) is the
   foundation hygiene builds on; decide the hygiene algorithm in the RFC.
4. **Invocation & definition syntax** — `name!(...)`? attribute macros
   (`#[derive]` generalised)? The four built-in derives (`Clone`/`Eq`/`Debug`/
   `Display`) become the first *user-expressible* case once the mechanism lands.

## Why this re-justifies the heavier tooling architecture

Macro expansion (and full traits) are exactly the features that make the
bespoke, name-based, recompute-per-request resolvers a dead end (you cannot
resolve a name *inside* or *around* a macro expansion by matching CST text). A
macro-aware IDE needs: expand → resolve over the expanded tree → map results back
to original spans. That is the semantic-model + provenance machinery rust-analyzer
has. So this feature is the strongest argument for investing in a real semantic
resolution layer + (eventually) incrementality, rather than per-feature
correlators. (See the architecture note in `cross-file-resolution.md`.)

## Phased plan (each its own RFC + MR, spec-first)

1. `[ ]` **RFC: the macro model** — declarative-first; invocation/definition
   grammar; hygiene algorithm; expansion-stage placement; span/provenance
   contract. `SYNTAX.md` + a new expansion-semantics section.
2. `[ ]` **Expansion stage in the pipeline** — `expand(module) -> module` with
   provenance-carrying spans; the existing stages run on the output unchanged.
   Re-express the built-in derives through it as the first real macros.
3. `[ ]` **Tooling through expansion** — goto/completion/references/hover resolve
   over the expanded tree and map back to source spans (the rust-analyzer
   problem, neoc-sized).
4. `[ ]` **Procedural macros (optional, later)** — run macro code on `neoc-vm`;
   sandboxing, determinism, and the compile-time/runtime boundary are the open
   risks.

## Acceptance (per slice)

- A declarative macro expands to neoc that then type-checks and lowers normally
  on every target; a `lang/CONFORMANCE` pair pins the expansion.
- Errors inside or around an expansion point at a sensible span (invocation or
  definition), never at synthesised offsets.
- Hygiene: a macro-introduced `let x` does not collide with a call-site `x`
  (conformance case).
- The built-in derives are re-expressed through the mechanism with no change to
  their emitted output (goldens stable).

## Open questions (for the RFC)

- Declarative-only v1, or design for procedural from the start?
- Token-level vs AST-level matching for declarative macros.
- Hygiene model (sets-of-scopes à la Racket, or simpler rename-based).
- For procedural-on-`neoc-vm`: determinism, sandbox limits, and how a macro's
  compile-time dependencies are resolved.
