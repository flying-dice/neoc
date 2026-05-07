# `vnd:toml` module

**Experimental**

The **`vnd:toml`** module exposes the [`toml`](https://crates.io/crates/toml) crate for parsing and serialising TOML.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/toml.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/toml.rs) |
| Tests | [`tests/vnd/toml.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/toml.test.luau) |
| Stability | Experimental |
| Mirror | [`toml`](https://docs.rs/toml/) |

## Syntax

```lua
local toml = require("vnd:toml")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:toml")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
