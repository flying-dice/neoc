# `vnd:jsonschema` module

**Experimental**

The **`vnd:jsonschema`** module exposes the [`jsonschema`](https://crates.io/crates/jsonschema) crate for validating JSON documents against a schema.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/jsonschema.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/jsonschema.rs) |
| Tests | [`tests/vnd/jsonschema.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/jsonschema.test.luau) |
| Stability | Experimental |
| Mirror | [`jsonschema`](https://docs.rs/jsonschema/) |

## Syntax

```lua
local jsonschema = require("vnd:jsonschema")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:jsonschema")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
