# IDE feature prior art — what `neoc-lsp` should replicate

Research collection feeding the cucumber IDE spec (`tooling/ide/*.feature`,
driven by `crates/neoc-lsp/tests/cucumber.rs`, neoc#60). Two mature reference
servers, cross-walked against neoc's current handlers, mapped to **their test
suites** so each neoc `.feature` can mirror a known-good upstream contract.

- **rust-analyzer** (`rust-lang/rust-analyzer`) — the `crates/ide*` family.
- **TypeScript language service** (`microsoft/TypeScript`) — `src/services/` +
  the `fourslash` test DSL.

Golden rule still binds: every hovered / completed / diagnosed / rendered string
**speaks neoc, never the host**. These tables describe *which interactions* to
support and *how upstream proves them* — not their output strings.

## neoc today

`crates/neoc-lsp/src/` — pure functions over the tree-sitter CST (`neoc-grammar`)
+ the `neoc-types` symbol table:

| Handler | LSP request | Source |
| --- | --- | --- |
| Diagnostics | `publishDiagnostics` | `lib.rs` (`transpile`) |
| Document symbols | `documentSymbol` | `symbols.rs` |
| Goto definition | `definition` | `goto.rs` |
| Hover | `hover` | `hover.rs` |
| Completion | `completion` | `completion.rs` |

Spec coverage so far: `hover.feature` (1 live scenario + 1 `@pending`).

## Feature matrix

`★` = neoc has it · `▷` = next tier · `○` = later · `·` = dev-only/defer.
"feature file" = the `tooling/ide/*.feature` that should own the contract.

| # | Feature | LSP request | rust-analyzer module · test | TS method · fourslash cmd | neoc | feature file |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Hover / quick info | `hover` | `ide/src/hover.rs` · `hover/tests.rs` (`check`) | `getQuickInfoAtPosition` · `verify.quickInfoAt` | ★ | `hover.feature` |
| 2 | Diagnostics | `publishDiagnostics`/`diagnostic` | `ide-diagnostics/` · `tests.rs` `check_diagnostics` (inline `//^^^ error:`) | `getSemantic/Syntactic/SuggestionDiagnostics` · `verify.baselineSyntacticAndSemanticDiagnostics` | ★ | `diagnostics.feature` |
| 3 | Document symbols / outline | `documentSymbol` | `ide/src/file_structure.rs` (expect-test) | `getNavigationTree` · `verify.navigationTree` | ★ | `symbols.feature` |
| 4 | Goto definition | `definition` | `ide/src/goto_definition.rs` (`check`, `cov_mark`) | `getDefinitionAndBoundSpan` · `verify.baselineGoToDefinition` | ★ | `goto.feature` |
| 5 | Completion (core) | `completion` | `ide-completion/` · `tests/` `check(fixture, expect!)`, `check_edit` | `getCompletionsAtPosition` · `verify.completions` | ★ | `completion.feature` |
| 6 | Completion resolve / details | `completionItem/resolve` | flyimport in `completions/flyimport.rs` · `tests/flyimport.rs` | `getCompletionEntryDetails` · `verify.applyCodeActionFromCompletion` | ▷ | `completion.feature` |
| 7 | Find all references | `references` | `ide/src/references.rs` (inline `mod tests`) | `getReferencesAtPosition` · `verify.baselineFindAllReferences` | ▷ | `references.feature` |
| 8 | Rename + prepare | `rename`, `prepareRename` | `ide/src/rename.rs` (`check`, `check_prepare`) | `getRenameInfo`+`findRenameLocations` · `verify.baselineRename` | ▷ | `rename.feature` |
| 9 | Document highlight | `documentHighlight` | `ide/src/highlight_related.rs` | `getDocumentHighlights` · `verify.baselineDocumentHighlights` | ▷ | `highlight.feature` |
| 10 | Goto type definition | `typeDefinition` | `ide/src/goto_type_definition.rs` | `getTypeDefinitionAtPosition` · `verify.baselineGoToType` | ▷ | `goto.feature` |
| 11 | Goto implementation | `implementation` | `ide/src/goto_implementation.rs` | `getImplementationAtPosition` · `verify.baselineGoToImplementation` | ▷ | `goto.feature` |
| 12 | Goto declaration | `declaration` | `ide/src/goto_declaration.rs` | (folded into definition) | ○ | `goto.feature` |
| 13 | Signature help | `signatureHelp` | `ide/src/signature_help.rs` (`check`) | `getSignatureHelpItems` · `verify.signatureHelp` | ▷ | `signature.feature` |
| 14 | Code actions / quick-fixes | `codeAction`, `codeAction/resolve` | `ide-assists/` (200+) · `tests.rs` `check_assist(h, before, after)` | `getCodeFixesAtPosition` · `verify.codeFix` | ▷ | `code-action.feature` |
| 15 | Workspace symbols | `workspace/symbol` | `ide-db/src/symbol_index.rs` | `getNavigateToItems` · `verify.navigateTo` | ○ | `workspace-symbols.feature` |
| 16 | Folding ranges | `foldingRange` | `ide/src/folding_ranges.rs` (`check_folds`) | `getOutliningSpans` · `verify.outliningSpansInCurrentFile` | ○ | `folding.feature` |
| 17 | Inlay hints | `inlayHint`, `inlayHint/resolve` | `ide/src/inlay_hints/` per-kind · inline tests | `provideInlayHints` · `verify.baselineInlayHints` | ○ | `inlay-hints.feature` |
| 18 | Selection range / expand | `selectionRange` | `ide/src/extend_selection.rs` | `getSmartSelectionRange` · `verify.baselineSmartSelection` | ○ | `selection.feature` |
| 19 | Matching brace | (custom `matchingBrace`) | `ide/src/matching_brace.rs` | `getBraceMatchingAtPosition` · `verify.matchingBracePositionInCurrentFile` | ○ | `matching-brace.feature` |
| 20 | Semantic tokens / highlight | `semanticTokens/full`,`/range`,`/delta` | `ide/src/syntax_highlighting.rs` · HTML snapshots | `getEncodedSemanticClassifications` · `verify.semanticClassificationsAre` | ○ | `semantic-tokens.feature` |
| 21 | Format (doc/range/on-type) | `formatting`,`rangeFormatting`,`onTypeFormatting` | shells to rustfmt (not an ide module) | `getFormattingEditsFor*` · `format.document`/`onType` | ○ | `format.feature` (neoc has `neoc-fmt`) |
| 22 | Organize imports | `codeAction` (`source.organizeImports`) | — | `organizeImports` · `verify.organizeImports` | ○ | `code-action.feature` |
| 23 | Call hierarchy | `prepareCallHierarchy`,`incoming/outgoingCalls` | `ide/src/call_hierarchy.rs` | `prepareCallHierarchy`+ calls · `verify.baselineCallHierarchy` | ○ | `call-hierarchy.feature` |
| 24 | Refactors | `codeAction` (`refactor.*`) | `ide-assists/` (`RefactorExtract`/`Inline`/`Rewrite`) | `getApplicableRefactors`+`getEditsForRefactor` · `verify.refactorAvailable` | · | `code-action.feature` |
| 25 | Join lines | (custom `joinLines`) | `ide/src/join_lines.rs` (`check_join_lines`) | — | · | — |
| 26 | On-enter | (custom `onEnter`) | `ide/src/typing/on_enter.rs` | — | · | — |
| 27 | Runnables / code lens | `codeLens`, `experimental/runnables` | `ide/src/runnables.rs`, `annotations.rs` | (n/a) | · | — |
| 28 | Linked editing | `linkedEditingRange` | (n/a) | `getLinkedEditingRangeAtPosition` · `verify.linkedEditing` | · | — |
| 29 | Expand macro / view IR | (custom) | `expand_macro.rs`, `view_*` | (n/a) | · | — |
| 30 | SSR | `experimental/ssr` | `ide-ssr/` | — | · | — |

Note: rust-analyzer has **no** type-hierarchy (only call-hierarchy). TS has a
large band of editor-specific, non-LSP ops (todo comments, doc-comment template,
JSX closing tag, paste-with-imports, `mapCode`, comment toggling) — out of scope
for neoc; listed only so we don't mistake them for gaps.

## Completion sub-categories (rust-analyzer)

When `completion.feature` grows, mirror these as scenarios:

| Category | ra module | Note |
| --- | --- | --- |
| keyword | `completions/keyword.rs` | keyword candidates by position |
| path / expr / type / pattern | `completions/{expr,type,use_,pattern}.rs` | position-aware |
| dot / field / postfix | `completions/{dot,field}.rs`, `postfix/` | `.method`, `.if`, `.match` |
| auto-import (flyimport) | `completions/flyimport.rs` | proposes + inserts the import |
| snippet | `completions/snippet.rs` | user-defined |
| record / fn-param / attribute | `completions/{record,fn_param,attribute}.rs` | structural |

## Inlay-hint kinds (rust-analyzer `InlayKind`)

Start with **Type** + **Parameter** only. Full set, each its own module under
`ide/src/inlay_hints/`: Type, Parameter, Chaining, ClosingBrace, ClosureCapture,
ClosureReturn, Adjustment, BindingMode, Discriminant, GenericParameter, Lifetime,
Bounds, Drop, RangeExclusive.

## Test-harness mechanics — what to steal

Both servers express IDE tests as **annotated source fixtures + an assertion**.
neoc already does the Gherkin equivalent (`Given the source: """…"""`, a `When`
locating the caret, a `Then` asserting). To extend the cucumber suite to the
features above we need the same primitives the upstreams have:

**rust-analyzer** (`crates/test-utils/src/fixture.rs`, `crates/ide/src/fixture.rs`):
- `$0` = caret; `$0 … $0` = range. → neoc: the `When` step already locates a
  caret by identifier+occurrence (`hover_at`). Add a range variant.
- `//- /path.rs` = multi-file; per-file meta (`crate:`, `deps:`, `edition:`). →
  neoc: a multi-file `Given` once cross-file features (references, workspace
  symbols, auto-import) land.
- `//^^^ error: msg` inline annotations + `check_diagnostics` renders actual in
  the same format and diffs. → neoc: directly applicable to `diagnostics.feature`
  — assert the set of `(range, severity, message)` order-independent (matches
  `CONFORMANCE/static/`'s `.diagnostics` convention).
- `check_assist(handler, before, after)` — before carries `$0`, after is the
  whole post-edit buffer. → neoc: `code-action.feature` / `rename.feature` shape:
  `Given source … When I apply <action> at <ident> … Then the source becomes:`.
- expect-test inline snapshots (`UPDATE_EXPECT=1` rewrites). → neoc: docstring
  `Then` blocks are the equivalent golden; keep them inline.
- `cov_mark::hit!`/`check!` proves a specific branch ran.

**TypeScript** (`src/harness/fourslashImpl.ts`, `fourslashInterfaceImpl.ts`):
- `/*marker*/` named position, `/**/` anonymous, `[| range |]`,
  `[|{ "kind": … } text |]` JSON-tagged range.
- `// @Filename:` multi-file; compiler-option directives (`// @target:` …).
- Two assertion modes: **inline** (`verify.quickInfoIs`, `verify.completions`)
  and **baseline** — serialise result, diff against committed
  `tests/baselines/reference/*.baseline`. → neoc: inline docstrings cover the
  first mode; for large structured outputs (reference graphs, call hierarchy)
  consider a committed golden file referenced from the step, à la baseline.
- `tests/cases/fourslash/server/` drives the wire protocol end-to-end — the
  closest analogue to testing `neoc-lsp` over stdio rather than the pure handler.

**Design implication for neoc's `.feature` vocabulary.** The step library in
`cucumber.rs` needs to grow these reusable steps:
- `Given the source:` (have) and `Given the file "<path>":` (multi-file).
- `When I <verb> on "<ident>" at its declaration | at the call site` (have for
  hover) — generalise the caret locator across handlers.
- `When I request references on "<ident>"` / `… completion at "<marker>"` / `…
  rename "<a>" to "<b>"` / `… apply "<fix>"`.
- `Then the definition is at "<ident>"`, `Then the references are:` (table),
  `Then the completions include:` (table), `Then the diagnostics are:` (table,
  order-independent), `Then the source becomes:` (docstring, post-edit buffer).

## Roadmap — replication order

Both agents converged on the same ordering. Tiered for a server that today has
diagnostics + symbols + goto-def + hover + completion:

**Tier 1 — table-stakes (build next, roughly in order):**
1. **References** (`references.feature`) — the resolver other features reuse.
2. **Completion resolve / details** — completion is hollow without lazy docs +
   auto-import edit; biggest UX multiplier on what's already there.
3. **Signature help** (`signature.feature`).
4. **Rename + prepare** (`rename.feature`) — builds on references.
5. **Goto type-definition / implementation / declaration** (`goto.feature`) —
   small deltas on goto-def.
6. **Code actions / quick-fixes** (`code-action.feature`) — pairs with the
   diagnostics we already emit; start with diagnostic-attached fixes.
7. **Document highlight** (`highlight.feature`) — cheap once references exist.

**Tier 2 — depth / polish:**
8. Workspace symbols · 9. Folding ranges · 10. Inlay hints (Type+Parameter
first) · 11. Selection range · 12. Matching brace · 13. Organize imports.

**Tier 3 — high effort or advanced:**
14. Semantic tokens · 15. Call hierarchy · 16. Refactors (extract/inline) ·
17. Format passthrough via `neoc-fmt`.

**Defer (dev-only / non-LSP / out of scope):**
join lines, on-enter, on-type, runnables/code lens, linked editing, expand
macro, view-IR, SSR, and the TS editor-specific band.

## Sources

- ra feature manual: <https://rust-analyzer.github.io/book/features.html>
  (generated from doc-comments by `cargo xtask codegen feature-docs`).
- ra IDE crates: `crates/ide`, `ide-assists`, `ide-completion`,
  `ide-diagnostics`; LSP→analysis map in
  `crates/rust-analyzer/src/handlers/request.rs`.
- ra harness: `crates/test-utils/src/fixture.rs`, `crates/ide/src/fixture.rs`,
  `crates/ide-assists/src/tests.rs`, `crates/ide-diagnostics/src/tests.rs`.
- TS LanguageService interface: `src/services/types.ts`; impls in `src/services/`
  (`completions.ts`, `findAllReferences.ts`, `goToDefinition.ts`,
  `signatureHelp.ts`, `rename.ts`, `inlayHints.ts`, `refactors/`, `codefixes/`).
- TS fourslash: `src/harness/fourslashImpl.ts`,
  `src/harness/fourslashInterfaceImpl.ts`; tests in `tests/cases/fourslash/`
  (+ `/server/` for wire-protocol), baselines in `tests/baselines/reference/`.
- LSP 3.17:
  <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/>
