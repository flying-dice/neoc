# `vnd:quick_xml` module

**Experimental**

The **`vnd:quick_xml`** module exposes the [`quick-xml`](https://crates.io/crates/quick-xml) crate for parsing and serialising XML.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/quick_xml.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/quick_xml.rs) |
| Tests | [`tests/vnd/quick_xml.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/quick_xml.test.luau) |
| Stability | Experimental |
| Mirror | [`quick-xml`](https://docs.rs/quick-xml/) |

## Syntax

```lua
local quick_xml = require("vnd:quick_xml")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:quick_xml")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
