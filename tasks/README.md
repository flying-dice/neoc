# tasks/

The durable project task list for `neocc`. One Markdown file per work item so any
contributor — human or LLM — can pick up where the last one left off without
re-deriving context from git history.

This supersedes any in-session/ephemeral task tracking. If you're an LLM
continuing this project: read this index, pick a `todo` item (respect
`Depends on`), set its status to `in-progress`, do the work, then set it to
`done` and note the commit. Keep these files honest — verify a claimed status
against `git log` and the actual code before trusting it.

## Orientation (read these first)

- `lang/` — the language definition (the source of truth). `RATIONALE`,
  `NOTATION`, `LEXICAL`, `SYNTAX`, `STATIC`, `DYNAMIC`, `CONFORMANCE/`.
- `ARCHITECTURE.md` — the crate layout, the pipeline, the per-stage testing rules.
- `CLAUDE.md` — house rules: terse style, the golden rule (no host-language
  leaks), `ext` is the only runtime seam, Conventional Commits.
- `plan.md` — the build narrative + the gap triage.
- `.claude/skills/language-review/` — run this when changing the language.

## Status legend

`todo` · `in-progress` · `blocked` · `done` · `deferred` (intentional non-goal)

## Pipeline state (one line)

`parse → check → elaborate → emit → Lua 5.1` works end to end for the whole
language, with assertions at every stage. See `done.md`. What remains is below.

## Index

| File | Item | Status | Priority | Blocked on |
| --- | --- | --- | --- | --- |
| `runtime-integration.md` | `../neoc` runtime embeds `neocc` | blocked | P1 | parked `Result`-at-`ext` boundary decision; cross-repo |
| `wasm-build.md` | tree-sitter→wasm32 + `neocc-wasm` | blocked | P2 | tree-sitter C → wasm32 toolchain |
| `non-goals.md` | intentional deferrals (advanced Rust) | deferred | — | by design |
| `done.md` | completed work, for context | done | — | — |

**Every ready task is done.** Critical path complete (`cli` → `lsp` →
`lsp-spans` → `modules-visibility`), plus formatter, the LSP IDE features, the
three codegen targets, the `extern "<host>"` FFI, and the conformance breadth +
attribute checks. The only open items are both **blocked**: `runtime-integration`
awaits a boundary-design decision (and is cross-repo), and `wasm-build` awaits a
wasm toolchain. Completed items live in `done/` (see `done/README.md`).
