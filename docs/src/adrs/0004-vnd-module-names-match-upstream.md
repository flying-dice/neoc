---
status: Accepted
date: 2026-05-03
---

# 0004. `vnd:*` module names match upstream crate names verbatim

Date: 2026-05-03

## Status

Accepted

## Context

`vnd:*` wraps individual Rust crates (ADR 0003). Need a naming rule.

Options:
- **Friendly aliases** (`vnd:http` for `hyper`) — breaks lookup-to-docs path.
- **Match upstream exactly** — eliminates indirection to upstream documentation.
- **Composite** (merge crates) — hides choices, complicates upgrades.

Upstream docs are authoritative for behaviour beyond the Lua adapter. Obscuring the route from script to docs is a footgun.

## Decision

Module name matches crate name verbatim. No renaming, no aliasing, no merging.

```lua
local hyper      = require("vnd:hyper")
local serde_json = require("vnd:serde_json")
local rusqlite   = require("vnd:rusqlite")
```

Hyphen → underscore substitution: `vnd:quick_xml` for `quick-xml`. Mechanical, consistent.

Multi-database crates disambiguate by feature: `vnd:sqlx_mysql`, `vnd:sqlx_postgres`.

## Consequences

- Script authors know upstream crate from `require` name. Reverse also true.
- Major version bumps straightforward — name unchanged.
- Replacing upstream crate is an explicit breaking change at every call site. Feature, not bug.
- Some names read awkwardly in Lua (`serde_json`, `quick_xml`). Cost at call site, benefit at lookup.
- Non-negotiable. No retroactive prettification.
