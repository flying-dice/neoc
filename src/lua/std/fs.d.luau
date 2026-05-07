--!strict
--
-- Type definitions for `std:fs` — paired with `src/lua/std/fs.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Mirrors `std::fs`
--
-- Every function maps 1:1 to a Rust `std::fs` function. All fallible
-- functions return `(result, nil)` on success or `(nil, "fs.op: ...")` on
-- failure — the standard neoc error convention.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Options table for `fs.open`.
]=]
export type OpenOpts = {
    --[=[
        Filesystem path to open.
    ]=]
    path: string,
    --[=[
        Open mode:
        - `"r"` — read-only. Fails if the file does not exist.
        - `"w"` — write, truncating any existing content.
        - `"a"` — append. Creates the file if absent.
    ]=]
    mode: "r" | "w" | "a",
}

--[=[
    The module table returned by `require("std:fs")`.
]=]
export type FsModule = {
    --[=[
        Create a single directory. Mirrors `std::fs::create_dir`.
        Fails if the path already exists or a parent directory is missing.

        Returns `(true, nil)` on success; `(nil, "fs.create_dir: ...")` on error.

        ```lua
        local fs = require("std:fs")
        local ok, err = fs.create_dir("data")
        ```
    ]=]
    create_dir: (path: string) -> (boolean?, string?),

    --[=[
        Create a directory and all missing ancestors. Mirrors
        `std::fs::create_dir_all`. Returns `(true, nil)` on success or if the
        directory already exists; `(nil, "fs.create_dir_all: ...")` on error.

        ```lua
        local ok, err = fs.create_dir_all("data/cache/tmp")
        ```
    ]=]
    create_dir_all: (path: string) -> (boolean?, string?),

    --[=[
        Open a file and return a reader or writer handle.

        - `mode = "r"` → `ReaderHandle`
        - `mode = "w"` or `"a"` → `WriterHandle`

        Returns `(handle, nil)` on success; `(nil, "fs.open: ...")` on error.

        ```lua
        local fs  = require("std:fs")
        local io  = require("std:io")

        local r, err = fs.open({ path = "input.txt", mode = "r" })
        local data = r:read_all()

        local w, err = fs.open({ path = "output.txt", mode = "w" })
        w:write("hello")
        w:flush()
        ```
    ]=]
    open: (opts: OpenOpts) -> (any, string?),

    --[=[
        Read the entire contents of `path` into a string.
        Mirrors `std::fs::read`. Binary-safe — the returned string may contain
        arbitrary bytes.

        Returns `(string, nil)` on success; `(nil, "fs.read: ...")` on error.

        ```lua
        local contents, err = fs.read("config.toml")
        ```
    ]=]
    read: (path: string) -> (string?, string?),

    --[=[
        Write `data` to `path`, replacing any existing file content.
        Mirrors `std::fs::write`. Binary-safe — `data` may be arbitrary bytes.

        Returns `(true, nil)` on success; `(nil, "fs.write: ...")` on error.

        ```lua
        local ok, err = fs.write("output.txt", "hello world")
        ```
    ]=]
    write: (path: string, data: string) -> (boolean?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local fs = require("std:fs")` resolves `fs` to `FsModule`.
declare function require(name: "std:fs"): FsModule
