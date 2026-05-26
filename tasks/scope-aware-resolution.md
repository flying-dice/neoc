# Scope-aware name resolution (cross-file Phase 5)

- **Status:** in progress — slices 1–4 landed (binding spans, resolution map,
  scope-aware references/rename/goto, `unused_let`). `unused_param` and the
  `shadowing` lint remain, gated on the RFC below.
- **Priority:** P3 — the largest single tooling item left
- **Depends on:** `cross-file-resolution` Phases 2–4 (landed); the `WorkspaceIndex`
- **Gate:** the two remaining lints change language-level static semantics →
  RFC/design per `CLAUDE.md`. The resolution machinery (slices 1–3) is tooling
  and is opt-in, so it carries no semantic change and landed without the RFC.

## The gap

Every name-sensitive LSP feature resolves **by name, file-locally**, with no
scope analysis. `references.rs`, `rename.rs`, `highlight.rs`, `goto.rs`, and the
cross-file `project_references`/`project_rename` all sit on
`cst::identifier_nodes(tree, source, name)` — *every* identifier node spelled
`name`. The modules say so themselves: references.rs — "a local that shadows a
top-level name is reported alongside it". Consequences:

```neoc
fn f(x: number) -> number {     // param `x`
  let y = { let x = 1; x };     // a different `x`, in an inner block
  x + y                          // the param `x`
}
```

Rename the param `x` → `n` and the inner-block `x` is renamed too (wrong). Find
references on either `x` returns all three occurrences (two distinct bindings
conflated). The cross-file `reference_scope` bounds the *file set* correctly,
but **within** a file the match is still text-level, so the same conflation
applies project-wide. This blocks the scope-sensitive lints (`unused let`,
`unused param`, `shadowing`) that need to know *which* binding an identifier is.

## What the major servers do (and what we take)

rust-analyzer resolves a name to a `Definition` (a binding identity), then
find-usages/rename operate on the *definition*, not the spelling — locals via
intra-body scope, items via the module tree. tsserver's `FindAllReferences`
likewise resolves a `Symbol` first. The shared lesson: **resolve the identifier
to a binding once; every name feature then operates on the binding.**

neoc already has the scope machinery for *checking* — `neoc-types`'s `Scopes`
(`Vec<HashMap<String, Ty>>`, push/pop/bind/lookup) walks the same lexical
structure during inference. It is thrown away after the walk. The plan is to
**record** the resolution it already computes, the same way `record_types` /
`expression_types` records `(span, Ty)` — not to build a second resolver.

## Where it lives (golden rule)

One resolver, shared — like `module_file` and `expression_types`. Resolution is
static semantics, so it belongs in `neoc-types`, beside the checker that already
tracks scope. `neoc-lsp` consumes a resolution map; it must not grow a parallel
CST-based scope walker, or goto/rename will disagree with what the checker sees.

## Design

### A binding identity + a resolution map (`neoc-types`)

Give every binding site (fn param, `let` pattern, closure param, match-arm
pattern, `for` pattern) an identity, and record, per identifier *use*, the
binding it resolves to:

```
BindingId        an opaque per-module id minted at each binding site
Binding          { id, name, def_span, kind: Param|Let|ClosureParam|MatchBind|ForBind }
ResolutionMap    binding: BindingId -> Binding
                 uses:    BindingId -> [Span]      every use that resolves here
                 at:      Span      -> BindingId    the binding a use-span refers to
```

Built behind a flag on the `Checker` (mirror `record_types`): extend each scope
frame from `name -> Ty` to `name -> (Ty, BindingId)`; on `bind`, mint an id and
record the `Binding`; record `use_span -> BindingId` **only** at the
single-segment value-resolution in `infer_path` (`infer.rs:399`). The other
`scopes.lookup` (`infer.rs:840`) is a struct-field-init type-inference fallback,
**not** an identifier use — it must not be recorded, or a local sharing a field
name gets a phantom use.

**Resolve *every* use, not just locals.** Top-level items get a `BindingId`
too — an `Item` binding minted from its declaration. `infer_path` already
resolves a single-segment name against the scopes first and the module table
second (`infer.rs:397–411`); record which one won. So a use that resolves to a
shadowing local maps to the local's id, and a use that resolves to the top-level
item maps to the item's id — *distinctly*. This is what lets a rename of the
**top-level** name skip a shadowing local (and vice-versa); the locals-only
split below would not (see "Why locals-only is not enough"). Cross-file, an
imported item's uses in an importer resolve to the item's id; the
`WorkspaceIndex` ties the importer's id-space to the defining file.

Expose `pub fn resolution(module) -> ResolutionMap` alongside
`expression_types`. Minting ids changes the scope-frame shape always-on (a small
per-binding cost); only the *map* accumulation is behind the flag, so plain
`check` pays a constant, not the recording.

### Why locals-only is not enough

A first cut might record only locals and leave top-level/imported names on the
name-based path. That fixes renaming a *local* but **not** renaming a top-level
or imported name a local shadows:

```neoc
fn helper() -> number { 1 }     // top-level
fn main() -> number {
  let helper = 2;               // shadows the top-level name
  helper                        // resolves to the local
}
```

Rename the **top-level** `helper`: a locals-only design falls back to
`references_by_name("helper")`, which is scope-blind and rewrites the local and
its use too — the same over-reach we set out to remove, now in the other
direction (and identically in an importer that shadows an imported name). So the
map must resolve items as well, and every feature operates on the resolved
`BindingId`, never the spelling.

### Prerequisite: binding-site spans in the AST

**The AST does not carry binding-site spans.** `Param::Named { name: Ident, ty }`,
`Pattern::Binding(Ident)`, `ClosureParam`, and `FieldPat` hold a bare `String`
(`Ident`); only `Expr`, `Stmt`, and `Item` carry `span`. A resolution map keyed
on `def_span` cannot be built until binding sites record their span. Sub-task 1
adds them, threaded parser → AST.

Cost is **mechanical, not risky**: ~35 sites construct or match these (parser 7,
types 10, codegen-lua 9, codegen-js 7; fmt/macros/lsp 0) — the 4 parser
constructions thread `span`, the rest add `..` to a match. Crucially, `Span`'s
`PartialEq` returns `true` unconditionally (`neoc-ast/src/lib.rs:29`), by design
("nodes … gain a span without disturbing those tests"), so **no AST-equality
golden changes** — the earlier worry about PartialEq-sensitive goldens does not
apply. `self` (`Param::Receiver`), for-loop, and struct-shorthand binds ride the
same `Pattern`/`Param` change for free; generic type params are out of scope (v1
is value bindings).

### LSP consumption (`neoc-lsp`)

References / rename / highlight, and the per-file step of
`project_references`/`project_rename`, **resolve the cursor to a `BindingId`**
(local *or* item) via the map, then operate on that binding's `uses` — never on
the spelling. A shadowing local and the name it shadows have different ids, so
each renames independently in both directions. Only when the cursor resolves to
nothing in the map (a genuinely unknown name, or a construct the resolver does
not yet model) does it fall back to today's name-based path.

Cross-file: `reference_scope` still bounds the *file set* (the defining file and
its importers); within each scoped file, resolve uses to the binding. An
imported item's uses across files share the item's identity through the
`WorkspaceIndex`.

Goto-definition jumps to the binding's `def_span`. Document highlight
distinguishes the binding from its reads via the map.

### Scope lints (`neoc-types` analysis, surfaced in `neoc-lsp`)

The binding model makes these precise:

- `unused_let` — a `let` binding whose `uses` is empty (with a `_`-prefix
  opt-out, per convention to settle in the RFC).
- `unused_param` — a function parameter never used. **Must exempt a parameter
  forced by a trait signature** (a trait method's params are mandatory on the
  impl even when unused; flagging them is a false positive — `STATIC.md` §3.7).
  v1 may scope this to free-function params and `_`-opt-out, deferring trait
  impls to the RFC.
- `shadowing` — a binding whose name equals an enclosing in-scope binding.
  `STATIC.md` §2 currently says "Shadowing is allowed and is not a warning," so
  this is **off by default** and the RFC must amend §2 before it lands.

These are **warnings**, not compiler errors. `lang/CONFORMANCE/static` is driven
by `neoc_types::check` and asserts *errors* (its `Diagnostic` has no severity),
so it is the wrong home — the existing five lints are warnings tested in
`neoc-lsp` (`lints.rs` unit tests + cucumber), not in CONFORMANCE. Land the
analysis in `neoc-types` (so any front-end can reuse it) but **test it in
`neoc-lsp`** like the other lints, *unless* the RFC decides to add a
warning-severity channel to compiler diagnostics — a larger change that would
then earn a CONFORMANCE home. Pick one in the RFC; do not assume CONFORMANCE.

## Phased plan (each its own MR)

1. `[x]` **Binding-site spans in the AST** — `span` on `Param::Named`,
   `Pattern::Binding`, `ClosureParam`, `FieldPat`, threaded through the parser.
   Mechanical; AST-equality goldens unaffected (`Span::eq` is always-true).
   (commit `a958a2b`.)
2. `[x]` **Resolution map in `neoc-types`** — `BindingId`, `Binding`,
   `Resolution` over locals **and** items; scope frames carry the id; uses
   recorded behind a flag at `infer_path` single-segment resolution only (not
   the `:840` field-init fallback); `neoc_types::resolution(module)` exposed.
   Unit-tested: param, both shadowing directions, local-shadows-item, item use.
   (commit `0807d48`.)
3. `[x]` **Wire references / rename / highlight / goto to the binding** — locals
   resolve via the map; items use the text match minus shadowing locals
   (`item_references`); goto-local jumps to the binding site; cross-file per-file
   step uses the item path. Tested both shadowing directions + item-vs-local.
   (commit `e68dc09`.)
4. `[~]` **Scope lints** — `unused_let` landed, scope-precise with `_`-opt-out
   (commit `f0658a1`). `unused_param` (trait-forced-param exemption) and
   `shadowing` (needs the §2 amendment) **remain, RFC-gated** — they change
   static semantics. Tested in `neoc-lsp` like the existing lints.
5. `[ ]` **Earn Phase 6** — the resolution recompute on edits is the first
   workspace consumer heavy enough to debounce/cancel (see
   `cross-file-resolution.md` §6).

## Remaining (RFC-gated)

- **`unused_param`** — needs the trait-forced-param exemption (a trait method's
  params are mandatory on the impl even when unused). The resolution map carries
  binding kind but not the enclosing-fn context; the lint must learn which params
  belong to a trait-impl method (an AST walk) before it can fire without false
  positives. RFC decides the `_`-opt-out and whether impl params are exempt.
- **`shadowing`** — `STATIC.md` §2 says "Shadowing is allowed and is not a
  warning." A shadowing lint contradicts that until the RFC amends §2.

## Pitfalls to avoid

- **Two resolvers.** Do not scope-walk the CST in `neoc-lsp`; consume the
  `neoc-types` map. One rule, shared.
- **Asymmetric shadowing.** Resolving only locals and leaving items name-based
  fixes renaming a local but not renaming an item a local shadows — the over-reach
  reappears in the other direction. Resolve *every* use to a binding.
- **The wrong recording site.** Record at `infer_path:399` only; the
  `scopes.lookup` at `:840` is a field-init type fallback, not an identifier use.
- **AST churn** — adding spans touches ~35 `Param::Named`/`Pattern::Binding`/
  `ClosureParam`/`FieldPat` sites (parser/types/codegens). Its own MR. Goldens are
  safe (`Span::eq` always-true); the cost is breadth, not risk.
- **`self` and pattern fields** — `self`, `Struct { x }` shorthand binds, and
  tuple/struct pattern fields are bindings too; enumerate them or the lints
  misfire. (For-loop and shorthand binds inherit the `Pattern` span for free.)
- **`unused_param` false positives** — trait-forced params are mandatory even
  when unused; exempt them.

## Acceptance

- Rename of a local edits only that binding's occurrences; a shadowing inner `x`
  and an outer `x` rename independently. `.feature` proves it.
- Rename of a **top-level/imported** name that a local shadows edits the item and
  its real uses, and **not** the shadowing local — in-file and cross-file.
- Find-references on any name returns exactly its binding's occurrences.
- `unused_let`/`unused_param`/`shadowing` fire precisely — no false positive on a
  used-but-shadowed binding, nor on a trait-forced unused param — tested in
  `neoc-lsp` beside the existing lints.
- `neoc check` output and the conformance goldens are unchanged (the map is
  opt-in; resolution does not alter typing or codegen).
- Cross-file *file-set* scoping (importers) is unchanged from Phase 3; only the
  within-file precision improves.

## Open questions (for the RFC)

- **`_`-prefix opt-out** for `unused_*`, and whether `shadowing` is on by
  default (Rust warns; many find it noisy). `STATIC.md` §2 currently *permits*
  shadowing without warning — the RFC must amend §2 to add a lint.
- **Lint severity channel.** Do the scope lints stay `neoc-lsp` warnings (tested
  there, like the five existing lints), or does the compiler gain a
  warning-severity diagnostic so they can carry a `CONFORMANCE/static` golden?
  The latter is the larger change.
- **`BindingId` stability** across edits — fine within one analysis; revisit if a
  persisted index needs stable ids.
- **Item-binding identity cross-file** — how an imported item's id in an importer
  ties to the defining file's id (via the `WorkspaceIndex`); spell the mechanism.
- **Generic type params and labels** — out of scope for v1 (value bindings only).
