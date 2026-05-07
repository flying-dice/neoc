# `std:fs` module

**Experimental**

The **`std:fs`** module mirrors filesystem reads, writes, and metadata operations from Rust's standard library.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/fs.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/fs.rs) |
| Tests | [`tests/std/fs.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/std/fs.test.luau) |
| Stability | Experimental |
| Mirror | [`std::fs`](https://doc.rust-lang.org/std/fs/) |

## Syntax

```lua
local fs = require("std:fs")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("std:fs")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `std:*` modules fit into the engine.
- [`std:*`](/std/) — Other modules in this namespace.
