# User-defined macros & macro expansion

- **Status:** PARKED — resumable plan. Design-space framed and the hard
  constraints found; **deferred behind the trait/type model** (`trait-type-model.md`).
  No code written. RFC-gated when picked up.
- **Priority:** P2 — confirmed coming feature ("user macros is absolutely a
  coming feature"). `SYNTAX.md` §8 currently says *"There is no general
  user-defined macro system in the first edition; the attribute set is fixed."* —
  this work lifts that.
- **Depends on:** the parser/AST pipeline; a Span **provenance** model (a real
  `neoc-ast` change — see below); ideally the scope-aware resolution map (for
  hygiene); the trait work (derives are the shared seam).
- **Touches:** the `tree-sitter-neoc` grammar (submodule), `neoc-ast` (new nodes
  **and** `Span`), `neoc-parser`, a new expansion stage in `neoc-compiler`,
  `neoc-types`, `neoc-lsp`; spec: `SYNTAX.md`, a new expansion-semantics section,
  `DYNAMIC.md`.

---

## Resuming cold — read this first

This is a handoff. Everything you need is in this file; you do not need prior
conversation. The state:

- **Decided:** declarative macros first; an expansion stage *between parse and
  check* (`parse → expand → check → elaborate → emit`), AST→AST, so every
  downstream stage sees only expanded neoc; **procedural-on-`neoc-vm` is
  architecturally blocked** (it would cycle the crate graph — see below) so it's
  out of scope for v1; hygiene rides the binding-identity resolution map; macros
  expand to *neoc*, never to target code.
- **Open (RFC decides):** declarative match level (token-tree vs AST), the
  hygiene algorithm, invocation/definition syntax, and the exact provenance
  representation.
- **Start here:** §"First steps when you resume" at the bottom.
- **Follow:** `checklist.md` (repo root) — this is a sweeping language feature and
  goes through every stage; and the `rfc` process if RFCs are in force again.
- **Related docs:** `trait-type-model.md` (derives seam), `checklist.md` (process),
  `cross-file-resolution.md` (the architecture note this reinforces), GitLab
  work-item **#54** (runtime type enforcement — interacts cleanly; see below).

---

## Goal

A user-defined macro system. A macro generates neoc constructs that then flow
through the normal stages unchanged. **A macro expands to neoc, never directly to
target code** (so the checker, codegen, and #54 enforcement all run on the
expanded tree — no stage downstream of expansion needs to know macros exist).

---

## Grounded codebase facts (verified this session — don't re-derive)

**The pipeline** (`crates/neoc-compiler/src/lib.rs`):
- `transpile(source, target)` (~line 125): `parser::parse(source)` →
  `types::check(&module)` → `types::elaborate(&module)` → `emit(target, …)`.
- `transpile_project(entry, target)`: a load loop (~line 263–307) parses each
  file, **scans `module.items` for `use` decls to discover the dependency graph**,
  and stores a `LoadedModule { module, … }`; then `check_and_emit_module`
  per file does `collect` → `resolve_use` → `import_referenced_types` →
  `check_with_table` → `elaborate_with_table` → `emit`.
- **Expansion insertion point** — `expand(module) -> module`, AST→AST:
  - single-file: between `parser::parse` and `types::check`.
  - multi-file: in the load loop, expand `parsed.module` **before** it is stored
    *and before the `use`-discovery scan* — because a macro may generate `use`
    decls that must be in the graph. This ordering is a real gotcha: expand
    first, then discover imports.

**`Span`** (`crates/neoc-ast/src/lib.rs`): `pub struct Span { pub start: usize,
pub end: usize }`, with a hand-written `impl PartialEq` that returns `true`
always (so AST-equality tests ignore spans). **It carries no origin** — fine for
single-source, useless for expanded code. `neoc-ast` is a **wasm-leaf** crate
(gated by `cargo check --target wasm32-unknown-unknown`), so a `Span` change
ripples to every compiler crate + the wasm gate.

**Derives today** (`crates/neoc-macros`): `pub fn derive(derive_name: &str,
target: DeriveTarget) -> Result<Vec<String>, String>` returns **Lua lines**, not
AST. Called at **codegen** time (`neoc-codegen-lua` `emit_derives`, ~line 536, and
the JS equivalent). So `#[derive(Eq)]` is a codegen-time string emit; the
checker never sees a derived method as a trait impl. `DeriveTarget` is
`Struct(&StructDecl)` / `Enum(&EnumDecl)`.

**Crate graph** (`CLAUDE.md` "Crate isolation"): compiler crates carry **no host
deps** (no `mlua`); `neoc-vm` bundles `mlua` and sits *above* the compiler
(`neoc-vm` embeds `neoc-compiler`). So a compiler-crate expansion stage **cannot**
depend on `neoc-vm` — that both violates isolation and cycles the graph.

**AST shape:** `Module { items: Vec<Item> }`; `Item`/`Stmt`/`Expr` carry spans;
binding/decl *name* sites historically don't (see `trait-type-model.md`).

**Conformance:** `lang/CONFORMANCE/{syntax,static,dynamic}`, driven by
`crates/neoc-compiler/tests/conformance_{syntax,static,dynamic}.rs`.

---

## The expansion stage

`expand(module: &Module) -> Module`, an AST→AST rewrite that resolves every
macro **invocation** into the constructs it produces and removes the invocation
node, so `check`/`elaborate`/`emit` never see a macro node. Macro **definitions**
are consumed by the expander (kept addressable for tooling/provenance).

Invariant (the desync rule from `checklist.md`): after `expand`, the tree
contains **no** macro nodes. A macro node reaching the checker or a codegen is a
desync bug.

---

## Span / provenance — the load-bearing piece

Expanded nodes need spans that map back to **both** the invocation site and the
macro definition, or diagnostics and goto land at synthesised offsets. This is
the hardest part (it's exactly rust-analyzer's macro problem, neoc-sized).

Options (RFC picks):
- a provenance field on `Span` (e.g. an optional `origin: ExpansionId`), or
- a side-table mapping expanded nodes → origin.

Either way it's an early sub-task, **not** an implementation afterthought,
because `neoc-ast` is a wasm leaf and the change ripples. The acceptance bar: an
error inside or around an expansion points at a sensible source span.

---

## Hygiene

Macro-introduced bindings must not capture, or be captured by, call-site names
(`let x` inside a macro must not collide with a caller's `x`). The foundation is
the **binding-identity resolution map** (`neoc-types/src/resolve.rs`, `BindingId`
— built for scope-aware resolution): hygiene is expressed in terms of binding
identities, not spellings. RFC chooses the algorithm: sets-of-scopes (Racket-style)
or simpler rename-based.

---

## Grammar + AST additions

- Grammar (submodule, per `checklist.md` §2): a **macro definition** form and a
  **macro invocation** form (`name!(...)`?, attribute-macro form generalising
  `#[derive]`?). Decide whether invocation args are a parsed expression list or
  an unparsed **token tree** (Rust uses token trees, which let macros take
  arbitrary syntax; that needs a delimited-token-tree CST node, not a parsed
  arg list). This is a real grammar design fork.
- AST: `MacroDef` and `MacroInvocation` nodes (invocation removed by `expand`).
- Token-level vs AST-level matching for declarative macros follows from the
  token-tree decision.

---

## Derives are the seam with the trait work

The four built-in derives (`Clone`/`Eq`/`Debug`/`Display`) **are trait impls**,
synthesised at codegen as Lua strings (above). Two consequences:

1. `trait-type-model.md` adds a **hardcoded bridge** so the trait checker treats
   the four derives as impls (needed so a derived-`Eq` type is usable as the `Eq`
   existential and tagged in `__traits`).
2. When macros land, the expansion stage turns `#[derive(..)]` into real `impl`
   blocks **in the AST** — and the trait checker's hardcoded bridge is **deleted**.
   So macros *subsume* the bridge; traits-first does not undermine macros, it pins
   the contract macros must satisfy (emit `impl` AST).
   - **Migration detail:** `neoc_macros::derive` returns `Vec<String>` (Lua), not
     AST. To re-express derives as macros, either make derive logic produce AST
     (synthetic `impl` blocks), or have the expander parse the derived source
     back into AST. RFC decides. Goldens must stay stable (no change to emitted
     output).

---

## Procedural macros — blocked as "run on neoc-vm"

A procedural macro is code that runs at compile time and returns a tree. The
tempting "run it on neoc's own VM (`neoc-vm`/Luau), Lisp-style" is
**architecturally impossible** as a drop-in: a compiler-crate expansion stage
can't depend on `neoc-vm`/`mlua` (crate isolation + graph cycle). If procedural
macros are ever wanted, they need an **out-of-process proc-macro server** the
compiler drives over IPC (staying pure) — its own large design. **v1 is
declarative-only.** Procedural is out of scope until that server is designed.

---

## #54 interaction (clean)

Work-item #54 (runtime type enforcement at drift points) sees only the
**expanded** AST, so macro-generated code is enforced exactly like
hand-written code — no special interaction. Good: nothing to coordinate beyond
"expand before check".

---

## Why this re-justifies the heavier tooling architecture

You cannot resolve a name *inside* or *around* a macro expansion by matching CST
text. A macro-aware IDE must: expand → resolve over the expanded tree → map
results back to source spans. That is the semantic-model + provenance machinery
rust-analyzer has, and it's the strongest argument for a real semantic resolution
layer over the bespoke, name-based resolvers (see `cross-file-resolution.md`'s
architecture note). Macros + full traits are *why* that investment is warranted.

---

## Phased plan (each its own MR, spec-first; RFC-gated)

1. `[ ]` **RFC: the macro model** — declarative-first; token-tree vs AST matching;
   invocation/definition grammar; hygiene algorithm; expansion-stage placement;
   the span/provenance contract. `SYNTAX.md` + a new expansion-semantics section.
2. `[ ]` **Span provenance in `neoc-ast`** — the load-bearing change; do it early,
   mind the wasm-leaf ripple.
3. `[ ]` **Grammar + AST + parser** — macro def + invocation nodes (submodule
   regen per `checklist.md` §2); `walk_*` in `neoc-parser`.
4. `[ ]` **Expansion stage** — `expand(module) -> module` in `neoc-compiler`,
   inserted before `use`-discovery / check (see insertion point above);
   provenance carried; downstream stages unchanged.
5. `[ ]` **Re-express the built-in derives** through the mechanism; delete the
   trait checker's derive-bridge; goldens stable.
6. `[ ]` **Tooling through expansion** — goto/completion/references/hover resolve
   over the expanded tree, map back to source spans.
7. `[ ]` **Procedural macros (later, optional)** — out-of-process server; sandbox,
   determinism, the compile-time/runtime boundary are the open risks.

---

## Acceptance (per slice)

- A declarative macro expands to neoc that type-checks and lowers normally on
  every target; a `lang/CONFORMANCE` pair pins the expansion.
- Errors inside or around an expansion point at a sensible span (invocation or
  definition), never at synthesised offsets.
- Hygiene: a macro-introduced `let x` does not collide with a call-site `x`
  (conformance case).
- The built-in derives are re-expressed through the mechanism with **no change**
  to their emitted output (goldens stable), and the trait derive-bridge is gone.
- No macro node survives into `check`/codegen (desync check).

---

## Open questions (for the RFC)

- Declarative-only v1 (assumed), or design the procedural server boundary now?
- Token-tree vs AST-level matching for declarative macros.
- Hygiene model: sets-of-scopes vs rename-based.
- Provenance representation: `Span` field vs side-table.
- Derive re-expression: macro-produces-AST vs parse-derived-source-back.
- Invocation/definition surface syntax (`name!(...)`, attribute macros).

---

## First steps when you resume

1. Re-read this file + `checklist.md` + the "Grounded codebase facts" above
   (re-verify the line numbers; the code will have moved).
2. Confirm the trait work landed the derive-bridge — that's the contract macros
   must satisfy and the thing they'll delete.
3. Write the RFC (phase 1): pin declarative-first, token-tree-vs-AST, hygiene,
   and the provenance contract. Get it gated.
4. Land provenance (phase 2) before the expansion stage — it's the ripple that's
   painful to retrofit.
