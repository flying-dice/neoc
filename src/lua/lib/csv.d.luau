--!strict
--
-- Type definitions for `lib:csv` — paired with `src/lua/lib/csv.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

local io = require("std:io")

--[=[
    Supported column types for schema-driven coercion.
]=]
export type ColumnType = "number" | "boolean"

--[=[
    Options shared by `csv.parse`, `csv.rows`, and `csv.reader`.
]=]
export type ParseOpts = {
    delimiter: string?,
    headers: boolean?,
    quote: string?,
    escape: string?,
    comment: string?,
    types: { [string]: ColumnType }?,
}

--[=[
    Options for `csv.stringify` and `csv.writer`. `headers` controls both the
    header row and the field output order.
]=]
export type WriteOpts = {
    headers: { string },
    delimiter: string?,
    quote: string?,
}

--[=[
    A parsed CSV row. Without `headers = true` rows are arrays of strings;
    with `headers = true` rows are tables keyed by header name. Typed columns
    yield numbers/booleans/nil; untyped columns are always strings.
]=]
export type Row = { [string]: any } | { string }

--[=[
    Iterator returned by `csv.rows` and `reader:rows()`. Calling it returns
    the next row or `nil` at EOF. Errors raise (not tuple) — the Luau iterator
    protocol cannot return `(nil, err)`.
]=]
export type RowIterator = () -> Row?

--[=[
    Streaming CSV reader over a `ReaderHandle`. Returned by `csv.reader`.
]=]
export type CsvReader = {
    --[=[
        Returns the cached header row (or parses it if not yet read).
        Errors if the reader was not created with `headers = true`.
    ]=]
    headers: (self: CsvReader) -> ({ string }?, string?),

    --[=[
        Returns a Luau iterator yielding the next row on each call, `nil`
        at EOF. Iterator errors raise.
    ]=]
    rows: (self: CsvReader) -> RowIterator,
}

--[=[
    Incremental CSV writer. Returned by `csv.writer`.
]=]
export type CsvWriter = {
    --[=[
        Append a row. `values` is either an array (positional, length must
        match `headers`) or a keyed table (looked up by header name; missing
        keys → empty string). Mixed array+keyed shape is rejected.
    ]=]
    row: (self: CsvWriter, values: { [any]: any }) -> (boolean?, string?),

    --[=[
        Flush and return the accumulated CSV string. Subsequent calls error.
    ]=]
    finish: (self: CsvWriter) -> (string?, string?),
}

--[=[
    The module table returned by `require("lib:csv")`.
]=]
export type CsvModule = {
    --[=[
        Parse a CSV string. Returns `(table, nil)` on success or
        `(nil, "csv.parse: ...")` on failure.

        ```lua
        local csv = require("lib:csv")
        local rows = csv.parse("a,b\n1,2")
        local records = csv.parse("name,age\nAlice,30", { headers = true })
        ```
    ]=]
    parse: (data: string, opts: ParseOpts?) -> ({ Row }?, string?),

    --[=[
        Return a Luau iterator over rows in an in-memory CSV string. The
        bytes are already loaded; only the parsed row table is allocated
        lazily. Iterator errors raise.

        ```lua
        for row in csv.rows(data, { headers = true }) do
            if row.status == "FAIL" then process(row) end
        end
        ```
    ]=]
    rows: (data: string, opts: ParseOpts?) -> RowIterator,

    --[=[
        Construct a streaming reader over a `ReaderHandle` (from
        `std:fs.open`, `std:io.from_string`, or any handle producer). The
        reader takes an exclusive borrow on the handle for its lifetime —
        a second `csv.reader` on the same handle returns a tuple error.

        ```lua
        local fs = require("std:fs")
        local h  = fs.open({ path = "huge.csv", mode = "r" })
        local r  = csv.reader(h, { headers = true })
        for row in r:rows() do process(row) end
        ```
    ]=]
    reader: (handle: io.ReaderHandle, opts: ParseOpts?) -> (CsvReader?, string?),

    --[=[
        Serialise an array of records to a CSV string. `headers` is
        required and controls both the header row and the field output
        order.

        ```lua
        local out = csv.stringify(
            {{ name = "Alice", age = "30" }},
            { headers = { "name", "age" } }
        )
        ```
    ]=]
    stringify: (records: { Row }, opts: WriteOpts) -> (string?, string?),

    --[=[
        Construct an incremental writer that buffers in memory until
        `:finish()`. `headers` is required and emitted as the first row.

        ```lua
        local w = csv.writer({ headers = { "test", "status" } })
        w:row({ "login", "PASS" })
        local output = w:finish()
        ```
    ]=]
    writer: (opts: WriteOpts) -> (CsvWriter?, string?),
}

declare function require(name: "lib:csv"): CsvModule
