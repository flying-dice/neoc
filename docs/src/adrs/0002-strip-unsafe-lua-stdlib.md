---
status: Accepted
date: 2026-05-03
---

# 0002. Strip the unsafe halves of the Lua standard library

Date: 2026-05-03

## Status

Accepted

## Context

Lua and Luau ship with a standard library that exposes operating-system surface to scripts: `io.open` reads and writes arbitrary files, `os.execute` spawns subprocesses, `package.loadlib` loads shared libraries, `debug` reaches into running coroutines and the registry. A general-purpose host runtime that simply embeds the language and exposes those modules inherits whatever the script chooses to do with them — file system access, process control, code injection — without the host having a say.

`neoc` is intended to run user-supplied scripts in a controlled environment. Scripts must be able to perform real work — read files, open sockets, parse JSON, talk to databases — but only through capabilities the host has explicitly granted. The default must be deny.

## Decision

The unsafe halves of the Lua standard library are stripped at engine construction. Scripts running inside `neoc` cannot reach the following surface:

- `io.*` — file and stream I/O is reintroduced through `std:fs` and `std:io`.
- `os.*` — process and time control is reintroduced through `std:thread`, `std:net`, and the worker model.
- `package.*` — the `require` resolver is replaced; see [ADR 0005](/adrs/0005-tuple-error-convention).
- `debug.*` — reflection over running coroutines, the registry, and upvalues is unavailable.
- `loadfile`, `dofile` — file-driven script loading is replaced by the binary's argument-driven model.

The remaining standard surface (`string`, `table`, `math`, `coroutine`, `bit32`, `utf8`, the basic `print` family) is available unchanged.

Every capability scripts need is reintroduced through explicit modules under the `std:*`, `lib:*`, and `vnd:*` namespaces (see [ADR 0003](/adrs/0003-three-namespace-module-structure)). Scripts cannot reach a capability the host has not registered.

## Consequences

- Scripts cannot perform an unsanctioned operation on the host. The capability surface is exactly what the host registers — no more.
- Existing Lua and Luau libraries that depend on stripped surface require porting before they can run inside `neoc`. The cost falls on the script author, not the host.
- The host bears the burden of reintroducing capabilities through curated modules. This is intentional: every capability is reviewed at registration time, errors are routed through documented surfaces, and the catalogue of what is available is finite and discoverable.
- The strict default makes `neoc` unsuitable as a drop-in replacement for environments where unrestricted Lua is expected — for example, an interactive REPL or a generic Lua scripting host. That is by design.
