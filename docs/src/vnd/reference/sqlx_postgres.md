# `vnd:sqlx_postgres` module

**Experimental**

The **`vnd:sqlx_postgres`** module exposes the PostgreSQL driver from the [`sqlx`](https://crates.io/crates/sqlx) async SQL toolkit.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/sqlx_postgres.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/sqlx_postgres.rs) |
| Tests | Not in CI (requires Docker / containers) |
| Stability | Experimental |
| Mirror | [`sqlx`](https://docs.rs/sqlx/) (`postgres` feature) |

## Syntax

```lua
local sqlx_postgres = require("vnd:sqlx_postgres")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:sqlx_postgres")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
