--!strict
--
-- Type definitions for `std:io` — paired with `src/lua/std/io.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## IO handles
--
-- `ReaderHandle` and `WriterHandle` are the canonical trait-object wrappers
-- for the io subsystem (ADR 0003). They plug directly into other modules:
-- `fs.open` returns a Reader or Writer, `serde_json.from_reader` reads from
-- a Reader, etc.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Opaque handle to a buffered readable byte stream.

    Returned by `io.from_string`, `fs.open({ mode = "r" })`, and any other
    module that produces readable streams. Pass it to `serde_json.from_reader`,
    `serde_yaml.decode`, `io.copy`, etc.

    ```lua
    local io = require("std:io")
    local r = io.from_string("hello")
    local data, err = r:read_all()
    ```
]=]
export type ReaderHandle = {
    --[=[
        Read up to `n` bytes from the stream.

        Returns `(string, nil)` with the bytes read (may be fewer than `n` at
        EOF), or `(nil, nil)` when the stream is exhausted (EOF with 0 bytes
        available), or `(nil, "io.read: ...")` on error.
    ]=]
    read: (self: ReaderHandle, n: number) -> (string?, string?),

    --[=[
        Read one line, stripping the trailing `\n` (and `\r\n` on Windows).

        Returns `(string, nil)` with the line content, or `(nil, nil)` at EOF,
        or `(nil, "io.read_line: ...")` on error.
    ]=]
    read_line: (self: ReaderHandle) -> (string?, string?),

    --[=[
        Read the entire remaining stream into a single string.

        Returns `(string, nil)` on success or `(nil, "io.read_all: ...")` on
        error.
    ]=]
    read_all: (self: ReaderHandle) -> (string?, string?),
}

--[=[
    Opaque handle to a buffered writable byte stream.

    Returned by `fs.open({ mode = "w" })`, `fs.open({ mode = "a" })`, and
    any other module that produces writable streams. Pass it to
    `serde_json.to_writer`, `serde_yaml.encode_to`, `io.copy`, etc.

    ```lua
    local fs = require("std:fs")
    local writer, err = fs.open({ path = "out.txt", mode = "w" })
    local ok, err = writer:write("hello")
    writer:flush()
    ```
]=]
export type WriterHandle = {
    --[=[
        Write all bytes of `data` to the stream.

        Returns `(true, nil)` on success or `(nil, "io.write: ...")` on error.
    ]=]
    write: (self: WriterHandle, data: string) -> (boolean?, string?),

    --[=[
        Flush any buffered bytes to the underlying sink.

        Returns `(true, nil)` on success or `(nil, "io.flush: ...")` on error.
        Writes are buffered — call `flush` before closing if you need to ensure
        data is committed.
    ]=]
    flush: (self: WriterHandle) -> (boolean?, string?),
}

--[=[
    The module table returned by `require("std:io")`.
]=]
export type IoModule = {
    --[=[
        Wrap a Lua string in a `ReaderHandle` so it can be passed to functions
        that expect a reader (e.g. `serde_json.from_reader`).

        ```lua
        local io = require("std:io")
        local r = io.from_string('{"key":"value"}')
        local obj, err = (require("vnd:serde_json")).from_reader(r)
        ```
    ]=]
    from_string: (s: string) -> ReaderHandle,

    --[=[
        Copy all remaining bytes from `reader` into `writer`.

        Returns `(number, nil)` — the total bytes copied — on success, or
        `(nil, "io.copy: ...")` on error.

        ```lua
        local io = require("std:io")
        local fs = require("std:fs")
        local reader = io.from_string("hello")
        local writer = fs.open({ path = "out.txt", mode = "w" })
        local n, err = io.copy(reader, writer)
        ```
    ]=]
    copy: (reader: ReaderHandle, writer: WriterHandle) -> (number?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local io = require("std:io")` resolves `io` to `IoModule`.
declare function require(name: "std:io"): IoModule
