# Done — completed work (for context)

- **Status:** done (reference; verify against `git log` if in doubt)

What already works, so a new contributor doesn't redo it. The
`parse → check → elaborate → emit → Lua 5.1` pipeline is live end to end with
assertions at every stage.

## Language definition (`lang/`)
- `RATIONALE`, `NOTATION`, and the four layers `LEXICAL` / `SYNTAX` / `STATIC` /
  `DYNAMIC` authored (Rust-shaped, not the bun-oas TS dialect). `SYNTAX` §10
  (consolidated EBNF) outstanding — see `conformance-breadth.md`.

## Compiler crates
- `neocc-grammar` — tree-sitter grammar, consumed as the `tree-sitter-neoc`
  submodule; `cc`-builds its parser; CST parse tests.
- `neocc-ast` — full typed AST (span-free, `PartialEq`).
- `neocc-parser` — modular CST→AST adapter (every construct); AST-stage tests.
- `neocc-types` — declaration collection, inference, checks (unknown names,
  type mismatches, struct-literal fields, `match` exhaustiveness, `?`
  well-formedness, trait-impl completeness), `elaborate` (string `+` → `Concat`),
  and derived-method awareness (`clone`/`eq`/`debug`/`display` are callable).
- `neocc-macros` — `Clone`/`Eq`/`Debug`/`Display` derives, structural over the AST.
- `neocc-codegen-lua` — emitter for the language: fn, structs (metatable
  dispatch), enums, `impl`, control flow, operators incl. string concat (`..`),
  calls, fields, indexing, struct/enum construction, lists, closures, `match` +
  patterns, `?`, derive splicing. (Remaining emitter gaps tracked in
  `emitter-limitations.md`.)
- `neocc-compiler` — `transpile(src) -> CompileOutput` (parse → check →
  elaborate → emit), diagnostics tagged by stage.

## Tests (assert at every stage)
CST parse tests · AST-shape tests · emit goldens incl. hand-built-AST (parser-
independent) · static `check` diagnostics · elaborate rewrites · end-to-end
(run under `lua`, assert stdout) · `lang/CONFORMANCE/dynamic/` corpus + runner.
The conformance corpus has already caught and fixed two real bugs.

## Ecosystem
- Three GitLab repos under `flying-dice` (private): `neocc`, `tree-sitter-neoc`
  (canonical grammar, submodule of `neocc`), `neoc-zed` (Zed extension pointing
  at `tree-sitter-neoc`).

## Stubs (not done — see task files)
`neocc` CLI bin, `neocc-lsp`, `neocc-fmt`, `neocc-wasm` are stubs.
