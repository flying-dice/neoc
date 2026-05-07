# `vnd:sqlx_mysql` module

**Experimental**

The **`vnd:sqlx_mysql`** module exposes the MySQL driver from the [`sqlx`](https://crates.io/crates/sqlx) async SQL toolkit.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/sqlx_mysql.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/sqlx_mysql.rs) |
| Tests | Not in CI (requires Docker / containers) |
| Stability | Experimental |
| Mirror | [`sqlx`](https://docs.rs/sqlx/) (`mysql` feature) |

## Syntax

```lua
local sqlx_mysql = require("vnd:sqlx_mysql")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:sqlx_mysql")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
