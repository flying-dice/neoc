# `lib:random`

**Experimental**

Cryptographically secure random bytes, ints, strings, tokens, and UUIDs (v4 / v7).

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/random.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/random.rs) |
| Tests | [`tests/lib/random.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/random.test.luau) |
| Stability | Experimental |
| Mirror | Python `secrets` + `uuid`; Node `crypto` |

## Syntax

```lua
local rng = require("lib:random")
```

## Description

> [!WARNING]
> **Stub.** Full surface in module source; contract exercised by test file.

Reach for `lib:random` whenever the value needs to be unpredictable to an
attacker — tokens, salts, IDs, nonces. Luau's built-in `math.random` is a
seedable PRNG and is unfit for any of those uses.

Use `uuid_v7` for database primary keys (timestamp-ordered → cheap B-tree
inserts). Use `uuid_v4` when ordering would leak information.

## See also

- [The module system](/guides/module-system)
- [`lib:*`](/lib/)
