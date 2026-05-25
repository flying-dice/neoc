--!strict
--
-- Type definitions for `lib:hex` — paired with `src/lua/lib/hex.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Surface
--
-- Three flat functions (stateless module — no UserData):
--   encode        bytes -> lowercase hex string
--   encode_upper  bytes -> uppercase hex string
--   decode        hex string -> bytes; case-insensitive, whitespace-tolerant
--
-- `decode` returns `(string?, string?)` — the decoded bytes on success or
-- `(nil, "hex.decode: ...")` on odd-length or non-hex input.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly. Don't remove
-- the `declare` lines to "fix" the squiggles — they're load-bearing.

--[=[
    The module table returned by `require("lib:hex")`.
]=]
export type HexModule = {
    --[=[
        Encode `input` bytes to a lowercase hex string — two characters per
        byte, drawn from `0`–`9` and `a`–`f`.

        ```lua
        local hex = require("lib:hex")
        hex.encode("hi")  -- "6869"
        ```
    ]=]
    encode: (input: string) -> string,

    --[=[
        Encode `input` bytes to an **uppercase** hex string (`0`–`9` and
        `A`–`F`). Same bytes as `encode`, upper-cased — matches fixtures and
        APIs that emit uppercase (e.g. .NET's `Convert.ToHexString`).
    ]=]
    encode_upper: (input: string) -> string,

    --[=[
        Decode a hex string back to raw bytes.

        Case-insensitive and tolerant of ASCII whitespace, so space- or
        newline-grouped dumps (`"de ad be ef"`) decode without pre-cleaning.
        Strict otherwise: returns `(nil, "hex.decode: ...")` on odd-length
        input or any non-hex, non-whitespace character.

        ```lua
        local bytes, err = hex.decode("6869")  -- "hi", nil
        local bytes, err = hex.decode("abc")   -- nil, "hex.decode: Odd number of digits"
        local bytes, err = hex.decode("zz")    -- nil, "hex.decode: Invalid character 'z' at position 0"
        ```
    ]=]
    decode: (input: string) -> (string?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local hex = require("lib:hex")` resolves `hex` to `HexModule`.
declare function require(name: "lib:hex"): HexModule
