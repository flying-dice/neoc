# `lib:hex`

**Experimental**

Hex encoding/decoding — bytes to hex string and back.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/hex.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/hex.rs) |
| Tests | [`tests/lib/hex.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/hex.test.luau) |
| Stability | Experimental |
| Mirror | Rust `hex` crate; Python `bytes.hex()` / `bytes.fromhex()` |

## Syntax

```lua
local hex = require("lib:hex")
```

## Description

> [!WARNING]
> **Stub.** Full surface in module source; contract exercised by test file.

`encode` is lowercase, `encode_upper` is uppercase. `decode` is
case-insensitive and skips ASCII whitespace — `"de ad be ef"` decodes without
pre-cleaning — then strict: odd-length or non-hex input returns `(nil, err)`.

## See also

- [The module system](/guides/module-system)
- [`lib:base64`](/lib/reference/base64) — the sibling encoding module
- [`lib:*`](/lib/)
