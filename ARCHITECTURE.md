# Architecture

How `neocc` is structured. The language itself is defined in `lang/` (the source of truth); this document is about the *implementation* that realises it.

## Pipeline

```
.neoc source
   │  tree-sitter parse            crates/neocc-grammar
   ▼
CST (tree-sitter tree)
   │  walk + adapt                 crates/neocc-parser
   ▼
typed AST                          crates/neocc-ast
   │  type-check                   crates/neocc-types  → diagnostics
   ▼
typed AST
   │  elaborate                    crates/neocc-types  (string `+` → Concat)
   ▼
elaborated AST
   │  emit (+ derives)             crates/neocc-codegen-lua, crates/neocc-macros
   ▼
Lua 5.1 source
```

`crates/neocc-compiler` orchestrates the run (`transpile(src) -> CompileOutput`) and is the stable seam an embedding runtime depends on. Each stage is single-purpose and reads its predecessor's output without mutating it.

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| `neocc-grammar` | vendors the `tree-sitter-neoc` grammar (git submodule); `cc`-builds its parser; exposes the `LANGUAGE` binding | (C runtime) |
| `neocc-ast` | the typed AST — pure data, span-free, `PartialEq` | — |
| `neocc-parser` | CST → AST adapter, one walker per construct family; parse diagnostics | ast, grammar |
| `neocc-types` | declaration collection, inference, well-formedness checks, `elaborate` | ast |
| `neocc-macros` | built-in derives (Clone/Eq/Debug/Display), structural over the AST | ast |
| `neocc-codegen-lua` | AST → Lua 5.1; one emit module per construct family | ast, macros |
| `neocc-compiler` | the public lib API: `transpile`; merges per-stage diagnostics | ast, parser, types, codegen-lua, macros |
| `neocc-fmt` | formatter (stub) | ast, parser |
| `neocc-lsp` | language server (stub) | compiler, parser |
| `neocc-wasm` | browser entry point (stub) | compiler |
| `neocc` | the CLI binary | compiler |

The dependency graph is acyclic and the AST is the frozen contract every stage shares.

## Conventions

### Naming
- `walk_<construct>` — CST → AST, only in `neocc-parser`.
- `emit_<construct>` / `lower` helpers — AST → Lua, only in `neocc-codegen-lua`.
- `infer_*` / `check_*` — typing, only in `neocc-types`.
- `derive` — Lua for a built-in trait, only in `neocc-macros`.

### Testing — assert at every stage
A bug must be caught at the stage that introduced it, not only end to end:
- **CST** — `neocc-grammar` parse tests over source.
- **AST** — `neocc-parser` tests assert the produced AST against an expected value, independent of emit.
- **Emit** — `neocc-codegen-lua` golden tests, including **hand-built-AST → Lua** so an emitter bug shows even when the parser is wrong, plus source → Lua goldens.
- **Static** — `neocc-types` tests assert the diagnostics on ill-typed programs and none on well-typed ones.
- **End-to-end** — `neocc-compiler` transpiles and runs the Lua under an interpreter, asserting stdout.
- **Conformance** — `lang/CONFORMANCE/` holds impl-agnostic `.neoc` + expected-output cases.

### Modularity
Each construct family lives in its own module in each crate (`parser/{item,ty,expr,pattern}.rs`, `codegen-lua/{lib,expr,pat}.rs`, `types/{collect,infer,env,ty,elaborate}.rs`). Adding a language feature is additive across these per-construct sites, which is what lets the work parallelise.

## Boundaries (enforced)
- The compiler crates carry no host-language assumptions: no `tokio`, `mlua`, `hyper`, no filesystem in the library path. I/O lives in the `neocc` bin and (over stdio) `neocc-lsp`.
- The pure leaf crates (`neocc-ast`, `neocc-macros`, `neocc-codegen-lua`) build for `wasm32-unknown-unknown`; CI checks them. The parser graph links the tree-sitter C runtime, which doesn't cross-compile to wasm32 yet — a tracked limitation.
- No host-language detail (Rust types, `tree-sitter`, crate names) appears in a user-facing diagnostic or in `lang/`. See `CLAUDE.md`'s golden rule.
