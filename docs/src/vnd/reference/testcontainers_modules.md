# `vnd:testcontainers_modules` module

**Experimental**

The **`vnd:testcontainers_modules`** module exposes preset container images from [`testcontainers-modules`](https://crates.io/crates/testcontainers-modules) for use in integration tests.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/testcontainers_modules.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/testcontainers_modules.rs) |
| Tests | Not in CI (requires Docker / containers) |
| Stability | Experimental |
| Mirror | [`testcontainers-modules`](https://docs.rs/testcontainers-modules/) |

## Syntax

```lua
local testcontainers_modules = require("vnd:testcontainers_modules")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:testcontainers_modules")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
