--!strict
--
-- Type definitions for `vnd:quick_xml` — paired with
-- `src/lua/vnd/quick_xml.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## XML representation in Lua
--
-- Decode goes through `serde_json::Value`, then to Lua tables. quick-xml
-- represents element text as `$text` and attributes as `@<name>`. This
-- module applies a flattening pass: when an element has only a `$text`
-- field, the object is replaced with the bare text value, so
-- `<name>x</name>` decodes to `{ name = "x" }` instead of
-- `{ name = { ["$text"] = "x" } }`.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    The module table returned by `require("vnd:quick_xml")`.
]=]
export type QuickXmlModule = {
    --[=[
        Parse XML from a `ReaderHandle`. Reads the handle to completion, then
        parses.

        Returns `(value, nil)` on success; `(nil, "quick_xml.decode: ...")` on
        error (not a reader handle, invalid UTF-8, or invalid XML).

        ```lua
        local io  = require("std:io")
        local xml = require("vnd:quick_xml")
        local r   = io.from_string("<root><name>Alice</name></root>")
        local t, err = xml.decode(r)
        print(t.name)  -- "Alice"
        ```
    ]=]
    decode: (reader: any) -> (any, string?),

    --[=[
        Parse an XML string directly.

        Returns `(value, nil)` on success; `(nil, "quick_xml.decode_string: ...")` on
        error.

        ```lua
        local t, err = (require("vnd:quick_xml")).decode_string("<item>hello</item>")
        ```
    ]=]
    decode_string: (s: string) -> (any, string?),

    --[=[
        Serialise a Lua value to an XML string. The root element is always
        named `<root>`. Lua table keys become child element names; arrays
        produce repeated sibling elements.

        Returns `(string, nil)` on success; `(nil, "quick_xml.encode: ...")` on
        error.

        ```lua
        local s, err = (require("vnd:quick_xml")).encode({ name = "Alice", age = 30 })
        -- '<root><age>30</age><name>Alice</name></root>'
        ```
    ]=]
    encode: (value: any) -> (string?, string?),

    --[=[
        Serialise a Lua value and write the XML bytes to a `WriterHandle`.

        Returns `(true, nil)` on success; `(nil, "quick_xml.encode_to: ...")` on
        error.
    ]=]
    encode_to: (value: any, writer: any) -> (boolean?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local quick_xml = require("vnd:quick_xml")` resolves to `QuickXmlModule`.
declare function require(name: "vnd:quick_xml"): QuickXmlModule
