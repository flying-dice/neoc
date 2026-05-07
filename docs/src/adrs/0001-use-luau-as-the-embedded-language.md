---
status: Accepted
date: 2026-05-03
---

# 0001. Use Luau as the embedded scripting language

Date: 2026-05-03

## Status

Accepted

## Context

`neoc` needs an embedded scripting language for user-supplied scripts running inside the host runtime. The language must run in a host that controls the execution environment tightly, must permit fine-grained sandboxing of the operating-system surface exposed to scripts, and must not require the host to ship a JIT or a full ECMAScript runtime.

Several candidates were available:

- **Lua 5.4** — small, well-documented, widely embedded. Has the standard library that `os`, `io`, `package`, and `debug` modules expose, much of which is unsafe in a multi-tenant runtime and would need to be stripped.
- **Luau** — Roblox's dialect of Lua. Sandbox-aware by design (the language explicitly drops or restricts unsafe surface), gradual typing, deterministic execution. Implemented in C++ and bindable from Rust through the [`mlua`](https://docs.rs/mlua/) crate's `luau` feature.
- **JavaScript via Deno or Boa** — large surface, heavyweight runtime, less amenable to embedding into a fixed-resource host.
- **WebAssembly** — capable, but the source-language story is downstream and the tooling burden on script authors is high.
- **Custom DSL** — too narrow; user scripts need general-purpose expressivity.

The host runtime is written in Rust. The bindings story for the embedded language is a load-bearing concern; the choice must integrate cleanly with `mlua` or an equivalent.

## Decision

`neoc` uses Luau as its embedded scripting language. The Luau interpreter is bundled into the binary through `mlua`'s `vendored` feature.

## Consequences

- Scripts benefit from Luau's gradual type system and its sandbox-aware design. The language itself drops parts of the Lua standard library that are unsafe for embedding (notably `dofile`, `loadfile`, and parts of `package`), reducing the surface that must be stripped at the host level.
- The host runtime can rely on Luau's deterministic execution semantics — no JIT, predictable allocation behaviour — when reasoning about embedded scripts.
- Script authors must use Luau syntax and semantics rather than vanilla Lua. Most Lua libraries written for 5.1/5.4 are compatible with adaptation; a small number that depend on stripped surface or on Lua-specific features (such as integer/number distinction) require porting.
- The choice of `mlua` as the binding layer gates this decision on the upstream maintainer's continued Luau support. Migration to a different binding layer would be possible but would require non-trivial work.
- The `mlua` `vendored` feature bundles Luau into the neoc binary, so the host does not depend on a system Lua installation. This simplifies distribution at the cost of a larger binary.
