# `std:io` module

**Experimental**

The **`std:io`** module exposes standard input and output handles — `print`, `println`, `eprintln`, and friends.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/io.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/io.rs) |
| Tests | [`tests/std/io.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/std/io.test.luau) |
| Stability | Experimental |
| Mirror | [`std::io`](https://doc.rust-lang.org/std/io/) |

## Syntax

```lua
local io = require("std:io")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("std:io")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `std:*` modules fit into the engine.
- [`std:*`](/std/) — Other modules in this namespace.
