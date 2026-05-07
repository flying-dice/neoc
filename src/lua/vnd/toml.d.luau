--!strict
--
-- Type definitions for `vnd:toml` — paired with `src/lua/vnd/toml.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## TOML-specific notes
--
-- TOML has no streaming parser — `decode` reads the reader handle to
-- completion before parsing. The root value passed to `encode*` must be a
-- table (TOML documents are always objects at the root).
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    The module table returned by `require("vnd:toml")`.
]=]
export type TomlModule = {
    --[=[
        Parse TOML from a `ReaderHandle`. Reads the handle to completion first,
        then parses (TOML has no streaming API).

        Returns `(value, nil)` on success; `(nil, "toml.decode: ...")` on
        error (not a reader handle, invalid UTF-8, or invalid TOML).

        ```lua
        local fs   = require("std:fs")
        local toml = require("vnd:toml")
        local r    = fs.open({ path = "config.toml", mode = "r" })
        local cfg, err = toml.decode(r)
        ```
    ]=]
    decode: (reader: any) -> (any, string?),

    --[=[
        Parse a TOML string directly.

        Returns `(value, nil)` on success; `(nil, "toml.decode_string: ...")` on
        error.

        ```lua
        local t, err = (require("vnd:toml")).decode_string('[server]\nport = 8080\n')
        print(t.server.port)  -- 8080
        ```
    ]=]
    decode_string: (s: string) -> (any, string?),

    --[=[
        Serialise a Lua table to a compact TOML string.
        The root value must be a table (TOML documents are always objects).

        Returns `(string, nil)` on success; `(nil, "toml.encode: ...")` on
        error.
    ]=]
    encode: (value: any) -> (string?, string?),

    --[=[
        Serialise a Lua table to a pretty-printed TOML string.
        Same semantics as `encode` but with extra whitespace for readability.

        Returns `(string, nil)` on success; `(nil, "toml.encode_pretty: ...")` on
        error.
    ]=]
    encode_pretty: (value: any) -> (string?, string?),

    --[=[
        Serialise a Lua table and write the TOML bytes to a `WriterHandle`.

        Returns `(true, nil)` on success; `(nil, "toml.encode_to: ...")` on
        error.
    ]=]
    encode_to: (value: any, writer: any) -> (boolean?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local toml = require("vnd:toml")` resolves `toml` to `TomlModule`.
declare function require(name: "vnd:toml"): TomlModule
