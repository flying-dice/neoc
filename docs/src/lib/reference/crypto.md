# `lib:crypto`

**Experimental**

Authenticated symmetric encryption (AEAD) — AES-128-GCM, AES-256-GCM, ChaCha20-Poly1305.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/crypto.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/crypto.rs) |
| Tests | [`tests/lib/crypto.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/crypto.test.luau) |
| Stability | Experimental |
| Mirror | Node `crypto.createCipheriv`; Go `crypto/cipher` (GCM); Java `Cipher` (`AES/GCM/NoPadding`); .NET `AesGcm` / `ChaCha20Poly1305` |

## Syntax

```lua
local crypto = require("lib:crypto")
```

## Description

> [!WARNING]
> **Stub.** Full surface in module source; contract exercised by test file.

Reach for `lib:crypto` to protect data at rest or in transit between trusted
components — JWE payloads, encrypted columns, message envelopes, replay-proof
tokens. Surface is AEAD-only by design: every supported algorithm produces a
ciphertext that is also a MAC, and `decrypt` errors rather than returning
plaintext if the tag fails to verify. There is no surface for raw CBC/CTR; the
module exists in part to keep those out of scripts.

> [!CAUTION]
> Never reuse a nonce under the same key. GCM and ChaCha20-Poly1305 both
> catastrophically lose authenticity (and partial confidentiality) on a single
> nonce-key reuse. Generate fresh random nonces:
>
> ```lua
> local rng    = require("lib:random")
> local crypto = require("lib:crypto")
>
> local nonce = rng.random_bytes(crypto.nonce_size("aes_256_gcm"))
> local ct    = assert(crypto.encrypt("aes_256_gcm", key, nonce, payload, header))
> ```

The nonce is passed alongside the ciphertext when storing or transmitting —
it isn't a secret. Bind the message header to the ciphertext through the
optional `aad` argument so a tampered header invalidates the tag.

## See also

- [`lib:random`](/lib/reference/random) — the CSPRNG that should produce every nonce and key
- [`lib:hex`](/lib/reference/hex), [`lib:base64`](/lib/reference/base64) — for transport-safe encoding of ciphertext and nonce
- [The module system](/guides/module-system)
- [`lib:*`](/lib/)
