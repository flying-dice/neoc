--!strict
--
-- Type definitions for `lib:random` — paired with `src/lua/lib/random.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## What this is
--
-- The CSPRNG side of randomness. All bytes/ints/strings draw from the OS
-- getrandom syscall (via the `rand` crate's `OsRng`); UUIDs use the `uuid`
-- crate's `v4` and `v7` generators which themselves source from the same.
--
-- For seedable / reproducible random — use Luau's built-in `math.random`.
-- That module is unsafe for tokens, salts, IDs, and nonces. This module is
-- the one to reach for in any of those cases.
--
-- ## Surface
--
-- Two groups under one module — matches every major runtime split
-- (`secrets` + `uuid` in Python, `crypto` in Node, etc.):
--
-- - CSPRNG: `random_bytes`, `random_int`, `random_string`, `token_urlsafe`
-- - UUIDs: `uuid_v4`, `uuid_v7`, `uuid_parse`, `uuid_format`
--
-- ## random_int is inclusive on both ends
--
-- `random_int(min, max)` returns an integer in `[min, max]` (both ends
-- inclusive), matching Luau's existing `math.random(m, n)` and Python's
-- `random.randint`. Picked over the half-open `[min, max)` convention used
-- by Node `crypto.randomInt` because the Luau audience already reads
-- inclusive semantics from `math.random` — surprising them is the larger
-- footgun.
--
-- ## random_string default alphabet
--
-- When no `alphabet` is passed, `random_string` draws from `[0-9A-Za-z]`
-- (62 chars). Pass a custom alphabet for hex (`"0123456789abcdef"`),
-- base32, or any other character set.
--
-- ## UUIDs return canonical strings
--
-- `uuid_v4()` and `uuid_v7()` return the 8-4-4-4-12 lowercase canonical
-- form (e.g. `550e8400-e29b-41d4-a716-446655440000`). Convert to raw 16
-- bytes via `uuid_parse`; convert back via `uuid_format`.
--
-- Prefer `uuid_v7` for database primary keys — its timestamp prefix makes
-- generated ids monotonically ordered, which keeps B-tree inserts cheap.
-- Reach for `uuid_v4` when ordering would leak information (anti-enumeration).
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly. Don't
-- remove the `declare` lines to "fix" the squiggles.

--[=[
    The module table returned by `require("lib:random")`.
]=]
export type RandomModule = {
    --[=[
        Return `n` cryptographically secure random bytes as a Luau string.
        Backed by the OS getrandom syscall.

        Returns `(string, nil)` on success; `(nil, "random.random_bytes: ...")`
        if `n` is negative. `n == 0` returns the empty string.

        ```lua
        local rng = require("lib:random")
        local salt = assert(rng.random_bytes(16))
        ```
    ]=]
    random_bytes: (n: number) -> (string?, string?),

    --[=[
        Return a uniformly distributed integer in the closed interval
        `[min, max]`. Both ends inclusive, matching `math.random(m, n)`.

        Backed by OS getrandom with rejection sampling to eliminate modulo
        bias.

        Returns `(number, nil)` on success; `(nil, "random.random_int: ...")`
        if `min > max`.

        ```lua
        local die = assert(rng.random_int(1, 6))       -- 1..6 inclusive
        local idx = assert(rng.random_int(0, #t - 1))  -- 0-based index
        ```
    ]=]
    random_int: (min: number, max: number) -> (number?, string?),

    --[=[
        Return a string of `n` characters drawn uniformly from `alphabet`.
        If `alphabet` is omitted, defaults to `[0-9A-Za-z]` (62 chars).

        Returns `(string, nil)` on success; `(nil, "random.random_string: ...")`
        if `n < 0` or `alphabet` is the empty string.

        ```lua
        local session = assert(rng.random_string(24))           -- alphanumeric
        local hex     = assert(rng.random_string(32, "0123456789abcdef"))
        ```
    ]=]
    random_string: (n: number, alphabet: string?) -> (string?, string?),

    --[=[
        Return a URL-safe base64-encoded random token derived from `n_bytes`
        of CSPRNG output. Equivalent to Python's `secrets.token_urlsafe(n)`.

        The returned string has no `=` padding and uses the URL-safe
        alphabet (`A-Z`, `a-z`, `0-9`, `-`, `_`) — safe to drop into URLs,
        headers, and filenames without escaping.

        Length is `ceil(n_bytes * 4 / 3)` characters.

        Returns `(string, nil)` on success; `(nil, "random.token_urlsafe: ...")`
        if `n_bytes < 0`.

        ```lua
        local api_key = assert(rng.token_urlsafe(32))  -- 43-char token
        ```
    ]=]
    token_urlsafe: (n_bytes: number) -> (string?, string?),

    --[=[
        Generate a random (v4) UUID in canonical 8-4-4-4-12 lowercase form.

        Reach for v4 when the id must reveal nothing about when or in what
        order it was created. For database primary keys or anywhere insert
        ordering matters, prefer `uuid_v7`.

        ```lua
        local id = rng.uuid_v4()  -- "550e8400-e29b-41d4-a716-446655440000"
        ```
    ]=]
    uuid_v4: () -> string,

    --[=[
        Generate a v7 (unix-ms timestamp ordered) UUID in canonical form.

        v7 ids are monotonically increasing when sorted lexicographically —
        ideal for database primary keys, log correlation, and anywhere
        chronological order is useful.

        ```lua
        local id = rng.uuid_v7()
        ```
    ]=]
    uuid_v7: () -> string,

    --[=[
        Parse a canonical UUID string into its 16 raw bytes. Accepts
        lowercase, uppercase, and braced forms.

        Returns `(bytes, nil)` on success; `(nil, "random.uuid_parse: ...")`
        on malformed input.

        ```lua
        local bytes, err = rng.uuid_parse(id)
        ```
    ]=]
    uuid_parse: (input: string) -> (string?, string?),

    --[=[
        Format 16 raw bytes back to a canonical lowercase UUID string.
        Round-trips with `uuid_parse`.

        Returns `(string, nil)` on success; `(nil, "random.uuid_format: ...")`
        if `bytes` is not exactly 16 bytes long.

        ```lua
        local s = assert(rng.uuid_format(bytes))
        ```
    ]=]
    uuid_format: (bytes: string) -> (string?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local rng = require("lib:random")` resolves `rng` to `RandomModule`.
declare function require(name: "lib:random"): RandomModule
