# lang/

The language. This directory is the single source of truth for what neoc *is*.

Anything outside `lang/` — the Rust crates, the CLI, the LSP, the WASM build — is an **implementation** that must satisfy what's defined here. If implementation and spec disagree, the spec wins.

## Files

| File | What it pins down |
| --- | --- |
| `RATIONALE.md` | Why neoc exists; what it is; the design choices everything else refers back to. |
| `NOTATION.md` | The meta-notations the other files use: EBNF dialect for grammar, inference-rule shape for typing, lowering brackets for dynamic semantics. Read this first. |
| `LEXICAL.md` | Character set, tokens, keywords, literals, comments, whitespace. The alphabet of the language. |
| `SYNTAX.md` | The grammar. EBNF productions over the tokens from `LEXICAL.md`. Defines which token sequences are valid neoc programs. |
| `STATIC.md` | Name resolution, scoping, type system, well-formedness rules. Defines which valid programs are *meaningful*. |
| `DYNAMIC.md` | The lowering table from neoc constructs to Lua 5.1. Defines what each meaningful program *does* when executed. |
| `CONFORMANCE/` | Executable test pairs — `.neoc` source plus expected `.lua` (or expected stdout). The contract every implementation must pass. |

## Reading order

For a newcomer: `RATIONALE.md` → `NOTATION.md` → `LEXICAL.md` → `SYNTAX.md` → `STATIC.md` → `DYNAMIC.md`. Each builds on the one before it.

For an implementer: the same order, but every claim in `STATIC.md` and `DYNAMIC.md` has a matching entry under `CONFORMANCE/`. A passing impl is one whose output matches every conformance case.

## Editing rules

- A change to any file here is a change to the language. The `language-review` skill in `.claude/skills/` enforces the cross-cutting sync rules.
- The spec must not name a host language, an implementing crate, a tooling library (Rust, `tree-sitter`, `mlua`, …), or anything else that would prevent a re-implementation in OCaml or Zig from satisfying the same text. See `CLAUDE.md` golden rule.
- Worked examples in spec files use neoc syntax and are themselves valid programs that appear in `CONFORMANCE/`.
