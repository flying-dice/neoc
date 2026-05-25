# Emitter core + pipeline (`neocc-codegen-lua`, `neocc-compiler`)

- **Status:** done (remaining emitter gaps tracked in `tasks/emitter-limitations.md`)
- **Area:** `crates/neocc-codegen-lua`, `crates/neocc-compiler`
- **Commit:** `40b926d`

## What was done

- `neocc-codegen-lua` — AST → Lua 5.1, modular (`lib.rs` items, `expr.rs`
  expressions, `pat.rs` patterns). The expression-vs-statement gap is bridged
  with IIFEs for block-like forms in value position. Lowers functions, structs,
  consts, `use`→`require`, control flow, operators, calls, field access,
  indexing, struct literals, lists, closures, assignment.
- `neocc-compiler` — the public seam `transpile(src) -> CompileOutput`,
  running parse → check → elaborate → emit, diagnostics tagged by stage.
- Per-stage tests: hand-built-AST → Lua goldens (parser-independent) + source →
  Lua goldens; end-to-end tests run the emitted Lua under `lua` and assert stdout.

Method dispatch uses Lua metatables (`T.__index = T` + `setmetatable`), so
`x.m(a)` lowers to the colon call `x:m(a)` with no type info needed at the call
site (`DYNAMIC.md` §3.1/§3.3/§4.7).
