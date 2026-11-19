---
status: Accepted
date: 2026-11-19
---

# 0012. Drop the neoc language; fall back to a core Lua runtime

Date: 2026-11-19

## Status

Accepted. Supersedes [ADR 0010](/adrs/0010-independent-codegen-backends) and the compile-target half of [ADR 0011](/adrs/0011-use-luajit-as-the-embedded-runtime). Tracked as NEOC-106.

## Context

neoc was two products in one workspace: a LuaJIT runtime with a curated module surface, and a language that compiled to Lua, with its own spec, parser, type checker, formatter, language server and wasm build. The language carried most of the crates, most of the CI and most of the contributor guidance, and it constrained the runtime: every module needed an `ext` header, and error text had to avoid host-language terms.

The runtime is what agents use. The language did not earn its cost.

## Decision

neoc is a Lua runtime. The `neoc` binary runs Lua (`neoc run`); nothing compiles.

Removed: the compiler crates (`neoc-ast`, `neoc-parser`, `neoc-grammar`, `neoc-types`, `neoc-macros`, `neoc-codegen-lua`, `neoc-compiler`, `neoc-fmt`, `neoc-lsp`, `neoc-wasm`, `neoc-autosql`), the language spec and conformance suite (`lang/`), the language Gherkin specs, the `.neoc` module headers and their emitter, and the `build`, `check`, `fmt`, `lsp`, `autosql` and `emit-headers` commands.

Kept: `neoc-vm` and `neoc` with `run`, globs, `--eval`, `_NEOC_ARGS`, standalone executables (`lib:compile`), `lib:test` and `std:workers`. The VM, its Lua backend and its dependencies are unchanged. So is the facade's recorded metadata (each function's signature and doc): it describes the VM's own API, nothing renders it today, and it is kept so tooling can generate editor types from it.

The last state of the language is preserved under the `neoc-lang-final` tag. Restore a path with `git checkout neoc-lang-final -- <path>`.

## Consequences

- **Easier:** two crates, one binary, a smaller CI. Module authors write Rust and a Lua test; no header to keep in sync.
- **Harder:** nothing renders the module surface for tooling yet. The facade still records it, so editor types can be generated from that record without touching the call sites.
- **Lost:** compile-time types and the formatter. Scripts are plain Lua 5.1 (LuaJIT).
