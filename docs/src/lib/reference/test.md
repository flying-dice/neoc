# `lib:test` module

**Experimental**

The **`lib:test`** module is the Luau test runner. It registers test cases, runs them, and reports pass/fail with assertion details.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/test.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/test.rs) |
| Tests | [`tests/lib/test.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/test.test.luau) |
| Stability | Experimental |
| Mirror | — |

## Syntax

```lua
local test = require("lib:test")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("lib:test")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `lib:*` modules fit into the engine.
- [`lib:*`](/lib/) — Other modules in this namespace.
