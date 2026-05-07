--!strict
--
-- Type definitions for `lib:base64` — paired with `src/lua/lib/base64.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Encoding variants
--
-- Four alphabets × two padding options = eight functions:
--   standard         (+/) with padding   encode / decode
--   standard_no_pad  (+/) without pad    encode_no_pad / decode_no_pad
--   url_safe         (-_) with padding   encode_url_safe / decode_url_safe
--   url_safe_no_pad  (-_) without pad    encode_url_safe_no_pad / decode_url_safe_no_pad
--
-- All decode functions return `(string?, string?)` — the decoded bytes on
-- success or `(nil, "base64.decode*: ...")` on failure.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly. Don't remove
-- the `declare` lines to "fix" the squiggles — they're load-bearing.

--[=[
    The module table returned by `require("lib:base64")`.
]=]
export type Base64Module = {
    --[=[
        Encode `input` using the standard alphabet (A–Z, a–z, 0–9, +, /)
        with `=` padding to a multiple of 4 characters.

        ```lua
        local b64 = require("lib:base64")
        b64.encode("hello")  -- "aGVsbG8="
        ```
    ]=]
    encode: (input: string) -> string,

    --[=[
        Decode a standard-alphabet base64 string back to raw bytes.

        Returns `(string, nil)` on success; `(nil, "base64.decode: ...")` on
        invalid input (bad characters, wrong padding).

        ```lua
        local bytes, err = b64.decode("aGVsbG8=")
        ```
    ]=]
    decode: (input: string) -> (string?, string?),

    --[=[
        Encode using the standard alphabet without `=` padding.

        Useful for cookies, HTTP headers, or any context where the trailing
        `=` would need to be percent-encoded or stripped anyway.
    ]=]
    encode_no_pad: (input: string) -> string,

    --[=[
        Decode standard-alphabet base64 that has **no** padding.

        Errors if the input has trailing `=` characters — use `decode` for
        padded input.
    ]=]
    decode_no_pad: (input: string) -> (string?, string?),

    --[=[
        Encode using the URL-safe alphabet (`-` and `_` instead of `+` and
        `/`), with `=` padding. Safe to embed in URLs and filenames without
        percent-encoding.
    ]=]
    encode_url_safe: (input: string) -> string,

    --[=[
        Decode URL-safe base64 with padding.
    ]=]
    decode_url_safe: (input: string) -> (string?, string?),

    --[=[
        Encode using the URL-safe alphabet without padding. This is the
        variant used by most modern token formats (e.g. JWT).
    ]=]
    encode_url_safe_no_pad: (input: string) -> string,

    --[=[
        Decode URL-safe base64 without padding.
    ]=]
    decode_url_safe_no_pad: (input: string) -> (string?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local b64 = require("lib:base64")` resolves `b64` to `Base64Module`.
declare function require(name: "lib:base64"): Base64Module
