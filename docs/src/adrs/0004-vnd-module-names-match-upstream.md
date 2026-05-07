---
status: Accepted
date: 2026-05-03
---

# 0004. `vnd:*` module names match upstream crate names verbatim

Date: 2026-05-03

## Status

Accepted

## Context

The `vnd:*` namespace exposes individual Rust crates to Lua ([ADR 0003](/adrs/0003-three-namespace-module-structure)). Each `vnd:*` module wraps exactly one upstream crate. The runtime needs a naming rule for these modules.

Three options were considered:

- **Friendly aliases** — pick names that read well in Lua (`vnd:http` instead of `vnd:hyper`; `vnd:db` instead of `vnd:rusqlite`). Reads cleanly, but the script author cannot move from the Lua call site to the upstream crate's documentation without translation.
- **Match upstream names exactly** — `vnd:hyper` is the `hyper` crate; `vnd:serde_json` is the `serde_json` crate. Reads less smoothly in places (Rust crate naming conventions sometimes differ from Lua's), but eliminates the indirection between the call site and the upstream documentation.
- **Composite names** — merge multiple crates into a single Lua module (`vnd:http` covering both `hyper` and `reqwest`). Hides the underlying choice and complicates the upgrade path.

The host runtime treats vendored crates as load-bearing dependencies; their documentation is the authoritative source for behaviour beyond the thin Lua adapter layer. Anything that obscures the route from the script back to that documentation is a footgun.

## Decision

The module name in the `vnd:*` namespace must match the upstream crate name verbatim. There is no renaming, no aliasing, and no merging of multiple crates into a composite module.

```lua
local hyper      = require("vnd:hyper")        -- the `hyper` crate
local serde_json = require("vnd:serde_json")   -- the `serde_json` crate
local rusqlite   = require("vnd:rusqlite")     -- the `rusqlite` crate
```

Crates whose names contain characters that are awkward in Lua identifiers (hyphens, primarily) substitute underscores: `vnd:quick_xml` for `quick-xml`, `vnd:serde_yaml` for `serde_yaml`. The substitution is mechanical (`-` to `_`) and applied consistently.

When two upstream crates need to coexist for the same conceptual capability — for example, `sqlx` exposed across multiple databases — they appear as separate `vnd:*` modules with names that disambiguate the upstream feature: `vnd:sqlx_mysql` and `vnd:sqlx_postgres`.

## Consequences

- Script authors who recognise an upstream crate know what `vnd:` module to `require`. The reverse is also true: anyone reading `require("vnd:hyper")` knows to consult the [`hyper` crate documentation](https://docs.rs/hyper/) for the underlying behaviour.
- Bumping a `vnd:*` module to a new major version of its upstream crate is straightforward — the module name does not change, and breaking changes propagate to scripts on a known schedule.
- Replacing the upstream crate with a different one is a breaking change. A script that `require`s `vnd:hyper` cannot transparently switch to `vnd:reqwest`; the migration is explicit at every call site. This is a feature, not a bug — the runtime does not lie about which crate it is using.
- Some `vnd:*` names read awkwardly in Lua because Rust crate naming conventions occasionally produce names like `serde_json` or `quick_xml`. The cost is borne at the call site for the benefit of the lookup-to-documentation path.
- The naming rule is non-negotiable. New `vnd:*` modules must follow it; existing names cannot be retroactively prettified.
