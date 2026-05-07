# `vnd:rusqlite` module

**Experimental**

The **`vnd:rusqlite`** module exposes the [`rusqlite`](https://crates.io/crates/rusqlite) SQLite driver. The SQLite library itself is bundled into the binary.

| Property | Value |
|---|---|
| Namespace | `vnd` |
| Source | [`src/lua/vnd/rusqlite.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/vnd/rusqlite.rs) |
| Tests | [`tests/vnd/rusqlite.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/vnd/rusqlite.test.luau) |
| Stability | Experimental |
| Mirror | [`rusqlite`](https://docs.rs/rusqlite/) |

## Syntax

```lua
local rusqlite = require("vnd:rusqlite")
```

## Description

> [!WARNING]
> This reference page is a **stub**. The full Lua surface is captured in the module source listed above and exercised by the test file — until this page is fleshed out, those are the contract.

## See also

- [The module system](/guides/module-system) — How `require("vnd:rusqlite")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — How `vnd:*` modules fit into the engine.
- [`vnd:*`](/vnd/) — Other modules in this namespace.
