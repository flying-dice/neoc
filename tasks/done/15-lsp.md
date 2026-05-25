# LSP — `neocc lsp` language server

- **Status:** done (diagnostics milestone)
- **Area:** `crates/neocc-lsp`, wired via `neocc lsp`
- **Commits:** `61eb8df` (compiler diagnostic span), `96102eb` (server + wiring)

## Outcome

`neocc lsp` is a `tower-lsp` server over stdio. On open/change it runs
`neocc_compiler::transpile` and publishes diagnostics. Verified end-to-end with
a real LSP handshake: `initialize` advertises `textDocumentSync: Full`, and a
`didOpen` of `fn main() { nope(); }` publishes `unknown name \`nope\`` with
`source: neocc:type`.

Spans, pragmatically: the parser already carries byte ranges, so `Diagnostic`
got an `Option<ByteSpan>` (`neocc-compiler`) — **syntax errors get precise
ranges**. Type/emit errors have no span yet, so they fall back to the
document's first line (the message still shows in the problem list). Precise
type-error ranges need spans on the AST → spun off as `lsp-spans.md`. Hover /
completion / goto are not implemented yet → `lsp-features.md`.

Tests: byte-offset→UTF-16 `Position` conversion (ASCII, multibyte, line
boundary, clamp), the `transpile`→LSP-diagnostic mapping, and a CLI test that
the built binary starts and exits cleanly on stdin close.

---

(original spec follows)

- **Priority:** P1 — the Zed extension (`flying-dice/neoc-zed`) already wires an
  LSP client; it has nothing to talk to until this exists.
- **Depends on:** `cli.md` (to launch it), `neocc-compiler`, `neocc-parser`

## Goal

A `tower-lsp`-based server. Start with the cheap, high-value features:

- **Diagnostics** — run `neocc_compiler::transpile` (or just parse + check) on
  change and publish parse/type diagnostics. **This needs real spans** — today
  diagnostics are span-free; the parser holds CST byte ranges, so thread spans
  from parse/check through to the LSP (see the AST-span note in `plan.md`).
- Then: hover (types from `neocc-types`), goto-definition, completion, document
  symbols / outline.

## Acceptance

- [ ] `neocc lsp` speaks LSP over stdio; an editor connects and shows diagnostics
      for a bad `.neoc` file.
- [ ] Diagnostics carry correct ranges.
- [ ] Tests for the request handlers (pure functions over a parsed document
      where possible).

## Notes / pointers

- Spans are the prerequisite. Decide where they live — likely a `Span` on AST
  nodes or a side-table keyed during the walk. Coordinate with `neocc-ast`
  (currently span-free by design).
- Hover/types come from `neocc-types` (`check`/`collect`/`Ty`).
- The Zed extension launches the binary `neocc` with args `["lsp"]`.
