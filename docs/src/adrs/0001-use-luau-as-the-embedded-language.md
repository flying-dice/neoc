---
status: Accepted
date: 2026-05-03
---

# 0001. Use Luau as the embedded scripting language

Date: 2026-05-03

## Status

Accepted

## Context

Need: embedded scripting language with tight host control, fine-grained sandboxing, no JIT requirement.

Candidates:

- **Lua 5.4** — small, widely embedded. Unsafe `os`/`io`/`package`/`debug` surface needs stripping.
- **Luau** — Roblox's Lua dialect. Sandbox-aware by design, gradual typing, deterministic. Bindable from Rust via [`mlua`](https://docs.rs/mlua/) `luau` feature.
- **JavaScript (Deno/Boa)** — heavyweight, less amenable to fixed-resource embedding.
- **WebAssembly** — capable but high tooling burden on script authors.
- **Custom DSL** — too narrow for general-purpose scripts.

Host is Rust. Bindings story via `mlua` is load-bearing.

## Decision

`neoc` uses Luau, bundled via `mlua`'s `vendored` feature.

## Consequences

- Luau's gradual type system and sandbox-aware design reduce host-level stripping work.
- Deterministic execution (no JIT) — predictable allocation behaviour.
- Script authors use Luau syntax. Most Lua 5.1/5.4 libraries compatible with adaptation.
- Gated on `mlua` upstream's continued Luau support.
- `vendored` bundles Luau into binary — no system Lua dependency, larger binary.
