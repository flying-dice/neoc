# Conformance breadth + consolidated grammar

- **Status:** done — static (24) + syntax (7+7) corpora + runners + SYNTAX §10 EBNF; lexical deferred (no token API). Commits `3392870` `2bad0b0` `d880d4e`.
- **Area:** `lang/CONFORMANCE/`, `lang/SYNTAX.md`
- **Priority:** P2

## Goal

The dynamic conformance corpus exists (`lang/CONFORMANCE/dynamic/`, run by
`crates/neocc-compiler/tests/conformance.rs`). Fill the rest:

- **`lang/CONFORMANCE/lexical/`** — tokenisation cases (`.neoc` → expected
  tokens), per `lang/CONFORMANCE/README.md`.
- **`lang/CONFORMANCE/syntax/`** — parse / reject cases (`.neoc` → CST, or
  `.reject` + expected error category).
- **`lang/CONFORMANCE/static/`** — type-check cases (`.neoc` → expected
  diagnostics).
- **`lang/SYNTAX.md` §10** — author the consolidated EBNF block (currently "to be
  authored"); §1–§9 are the commentary.

## Acceptance

- [x] `static/` — 24 cases (`name.neoc` + `name.diagnostics`) + runner
      (`crates/neocc-compiler/tests/conformance_static.rs`), order-independent set match.
- [x] `syntax/` — 7 accept (`name.neoc`) + 7 reject (`name.reject` +
      `name.reject.expected`) + runner (`conformance_syntax.rs`), parses via
      `neocc_grammar::LANGUAGE` and asserts no-error / has-error.
- [ ] `lexical/` — **deferred**. neoc has no flat tokeniser exposing the
      `LEXICAL.md` §8 token classes; tree-sitter yields a CST, not a
      `KIND@line:col "lexeme"` stream. Synthesising one in the runner would
      fabricate a token API the spec lacks and violate the "spec leads, never
      copy the impl" rule. See `lang/CONFORMANCE/lexical/README.md`. Land cases
      here once a `neocc_lexer` exists.
- [x] `SYNTAX.md` §10 — single EBNF block, all §1–§9 productions in dependency
      order, cross-checked against `crates/neocc-grammar/grammar.js` (notes the
      grammar-faithful additions: `loop`/`while`/`for`, `extern`, `let mut`,
      the `CondExpr` struct-literal restriction).

## Notes

- The dynamic corpus already caught two real bugs (Result/Option match
  fall-through; unit-variant exhaustiveness). The other layers are likely to be
  similarly valuable — expect them to surface lexer/parser/checker edge cases.
