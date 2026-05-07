---
status: Accepted
date: 2026-05-03
---

# 0003. Three-namespace module structure (`std`, `lib`, `vnd`)

Date: 2026-05-03

## Status

Accepted

## Context

`neoc` reintroduces capabilities to scripts through explicit modules ([ADR 0002](/adrs/0002-strip-unsafe-lua-stdlib)). The runtime needs a discipline for organising those modules so that script authors can reason about what each one is for, what rules it follows, and where to look when they need a particular capability.

Several patterns were considered:

- **Flat module space** — every module lives at the top level (`require("fs")`, `require("hyper")`). Simple, but conflates project-native modules with vendored crates and standard-library mirrors. Naming collisions with upstream crate names become a hazard.
- **Single-prefix space** — every module under one prefix (`require("rb:fs")`, `require("rb:hyper")`). Solves naming collisions but does not communicate which modules mirror upstream conventions and which are project-native.
- **Multi-namespace** — three or more namespaces, each carrying a discipline. Communicates intent at the call site; allows different modules to be governed by different rules.

The runtime exposes three kinds of capability that have meaningfully different design rules:

1. Capabilities that mirror Rust's standard library — filesystem, networking, threading, collections, I/O. These should *look like* Rust's standard library to a reader who knows it.
2. Capabilities that are project-native — JSON document handles, the test runner, base64 encoding. These have their own design lineage.
3. Capabilities that wrap individual upstream crates — HTTP via `hyper`, JSON parsing via `serde_json`, SQLite via `rusqlite`. These should reach the upstream crate's surface and documentation directly.

## Decision

`neoc` uses three namespaces for Lua modules, picked at module registration:

`std:*`
:   Mirrors Rust's standard library (`std::*`). Module names track the standard library's module names where possible (`std:fs` for `std::fs`, `std:net` for `std::net`). A small number of `std:*` modules cover capabilities that have no `std::` equivalent but are broadly needed and have no clean home elsewhere — `std:workers` is the principal example.

`lib:*`
:   Blessed gap-fillers and project-native modules. A module qualifies for `lib:*` when its purpose is well-defined, its surface is stable, and it is genuinely useful to most scripts. Speculative or experimental functionality belongs elsewhere until it has earned a place here.

`vnd:*`
:   Vendored Rust crates exposed to Lua. Each `vnd:*` module wraps exactly one upstream crate. The naming rule for `vnd:*` is governed by [ADR 0004](/adrs/0004-vnd-module-names-match-upstream).

Every module belongs to exactly one namespace, picked at registration time and not changeable after.

## Consequences

- Script authors can reason about the rules a module follows from its namespace alone. `require("std:fs")` is expected to read like Rust's `std::fs`; `require("vnd:hyper")` is expected to expose the `hyper` crate; `require("lib:test")` is expected to be project-native.
- Naming collisions between project-native modules and vendored crates are impossible — they live in different namespaces.
- Adding a new module requires choosing the right namespace. The choice is sometimes contested (a new module might fit `lib:*` or `std:*` depending on how it is shaped); an RFC is the right surface for that conversation.
- Promoting a module across namespaces (for example, from `lib:*` to `std:*`) is a breaking change for scripts that `require` it. This raises the cost of namespace decisions but rewards careful initial placement.
- Documentation is organised by namespace, with one landing page and a `reference/` subdirectory per namespace. Readers locate modules through the same structure that script authors `require` them through.
