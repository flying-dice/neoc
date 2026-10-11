---
status: Accepted
date: 2026-10-10
---

# 0011. Use LuaJIT as the embedded runtime and sole compile target

Date: 2026-10-10

## Status

Accepted. Supersedes [ADR 0001](/adrs/0001-use-luau-as-the-embedded-language).

## Context

ADR 0001 chose Luau as the embedded language. The compiler since grew a second concern: which language neoc emits. Luau as runtime plus a typed Luau emit target plus a plain Lua 5.1 base left two output dialects and a downgrade step between them. Authors and agents could pick the wrong one.

Facts:

- The Lua 5.1 emitter (`neoc-codegen-lua`) already existed and defines the lowering in `lang/DYNAMIC.md`.
- LuaJIT runs Lua 5.1 directly and is bindable from Rust via [`mlua`](https://docs.rs/mlua/) `luajit`.
- LuaJIT is a mature, widely embedded runtime.

## Decision

`neoc` embeds LuaJIT through `mlua`'s `luajit` feature. `lua` (Lua 5.1, `.lua`) is the only compile target. One runtime, one target.

The sandbox amends [ADR 0002](/adrs/0002-strip-unsafe-lua-stdlib) for LuaJIT; 0002 holds the strip list.

Conformance run-trials execute in-process on the same LuaJIT sandbox.

## Consequences

- One runtime and one target: nothing to choose, so nothing to choose wrongly.
- No typed output. Declared neoc types are enforced by runtime checks (`lang/DYNAMIC.md` §9), not by an emitted type layer.
- Numbers are doubles only, unchanged from ADR 0001. There is no native `i64`; integers beyond 2^53 lose precision.
- No `buffer` type. Binary data is Lua strings.
- `ffi`, `jit`, and `debug` are unavailable to scripts; the library set is an allow-list; base globals are a deny-list (`STRIPPED_GLOBALS`).
- Gated on `mlua` upstream's continued LuaJIT support.
- `vendored` builds LuaJIT into the binary: no system Lua dependency; a C toolchain is required at build time.
