# `std:collections` module

**Experimental**

The **`std:collections`** module exposes concurrent and shareable collection types — `map`, `counter`, and `mutex`.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/collections.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/collections.rs) |
| Tests | [`tests/std/collections.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/std/collections.test.luau) |
| Stability | Experimental |
| Mirror | [`std::collections`](https://doc.rust-lang.org/std/collections/) |

## Syntax

```lua
local collections = require("std:collections")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("std:collections")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `std:*` modules fit into the engine.
- [`std:*`](/std/) — Other modules in this namespace.
