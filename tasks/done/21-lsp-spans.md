# Precise ranges for type/emit diagnostics (AST spans)

- **Status:** done
- **Commits:** `c0ae4dc` (ast Span), `a4e06b8` (parser/codegen refactor),
  `047414b` (located diagnostics)

## Outcome

AST nodes now carry byte-offset `Span`s, so type diagnostics land on the
offending node instead of the document's first line. `Expr`/`Stmt` became
`{ kind, span }` structs (wrapping new `ExprKind`/`StmtKind`); `Item` gained a
`span`. The key to not churning every AST-equality test: `Span`'s `PartialEq`
always returns `true`, so a node carrying a span still compares structurally —
the parser's `assert_eq!`-on-AST tests passed unchanged (~335 match/construction
sites updated across parser/types/codegen). The parser fills spans from CST
`byte_range()`; `elaborate` propagates them onto rewritten nodes. `neocc-types`
diagnostics gained a span (`error_at`), set for the common errors (unknown name,
type/arg/arity mismatch, field/method/index errors, match exhaustiveness, …);
`neocc-compiler` carries it into `Diagnostic.span`, and the LSP already renders
it. 428 tests, conformance 212. `expect()`-family mismatches keep the `0,0`
fallback (→ file start) — a noted follow-up.

---

(original spec follows)

- **Priority:** P1 — the LSP works (`done/15-lsp.md`), but type errors land on
  the document's first line instead of the offending node.

## Problem

`neocc_compiler::Diagnostic` carries `Option<ByteSpan>`, populated for parse
diagnostics (the parser has byte ranges). Type and emit diagnostics have no
span, so the LSP falls back to line 0. To locate them, the type checker needs
the span of the node it is reporting against — which means the AST must carry
spans. The AST is span-free by design today (`neocc-ast/src/lib.rs`).

## Approach

A stashed WIP exists: `git stash list` → "wip: agent's incomplete AST-span
refactor (enum->kind+span)". It converted `Expr`/`Stmt` into `{ kind, span }`
structs (mirroring how `Item` already splits into `kind: ItemKind`). It did not
compile — it broke every match/constructor in codegen/elaborate/compiler and
all the AST-equality tests, and never reached the LSP. Treat it as reference,
not a base to resume.

Recommended, lower-blast-radius path:

1. Add `Span { start, end }` to `neocc-ast` with a **manually-`true` `PartialEq`**
   so nodes that carry a span still compare structurally-equal — the
   AST-equality tests keep passing unchanged.
2. Attach `span: Span` to the nodes the checker reports against (`Expr`, plus
   `Stmt`/`Item` as needed). Adding a field still breaks struct *literals*, so
   update constructors (a `Expr::new(kind, span)` helper keeps call sites tidy).
3. Populate spans in `neocc-parser` from each CST node's `byte_range()`. Make
   `elaborate` propagate the original span onto rewritten nodes.
4. Give `neocc-types::Diagnostic` a span; set it from the reported node. Carry
   it into `neocc-compiler::Diagnostic.span`. The LSP already renders whatever
   span it gets — no LSP change needed once the span is non-`None`.

## Acceptance

- [ ] A type error (`unknown name`, mismatch, arity, non-exhaustive) highlights
      the offending node, not line 0.
- [ ] AST-equality tests still pass (the `Span` equality trick).
- [ ] `cargo test --workspace`, clippy, fmt all clean.
