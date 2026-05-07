# `std:net` module

**Experimental**

The **`std:net`** module exposes TCP listeners and connected sockets, mirroring Rust's networking primitives.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/net.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/net.rs) |
| Tests | [`tests/std/net.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/std/net.test.luau) |
| Stability | Experimental |
| Mirror | [`std::net`](https://doc.rust-lang.org/std/net/) |

## Syntax

```lua
local net = require("std:net")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("std:net")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `std:*` modules fit into the engine.
- [`std:*`](/std/) — Other modules in this namespace.
