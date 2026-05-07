--!strict
--
-- Type definitions for `vnd:serde_yaml` — paired with
-- `src/lua/vnd/serde_yaml.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## YAML-specific types
--
-- Decode goes through `serde_json::Value` as the normalisation step.
-- YAML-specific types (timestamps, tagged values) are flattened to
-- strings/objects and are not preserved on round-trip.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    The module table returned by `require("vnd:serde_yaml")`.
]=]
export type SerdeYamlModule = {
    --[=[
        Parse YAML from a `ReaderHandle` (e.g. from `fs.open` or
        `io.from_string`). Reads the handle to completion, then parses.

        Returns `(value, nil)` on success; `(nil, "serde_yaml.decode: ...")` on
        error (not a reader handle, or invalid YAML).

        ```lua
        local io   = require("std:io")
        local yaml = require("vnd:serde_yaml")
        local r    = io.from_string("key: value\n")
        local t, err = yaml.decode(r)
        print(t.key)  -- "value"
        ```
    ]=]
    decode: (reader: any) -> (any, string?),

    --[=[
        Parse a YAML string directly.

        Returns `(value, nil)` on success; `(nil, "serde_yaml.decode_string: ...")` on
        error.

        ```lua
        local t, err = (require("vnd:serde_yaml")).decode_string("x: 1\ny: 2\n")
        ```
    ]=]
    decode_string: (s: string) -> (any, string?),

    --[=[
        Serialise a Lua value to a YAML string.

        Accepts: tables, strings, numbers, booleans, `nil`.

        Returns `(string, nil)` on success; `(nil, "serde_yaml.encode: ...")` on
        error.

        ```lua
        local s, err = (require("vnd:serde_yaml")).encode({ host = "localhost", port = 8080 })
        ```
    ]=]
    encode: (value: any) -> (string?, string?),

    --[=[
        Serialise a Lua value and write the YAML bytes to a `WriterHandle`.

        Returns `(true, nil)` on success; `(nil, "serde_yaml.encode_to: ...")` on
        error.
    ]=]
    encode_to: (value: any, writer: any) -> (boolean?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local serde_yaml = require("vnd:serde_yaml")` resolves to `SerdeYamlModule`.
declare function require(name: "vnd:serde_yaml"): SerdeYamlModule
