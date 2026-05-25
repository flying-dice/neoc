# Architecture

How `neoc` is structured. The language itself is defined in `lang/` (the source of truth); this document is about the *implementation* that realises it.

## Pipeline

```
.neoc source
   │  tree-sitter parse            crates/neoc-grammar
   ▼
CST (tree-sitter tree)
   │  walk + adapt                 crates/neoc-parser
   ▼
typed AST                          crates/neoc-ast
   │  type-check                   crates/neoc-types  → diagnostics
   ▼
typed AST
   │  elaborate                    crates/neoc-types  (string `+` → Concat)
   ▼
elaborated AST
   │  emit (+ derives)             crates/neoc-codegen-{lua,js}, crates/neoc-macros
   ▼
Lua 5.1 / JavaScript (ES2022) source
```

`crates/neoc-compiler` orchestrates the run (`transpile(src, target) -> CompileOutput`) and is the stable seam an embedding runtime depends on; the target selects the codegen backend (`lua` / `js`). Each stage is single-purpose and reads its predecessor's output without mutating it.

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| `neoc-grammar` | vendors the `tree-sitter-neoc` grammar (git submodule); `cc`-builds its parser; exposes the `LANGUAGE` binding | (C runtime) |
| `neoc-ast` | the typed AST — pure data, span-free, `PartialEq` | — |
| `neoc-parser` | CST → AST adapter, one walker per construct family; parse diagnostics | ast, grammar |
| `neoc-types` | declaration collection, inference, well-formedness checks, `elaborate` | ast |
| `neoc-macros` | built-in derives (Clone/Eq/Debug/Display), structural over the AST | ast |
| `neoc-codegen-lua` | AST → Lua 5.1; one emit module per construct family | ast, macros |
| `neoc-codegen-js` | AST → JavaScript (ES2022); mirrors `neoc-codegen-lua`'s structure | ast, macros |
| `neoc-compiler` | the public lib API: `transpile` to any target; merges per-stage diagnostics | ast, parser, types, codegen-lua, codegen-js, macros |
| `neoc-fmt` | formatter | ast, parser |
| `neoc-lsp` | language server | compiler, parser |
| `neoc-wasm` | browser entry point | compiler |
| `neoc-vm` | the Luau runtime: engine construction, script execution, `.neoc` header emission; owns the `mlua`/`tokio` stack and the `std:*`/`lib:*`/`vnd:*` modules | (runtime deps) |
| `neoc` | the `neoc` binary: the unified `clap` tree and the folded compiler subcommands (`build`/`check`/`fmt`/`lsp`); dispatches `run`/`emit-headers` to `neoc-vm` | vm, compiler, fmt, lsp |

The dependency graph is acyclic and the AST is the frozen contract every stage shares.

## Conventions

### Naming
- `walk_<construct>` — CST → AST, only in `neoc-parser`.
- `emit_<construct>` / `lower` helpers — AST → Lua, only in `neoc-codegen-lua`.
- `infer_*` / `check_*` — typing, only in `neoc-types`.
- `derive` — Lua for a built-in trait, only in `neoc-macros`.

### Testing — assert at every stage
A bug must be caught at the stage that introduced it, not only end to end:
- **CST** — `neoc-grammar` parse tests over source.
- **AST** — `neoc-parser` tests assert the produced AST against an expected value, independent of emit.
- **Emit** — `neoc-codegen-lua` golden tests, including **hand-built-AST → Lua** so an emitter bug shows even when the parser is wrong, plus source → Lua goldens.
- **Static** — `neoc-types` tests assert the diagnostics on ill-typed programs and none on well-typed ones.
- **End-to-end** — `neoc-compiler` transpiles and runs the Lua under an interpreter, asserting stdout.
- **Conformance** — `lang/CONFORMANCE/` holds impl-agnostic `.neoc` + expected-output cases.

### Modularity
Each construct family lives in its own module in each crate (`parser/{item,ty,expr,pattern}.rs`, `codegen-lua/{lib,expr,pat}.rs`, `types/{collect,infer,env,ty,elaborate}.rs`). Adding a language feature is additive across these per-construct sites, which is what lets the work parallelise.

## Boundaries (enforced)
- The compiler crates carry no host-language assumptions: no `tokio`, `mlua`, `hyper`, no filesystem in the library path. I/O lives in the `neoc` bin and (over stdio) `neoc-lsp`.
- The pure leaf crates (`neoc-ast`, `neoc-macros`, `neoc-codegen-lua`) build for `wasm32-unknown-unknown`; CI checks them. The parser graph links the tree-sitter C runtime, which doesn't cross-compile to wasm32 yet — a tracked limitation.
- No host-language detail (Rust types, `tree-sitter`, crate names) appears in a user-facing diagnostic or in `lang/`. See `CLAUDE.md`'s golden rule.
