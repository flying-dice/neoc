# Grammar — tree-sitter + standalone repo + submodule

- **Status:** done
- **Area:** `crates/neocc-grammar`, `flying-dice/tree-sitter-neoc`
- **Commits:** `0eeba24` (grammar), `c52aac3` (submodule conversion)

## What was done

- A fresh tree-sitter `grammar.js` matching `lang/SYNTAX.md` (not the bun-oas
  grammar). Single identifier token (binding-vs-variant deferred to name
  resolution, as Rust does); a dedicated condition-expression hierarchy keeps the
  struct-literal-vs-block ambiguity out of `if`/`while`/`for`/`match` heads.
- `build.rs` compiles the generated `parser.c` via `cc`; `lib.rs` exposes the
  `LANGUAGE` binding + `NODE_TYPES`; 5 CST parse tests.
- The grammar was extracted to a standalone repo `flying-dice/tree-sitter-neoc`
  (canonical) and `neocc-grammar` now consumes it as a **git submodule** at
  `crates/neocc-grammar/tree-sitter-neoc` (relative URL for CI). One source of
  truth; edit in the submodule, regenerate, bump here.
