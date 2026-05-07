# `std:thread` module

**Experimental**

The **`std:thread`** module exposes thread-local primitives and the `sleep` function.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/thread.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/thread.rs) |
| Tests | [`tests/std/thread.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/std/thread.test.luau) |
| Stability | Experimental |
| Mirror | [`std::thread`](https://doc.rust-lang.org/std/thread/) |

## Syntax

```lua
local thread = require("std:thread")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("std:thread")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `std:*` modules fit into the engine.
- [`std:*`](/std/) — Other modules in this namespace.
