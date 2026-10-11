---
status: Accepted
date: 2026-05-03
---

# 0002. Strip the unsafe halves of the Lua standard library

Date: 2026-05-03

## Status

Accepted. Amended by [ADR 0011](/adrs/0011-use-luajit-as-the-embedded-runtime) (2026-10-10): LuaJIT strip list.

## Context

LuaJIT ships with OS surface: `io.open`, `os.execute`, `package.loadlib`, `debug`. Embedding without stripping inherits whatever scripts choose to do. neoc runs user scripts in a controlled environment — capabilities must be explicitly granted. Default: deny.

## Decision

Unsafe standard library stripped at engine construction:

- `io.*` → reintroduced via `std:fs` and `std:io`
- `os.*` → via `std:thread`, `std:net`, worker model
- `package.*` → `require` replaced (see [ADR 0003](/adrs/0003-three-namespace-module-structure))
- `debug.*`, `ffi.*`, `jit.*` → unavailable (never loaded)
- `load`, `loadstring`, `loadfile`, `dofile`, `string.dump` → removed; replaced by argument-driven model
- `getfenv`, `setfenv`, `newproxy`, `module`, `collectgarbage`, `gcinfo` → removed

Remaining surface (`string`, `table`, `math`, `coroutine`, `bit`, `print` family) unchanged.

Capabilities reintroduced through `std:*`, `lib:*`, `vnd:*` modules. Scripts cannot reach unregistered capabilities.

## Consequences

- Scripts cannot perform unsanctioned operations. Surface is exactly what host registers.
- Existing Lua libraries depending on stripped surface require porting.
- Host bears burden of curating capability modules — intentional.
- neoc unsuitable as drop-in for unrestricted Lua environments — by design.
