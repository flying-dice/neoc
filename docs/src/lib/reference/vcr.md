# `lib:vcr` module

**Experimental**

The **`lib:vcr`** module records and replays HTTP request and response fixtures, providing deterministic tests for code that calls external services.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/vcr.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/vcr.rs) |
| Tests | [`tests/lib/vcr.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/vcr.test.luau) |
| Stability | Experimental |
| Mirror | — |

## Syntax

```lua
local vcr = require("lib:vcr")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("lib:vcr")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `lib:*` modules fit into the engine.
- [`lib:*`](/lib/) — Other modules in this namespace.
