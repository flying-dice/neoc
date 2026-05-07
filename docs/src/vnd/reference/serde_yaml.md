# `vnd:serde_yaml` module

**Experimental**

The **`vnd:serde_yaml`** module exposes the [`serde_yaml`](https://crates.io/crates/serde_yaml) crate for parsing and serialising YAML.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/serde_yaml.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/serde_yaml.rs) |
| Tests | [`tests/vnd/serde_yaml.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/serde_yaml.test.luau) |
| Stability | Experimental |
| Mirror | [`serde_yaml`](https://docs.rs/serde_yaml/) |

## Syntax

```lua
local serde_yaml = require("vnd:serde_yaml")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:serde_yaml")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
