---
status: Accepted
date: 2026-05-03
---

# 0002. Strip the unsafe halves of the Lua standard library

Date: 2026-05-03

## Status

Accepted

## Context

Lua/Luau ships with OS surface: `io.open`, `os.execute`, `package.loadlib`, `debug`. Embedding without stripping inherits whatever scripts choose to do. neoc runs user scripts in a controlled environment — capabilities must be explicitly granted. Default: deny.

## Decision

Unsafe standard library stripped at engine construction:

- `io.*` → reintroduced via `std:fs` and `std:io`
- `os.*` → via `std:thread`, `std:net`, worker model
- `package.*` → `require` replaced (see [ADR 0003](/adrs/0003-three-namespace-module-structure))
- `debug.*` → unavailable (unsafe in multi-tenant runtime)
- `loadfile`, `dofile` → replaced by argument-driven model

Remaining surface (`string`, `table`, `math`, `coroutine`, `bit32`, `utf8`, `print` family) unchanged.

Capabilities reintroduced through `std:*`, `lib:*`, `vnd:*` modules. Scripts cannot reach unregistered capabilities.

## Consequences

- Scripts cannot perform unsanctioned operations. Surface is exactly what host registers.
- Existing Lua libraries depending on stripped surface require porting.
- Host bears burden of curating capability modules — intentional.
- neoc unsuitable as drop-in for unrestricted Lua environments — by design.
