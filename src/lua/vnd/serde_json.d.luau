--!strict
--
-- Type definitions for `vnd:serde_json` — paired with
-- `src/lua/vnd/serde_json.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Relationship to `lib:json`
--
-- `vnd:serde_json` is the parse/serialise layer. It turns JSON bytes into
-- `JsonObject` / `JsonArray` handles (from `lib:json`) and back. Start here
-- when you have a JSON string or stream. Go to `lib:json` when you want to
-- construct or mutate documents programmatically.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    The module table returned by `require("vnd:serde_json")`.
]=]
export type SerdeJsonModule = {
    --[=[
        Parse a JSON string into a `JsonObject` or `JsonArray` handle.

        Returns `(handle, nil)` on success; `(nil, "serde_json.from_str: ...")` if
        the input is not valid JSON.

        ```lua
        local serde_json = require("vnd:serde_json")
        local obj, err   = serde_json.from_str('{"name":"Alice"}')
        print(obj:get("name"))  -- "Alice"
        ```
    ]=]
    from_str: (s: string) -> (any, string?),

    --[=[
        Parse JSON from a `ReaderHandle` (e.g. from `fs.open` or `io.from_string`).

        Returns `(handle, nil)` on success; `(nil, "serde_json.from_reader: ...")` on
        error (not a reader handle, or invalid JSON).

        ```lua
        local reader = (require("std:io")).from_string('{"x":1}')
        local obj, err = (require("vnd:serde_json")).from_reader(reader)
        ```
    ]=]
    from_reader: (reader: any) -> (any, string?),

    --[=[
        Serialise a Lua value to a compact JSON string.

        Accepts: tables (converted to JSON object or array), strings, numbers,
        booleans, `nil` (→ JSON `null`), and `JsonObject` / `JsonArray` handles.

        Returns `(string, nil)` on success; `(nil, "serde_json.to_string: ...")` on
        serialisation error.

        ```lua
        local s, err = (require("vnd:serde_json")).to_string({ name = "Alice", age = 30 })
        -- '{"age":30,"name":"Alice"}'
        ```
    ]=]
    to_string: (value: any) -> (string?, string?),

    --[=[
        Serialise a Lua value to an indented, human-readable JSON string.
        Same input rules as `to_string`.

        Returns `(string, nil)` on success; `(nil, "serde_json.to_string_pretty: ...")` on
        error.
    ]=]
    to_string_pretty: (value: any) -> (string?, string?),

    --[=[
        Serialise a Lua value and write the JSON bytes to a `WriterHandle`.

        Returns `(true, nil)` on success; `(nil, "serde_json.to_writer: ...")` on
        error (not a writer handle, or serialisation failure).

        ```lua
        local fs         = require("std:fs")
        local serde_json = require("vnd:serde_json")
        local w          = fs.open({ path = "out.json", mode = "w" })
        local ok, err    = serde_json.to_writer({ items = { 1, 2, 3 } }, w)
        w:flush()
        ```
    ]=]
    to_writer: (value: any, writer: any) -> (boolean?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local serde_json = require("vnd:serde_json")` resolves to `SerdeJsonModule`.
declare function require(name: "vnd:serde_json"): SerdeJsonModule
