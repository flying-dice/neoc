# `vnd:serde_json` module

**Experimental**

The **`vnd:serde_json`** module exposes the [`serde_json`](https://crates.io/crates/serde_json) crate for parsing and serialising JSON. Parsed documents are returned as [`lib:json`](/lib/reference/json) Object and Array handles.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/serde_json.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/serde_json.rs) |
| Tests | [`tests/vnd/serde_json.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/serde_json.test.luau) |
| Stability | Experimental |
| Mirror | [`serde_json`](https://docs.rs/serde_json/) |

## Syntax

```lua
local serde_json = require("vnd:serde_json")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:serde_json")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
