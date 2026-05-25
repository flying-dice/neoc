# Browser / WASM build

- **Status:** blocked
- **Area:** `crates/neocc-wasm` (stub), `crates/neocc-grammar` (the blocker)
- **Priority:** P2

## Goal

A `wasm32-unknown-unknown` build of the compiler exposed via wasm-bindgen
(`neocc-wasm`), for in-browser transpilation / a playground, published to npm.

## The blocker

The parser graph links the tree-sitter **C runtime**, which doesn't cross-compile
to `wasm32-unknown-unknown`. The CI wasm gate therefore checks only the host-free
leaf crates (`neocc-ast`, `neocc-macros`, `neocc-codegen-lua`). Two ways out:

1. Compile the C parser to wasm via `cc` with the right target flags (tree-sitter
   core is small, self-contained C — feasible but fiddly).
2. Translate `parser.c` to Rust once with `c2rust` and vendor it (removes the C
   dep; heavier maintenance — regenerate on every grammar change).

## Acceptance

- [ ] `cargo build --target wasm32-unknown-unknown -p neocc-wasm` succeeds.
- [ ] `neocc-wasm` exposes a wasm-bindgen `transpile(src) -> JsValue`.
- [ ] CI wasm gate widened to the full compiler graph.

## Notes

- `neocc-wasm` must depend only on `neocc-compiler` + `wasm-bindgen` +
  `serde-wasm-bindgen` (CI enforces no extra deps once wired).
- No host I/O (`std::fs`, `tokio`, `mlua`, …) anywhere in the compiler crates.
