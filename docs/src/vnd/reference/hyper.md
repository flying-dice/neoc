# `vnd:hyper` module

**Experimental**

The **`vnd:hyper`** module exposes the [`hyper`](https://crates.io/crates/hyper) HTTP client and server crate to Lua.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/hyper.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/hyper.rs) |
| Tests | [`tests/vnd/hyper.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/hyper.test.luau) |
| Stability | Experimental |
| Mirror | [`hyper`](https://docs.rs/hyper/) |

## Syntax

```lua
local hyper = require("vnd:hyper")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:hyper")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
