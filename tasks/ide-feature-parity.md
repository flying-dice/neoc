# IDE feature parity for `neoc-lsp`

- **Status:** in-progress
- **Priority:** P2
- **Depends on:** neoc#60 (cucumber IDE-spec harness — landed)

Build out `neoc-lsp` toward the editor-feature surface of mature servers
(rust-analyzer, the TypeScript language service). The inventory and upstream
test-suite cross-walk live in `tooling/ide/PRIOR-ART.md`; this file is the
ordered build plan. Each feature is its own MR with its own `.feature` spec.

## Ground rules

- **One feature, one MR, one `.feature`.** A feature lands when: pure handler in
  `crates/neoc-lsp/src/<feature>.rs`, wired into the `LanguageServer` impl +
  advertised in `ServerCapabilities`, a `tooling/ide/<feature>.feature` spec
  driven by `tests/cucumber.rs`, and unit tests beside the handler.
- **Golden rule binds.** Every rendered / surfaced string speaks neoc, never the
  host. Tooling-only — not part of the language re-implementation guarantee, so
  no `lang/CONFORMANCE/` entry.
- **Name-based, file-local resolution** is the current model (see `cst.rs`): no
  scope/shadowing analysis, no cross-file index. Features inherit that limit and
  document it. Cross-file + scope-aware resolution is a separate track (below).

## Current surface

Handlers (pure fns over `&str`, on the tree-sitter CST + `neoc-types` table):
diagnostics, document symbols, goto-definition, hover, completion. Spec coverage:
`hover.feature`.

## Build order

Dependency-ordered. `[ ]` todo · `[~]` in-progress · `[x]` done.

> **Progress:** building on `feat/lsp-ide-feature-parity`, one commit per
> feature. Shared signature renderer lifted into
> `hover::item_signature_and_doc`.

### Tier 1 — table-stakes

1. `[x]` **References** — `references.feature`. `textDocument/references`.
   Collect every `identifier` node matching the resolved name; honour
   `context.includeDeclaration`. Foundation rename + highlight reuse.
2. `[x]` **Completion resolve / details** — `completionItem/resolve`. Lazy
   signature + `///` docs on the focused item (URI round-tripped through
   `CompletionItem.data`). Reuses the shared signature renderer.
3. `[x]` **Signature help** — `signature.feature`. `textDocument/signatureHelp`.
   Callee signature + active parameter (comma count) at a free-function call.
   Method calls / tuple-struct constructors deferred (need receiver-type
   resolution); pinned by test.
4. `[x]` **Rename + prepare** — `rename.feature`. `rename`, `prepareRename`.
   Rename = references → `WorkspaceEdit`; `prepareRename` returns the
   identifier range or rejects a non-identifier cursor. New-name validity is
   left to the client for now (no keyword/identifier guard).
5. `[x]` **Goto type-definition / implementation** — `goto.feature`.
   type-definition jumps from a value to its named type's declaration (top-level
   values only — locals/params untracked); implementation lists the impl blocks
   targeting a type or implementing a trait. Goto-declaration skipped: redundant
   with definition in neoc (no header/impl split).
6. `[blocked]` **Code actions / quick-fixes** — `code-action.feature`.
   `codeAction`. Blocked: neoc's `transpile` diagnostics carry no structured fix
   data (just message + span + stage), so diagnostic-attached quick-fixes have
   nothing to hang off, and message-text matching would be a golden-rule-fragile
   hack. Needs either fix metadata on `neoc_compiler::Diagnostic` or a dedicated
   generative-refactor design. Deferred until that lands.
7. `[x]` **Document highlight** — `highlight.feature`. `documentHighlight`.
   The find-all-references set (already file-local) tagged read/write — the
   declaration is a write, its uses reads.

### Tier 2 — depth / polish

8. `[ ]` Workspace symbols (`workspace/symbol`) — needs the cross-file index.
9. `[x]` Folding ranges (`foldingRange`) — brace-delimited bodies + block
   comments, found generically from `{`/`}` CST tokens.
10. `[ ]` Inlay hints (`inlayHint`) — Type + Parameter kinds first.
11. `[x]` Selection range (`selectionRange`) — expand-selection up the CST
    ancestry, equal-range wrappers collapsed.
12. `[deferred]` Matching brace — rust-analyzer custom request
    (`experimental/matchingBrace`), not standard LSP; skip until there's a
    client that wants it.
13. `[ ]` Organize imports (code action `source.organizeImports`).

### Tier 3 — high effort / advanced

14. `[ ]` Semantic tokens (`semanticTokens/full`).
15. `[ ]` Call hierarchy.
16. `[ ]` Refactors (extract / inline).
17. `[ ]` Format passthrough via `neoc-fmt` (`formatting`, `rangeFormatting`).

### Deferred (dev-only / non-LSP / out of scope)

join lines, on-enter, on-type formatting, runnables / code lens, linked editing,
expand macro, view-IR, SSR. rust-analyzer has **no** type hierarchy — not a gap.

## Cross-cutting tracks (unblock several features)

- **Harness vocabulary.** `tests/cucumber.rs` step library grows reusable steps
  as features land: multi-file `Given the file "<path>":`, table `Then`s
  (references, completions, diagnostics — order-independent), post-edit-buffer
  `Then the source becomes:` (rename, code actions). See PRIOR-ART "Test-harness
  mechanics".
- **Cross-file + scope-aware resolution.** A document index + real name
  resolution (scopes, shadowing, imports). Unblocks correct references, rename,
  workspace symbols, auto-import. Largest single lift; schedule before Tier 2
  cross-file work. Track as its own task when it starts.
