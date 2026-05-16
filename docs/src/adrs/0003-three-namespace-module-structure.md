---
status: Accepted
date: 2026-05-03
---

# 0003. Three-namespace module structure (`std`, `lib`, `vnd`)

Date: 2026-05-03

## Status

Accepted

## Context

Modules reintroduce capabilities (ADR 0002). Need organisational discipline so authors reason about rules per module.

Patterns considered:
- **Flat** (`require("fs")`) — conflates origins, naming collisions with crates.
- **Single-prefix** (`require("rb:fs")`) — no design-rule signal.
- **Multi-namespace** — communicates intent at call site, different rules per namespace.

Three kinds of capability with different design rules:
1. Rust std mirrors (should look like Rust)
2. Project-native (own design lineage)
3. Vendored crates (reach upstream docs directly)

## Decision

Three namespaces, picked at registration:

`std:*`
:   Mirrors `std::*`. Names track Rust module names. Small exceptions for broadly-needed capabilities with no `std::` equivalent (e.g. `std:workers`).

`lib:*`
:   Gap-fillers and project-native. Qualifies when: well-defined, stable, broadly useful.

`vnd:*`
:   Vendored crates. One module = one crate. Naming per [ADR 0004](/adrs/0004-vnd-module-names-match-upstream).

Each module belongs to exactly one namespace, immutable after registration.

## Consequences

- Namespace communicates rules. Readers know what to expect from `std:fs` vs `vnd:hyper` vs `lib:test`.
- No naming collisions between project modules and vendored crates.
- Promoting across namespaces is a breaking change — rewards careful initial placement.
- Docs organised by namespace, mirroring `require` paths.
