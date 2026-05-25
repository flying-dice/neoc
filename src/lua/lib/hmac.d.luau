--!strict
--
-- Type definitions for `lib:hmac` — paired with `src/lua/lib/hmac.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Surface
--
-- Streaming `Hmac` UserData + flat one-shot `hmac` helper, mirroring
-- `lib:hash`:
--   `Hmac.new(algo, key)` → `:update(bytes)` → `:digest()` (Python `hmac`,
--   Node `crypto.createHmac`, Go `crypto/hmac`, Java `Mac`).
-- Plus a constant-time `verify` for comparing tags.
--
-- ## Algorithms
--
-- Names match `lib:hash`'s SHA-2 family. Tag size equals the digest size:
--
-- | Algorithm | Tag size |
-- |-----------|----------|
-- | `sha256`  | 32 bytes |
-- | `sha384`  | 48 bytes |
-- | `sha512`  | 64 bytes |
--
-- Non-SHA2 HMAC (HMAC-SHA1, HMAC-MD5) is intentionally absent — HMAC's
-- security argument assumes a modern PRF. KMAC / Poly1305 / GMAC are separate
-- constructions, not in scope here.
--
-- ## Output is bytes
--
-- `digest()` returns the raw tag — hash, hex, and base64 stay orthogonal so
-- callers chain whichever encoding they need. Hex via `lib:hex`, base64 via
-- `lib:base64`.
--
-- ## Digest is idempotent
--
-- Calling `:digest()` does not consume the MAC. State is cloned before
-- finalising — repeated `:digest()` calls return the same tag, and you can
-- keep `:update`-ing afterwards. Matches `lib:hash`.
--
-- ## Verify in constant time
--
-- Never compare tags with `==` — that leaks a timing side-channel an attacker
-- can use to forge a tag byte by byte. `verify` compares every byte
-- regardless of where the first mismatch falls. A length mismatch (not secret
-- — fixed by the algorithm) returns `false`.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly. Don't remove
-- the `declare` lines to "fix" the squiggles.

export type Algorithm = "sha256" | "sha384" | "sha512"

--[=[
    A reusable streaming MAC. Construct with `hmac.Hmac.new(algo, key)`, feed
    bytes via `:update`, read the running tag via `:digest`.

    Safe to call `:digest()` multiple times and to keep `:update`-ing after a
    digest read.
]=]
export type Hmac = {
    --[=[
        Feed `data` into the running MAC. Returns nothing.

        ```lua
        local m = hmac.Hmac.new("sha256", "secret")
        m:update("hello ")
        m:update("world")
        ```
    ]=]
    update: (self: Hmac, data: string) -> (),

    --[=[
        Return the current tag as raw bytes. Idempotent — does not consume the
        MAC; further `:update` calls extend state and a subsequent `:digest()`
        reflects the new total.
    ]=]
    digest: (self: Hmac) -> string,

    --[=[
        Return the algorithm name this MAC was constructed with.
    ]=]
    algorithm: (self: Hmac) -> Algorithm,
}

export type HmacConstructor = {
    --[=[
        Construct a new streaming MAC over `key`. The key may be any length.

        Returns `(Hmac, nil)` on success; `(nil, "hmac.Hmac.new: ...")` on an
        unknown algorithm.

        ```lua
        local hmac = require("lib:hmac")
        local m = assert(hmac.Hmac.new("sha256", "secret"))
        m:update("hello")
        local tag = m:digest()
        ```
    ]=]
    new: (algo: Algorithm, key: string) -> (Hmac?, string?),
}

--[=[
    The module table returned by `require("lib:hmac")`.
]=]
export type HmacModule = {
    --[=[
        Constructor table for the streaming `Hmac` UserData.
    ]=]
    Hmac: HmacConstructor,

    --[=[
        One-shot tag of `data` under `algo` and `key`. Equivalent to
        `Hmac.new(algo, key):update(data):digest()` but skips the UserData
        allocation — preferred for one-off authentication.

        Returns `(tag, nil)` on success; `(nil, "hmac.hmac: ...")` on an
        unknown algorithm.

        ```lua
        local hmac = require("lib:hmac")
        local tag = assert(hmac.hmac("sha256", "secret", "hello"))
        ```
    ]=]
    hmac: (algo: Algorithm, key: string, data: string) -> (string?, string?),

    --[=[
        Compare two tags in constant time. Returns `true` iff they are equal.

        Use this — never `==` — to check a received tag against a recomputed
        one. The comparison does not short-circuit, so it leaks no timing
        information about how many leading bytes matched.

        ```lua
        local expected = hmac.hmac("sha256", key, body)
        if not hmac.verify(expected, received_tag) then
            error("bad signature")
        end
        ```
    ]=]
    verify: (a: string, b: string) -> boolean,

    --[=[
        Return the list of supported algorithm names in canonical order.

        ```lua
        for _, a in ipairs(hmac.algorithms()) do print(a) end
        ```
    ]=]
    algorithms: () -> { Algorithm },
}

-- Literal-string overload — picked up by luau-lsp so
-- `local hmac = require("lib:hmac")` resolves `hmac` to `HmacModule`.
declare function require(name: "lib:hmac"): HmacModule
