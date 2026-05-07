# `lib:base64` module

**Experimental**

The **`lib:base64`** module encodes and decodes Base64, including the URL-safe variant.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/base64.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/base64.rs) |
| Tests | [`tests/lib/base64.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/base64.test.luau) |
| Stability | Experimental |
| Mirror | [RFC 4648](https://datatracker.ietf.org/doc/html/rfc4648) |

## Syntax

```lua
local base64 = require("lib:base64")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("lib:base64")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `lib:*` modules fit into the engine.
- [`lib:*`](/lib/) — Other modules in this namespace.
