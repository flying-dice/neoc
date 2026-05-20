--!strict
--
-- Type definitions for `lib:hash` — paired with `src/lua/lib/hash.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Surface
--
-- Streaming `Hasher` UserData + flat one-shot `hash` helper, matching the
-- pattern every major runtime ships:
--   `Hasher.new(algo)` → `:update(bytes)` → `:digest()` (Python `hashlib`,
--   Node `crypto.createHash`, Java `MessageDigest`, Go `hash.Hash`).
--
-- ## Algorithms
--
-- Names are snake_case matching Python's `hashlib`. Output sizes:
--
-- | Algorithm  | Class            | Digest size |
-- |------------|------------------|-------------|
-- | `sha256`   | SHA-2 / crypto   | 32 bytes    |
-- | `sha384`   | SHA-2 / crypto   | 48 bytes    |
-- | `sha512`   | SHA-2 / crypto   | 64 bytes    |
-- | `sha3_256` | SHA-3 / crypto   | 32 bytes    |
-- | `sha3_512` | SHA-3 / crypto   | 64 bytes    |
-- | `blake3`   | BLAKE3 / crypto  | 32 bytes    |
-- | `xxh3`     | non-crypto       | 8 bytes     |
-- | `xxh64`    | non-crypto       | 8 bytes     |
-- | `md5`      | deprecated       | 16 bytes    |
-- | `sha1`     | deprecated       | 20 bytes    |
--
-- `md5` and `sha1` exist for interop with legacy formats only — never use
-- them for security boundaries. They're not removed because content
-- addressing systems (git, OCI manifests, ETags) still depend on them.
--
-- ## Output is bytes
--
-- `digest()` returns raw bytes — the spec keeps hash, hex, and base64
-- orthogonal so callers chain whichever encoding they actually need.
-- Hex / base64 via `lib:hex` (planned) / `lib:base64`.
--
-- ## Digest is idempotent
--
-- Calling `:digest()` does not consume the hasher. State is cloned before
-- finalising — repeated `:digest()` calls return the same value, and you
-- can keep `:update`-ing afterwards. Matches Python `hashlib`. Avoids the
-- Node/Java footgun where the second `:digest()` throws or returns garbage.
--
-- ## xxhash byte order
--
-- `xxh3` and `xxh64` produce a 64-bit integer; we serialise it big-endian
-- so the bytes match what Python's `xxhash` package returns and what a hex
-- dump would naturally display.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly. Don't
-- remove the `declare` lines to "fix" the squiggles.

export type Algorithm =
    "sha256" | "sha384" | "sha512"
    | "sha3_256" | "sha3_512"
    | "blake3"
    | "xxh3" | "xxh64"
    | "md5" | "sha1"

--[=[
    A reusable streaming hasher. Construct with `hash.Hasher.new(algo)`,
    feed bytes via `:update`, read the running digest via `:digest`.

    Safe to call `:digest()` multiple times and to keep `:update`-ing after
    a digest read.
]=]
export type Hasher = {
    --[=[
        Feed `data` into the running digest. Returns nothing.

        ```lua
        local h = hash.Hasher.new("sha256")
        h:update("hello ")
        h:update("world")
        ```
    ]=]
    update: (self: Hasher, data: string) -> (),

    --[=[
        Return the current digest as raw bytes. Idempotent — does not
        consume the hasher; further `:update` calls extend state and a
        subsequent `:digest()` reflects the new total.
    ]=]
    digest: (self: Hasher) -> string,

    --[=[
        Return the algorithm name this hasher was constructed with.
        Useful when storing hashers alongside other UserData.
    ]=]
    algorithm: (self: Hasher) -> Algorithm,
}

export type HasherConstructor = {
    --[=[
        Construct a new streaming hasher.

        Returns `(Hasher, nil)` on success; `(nil, "hash.Hasher.new: ...")`
        on unknown algorithm.

        ```lua
        local hash = require("lib:hash")
        local h = assert(hash.Hasher.new("sha256"))
        h:update("hello")
        local digest = h:digest()
        ```
    ]=]
    new: (algo: Algorithm) -> (Hasher?, string?),
}

--[=[
    The module table returned by `require("lib:hash")`.
]=]
export type HashModule = {
    --[=[
        Constructor table for the streaming `Hasher` UserData.
    ]=]
    Hasher: HasherConstructor,

    --[=[
        One-shot hash of `data` under `algo`. Equivalent to
        `Hasher.new(algo):update(data):digest()` but skips the UserData
        allocation — preferred for one-off hashing.

        Returns `(bytes, nil)` on success; `(nil, "hash.hash: ...")` on
        unknown algorithm.

        ```lua
        local hash = require("lib:hash")
        local digest = assert(hash.hash("sha256", "hello"))
        ```
    ]=]
    hash: (algo: Algorithm, data: string) -> (string?, string?),

    --[=[
        Return the list of supported algorithm names in canonical order
        (cryptographic first, non-cryptographic next, deprecated last).

        ```lua
        for _, a in ipairs(hash.algorithms()) do print(a) end
        ```
    ]=]
    algorithms: () -> { Algorithm },
}

-- Literal-string overload — picked up by luau-lsp so
-- `local hash = require("lib:hash")` resolves `hash` to `HashModule`.
declare function require(name: "lib:hash"): HashModule
