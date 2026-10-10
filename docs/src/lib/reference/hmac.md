# `lib:hmac`

**Experimental**

Keyed-hash message authentication codes — HMAC over SHA-256 / SHA-384 / SHA-512.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/hmac.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/hmac.rs) |
| Tests | [`tests/lib/hmac.test.lua`](https://github.com/flying-dice/neoc/blob/main/tests/lib/hmac.test.lua) |
| Stability | Experimental |
| Mirror | Python `hmac`; Node `crypto.createHmac`; Go `crypto/hmac` |

## Syntax

```lua
local hmac = require("lib:hmac")
```

## Description

> [!WARNING]
> **Stub.** Full surface in module source; contract exercised by test file.

Reach for `lib:hmac` to authenticate a message under a shared secret — signed
webhooks (GitHub, Stripe), AWS SigV4, JWT `HS*`, OAuth1. Mirrors `lib:hash`:
`Hmac.new(algo, key)` → `:update(bytes)` → `:digest()`, with a one-shot
`hmac(algo, key, bytes)` helper. Tags are raw bytes — encode with `lib:hex` or
`lib:base64`.

> [!CAUTION]
> Verify tags with `hmac.verify(expected, received)`, **never** `==`. A plain
> `==` returns as soon as two bytes differ, and that timing difference lets an
> attacker recover a valid tag one byte at a time. `verify` compares in
> constant time.

```lua
local expected = assert(hmac.hmac("sha256", secret, body))
if not hmac.verify(expected, received_tag) then
    error("bad signature")
end
```

## See also

- [`lib:hash`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/hash.rs) — the underlying digests
- [The module system](/guides/module-system)
- [`lib:*`](/lib/)
