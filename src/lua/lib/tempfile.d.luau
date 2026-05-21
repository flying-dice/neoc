--!strict
--
-- Type definitions for `lib:tempfile` — paired with `src/lua/lib/tempfile.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface is
-- reviewed alongside the implementation.
--
-- ## Surface
--
-- Two handle types — `TempFile`, `TempDir` — paired with flat helpers
-- `tempfile`, `tempdir`, `gettempdir`. Same shape as Python's `tempfile`,
-- Deno's `Deno.makeTempFile` / `makeTempDir`, and Go's `os.CreateTemp` /
-- `os.MkdirTemp`. Handles unlink on garbage collection — let them go out
-- of scope and cleanup happens for free.
--
-- ## Persistence
--
-- Call `:keep()` to consume the handle and keep the file or directory on
-- disk past drop. Once kept, the handle is no longer usable — subsequent
-- `:keep()` returns `(nil, "...already closed")`.
--
-- ## Explicit close
--
-- `:close()` releases the handle eagerly. Idempotent — calling twice is
-- fine. For directories the second return surfaces unlink errors so a
-- script can react to "directory in use" or similar.
--
-- ## Options
--
-- All constructors accept an optional table:
--
-- | Field    | Default                | Notes                                   |
-- |----------|------------------------|-----------------------------------------|
-- | `prefix` | `"tmp"`                | Filename prefix.                        |
-- | `suffix` | `""`                   | Filename suffix.                        |
-- | `dir`    | `tempfile.gettempdir()`| Parent directory. Must already exist.   |
--
-- Prefix or suffix containing a path separator surfaces as the underlying
-- crate's error rather than being silently rewritten.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag those lines as errors. luau-lsp consumes them correctly.

export type TempFileOptions = {
    prefix: string?,
    suffix: string?,
    dir: string?,
}

export type TempDirOptions = TempFileOptions

--[=[
    A handle to a single temporary file. The file is unlinked when the
    handle is garbage-collected; call `:keep()` to persist it, or
    `:close()` to release the handle eagerly.
]=]
export type TempFile = {
    --[=[
        Return the absolute path to the temp file as a string. Stable
        across calls — does not consume the handle.
    ]=]
    path: (self: TempFile) -> string,

    --[=[
        Consume the handle and keep the file on disk past drop. Returns
        the path on success. Subsequent calls return `(nil, err)` because
        the handle is already consumed.

        ```lua
        local f = assert(tempfile.tempfile())
        local persisted = assert(f:keep())
        ```
    ]=]
    keep: (self: TempFile) -> (string?, string?),

    --[=[
        Release the handle eagerly. Idempotent — calling twice on the
        same handle is safe.
    ]=]
    close: (self: TempFile) -> (),
}

--[=[
    A handle to a temporary directory. The directory and everything
    inside it is unlinked recursively on garbage collection.
]=]
export type TempDir = {
    --[=[
        Return the absolute path to the temp directory as a string.
    ]=]
    path: (self: TempDir) -> string,

    --[=[
        Consume the handle and keep the directory on disk past drop.
        Returns the path. Once kept, the handle is no longer usable.
    ]=]
    keep: (self: TempDir) -> (string?, string?),

    --[=[
        Remove the directory eagerly. Returns `(true, nil)` on success,
        `(nil, err)` if the unlink fails. Idempotent — calling twice
        always returns `(true, nil)` on the second call.
    ]=]
    close: (self: TempDir) -> (boolean?, string?),
}

export type TempFileConstructor = {
    --[=[
        Construct a new `TempFile`. Same options table as the flat
        `tempfile.tempfile()`; this constructor is the canonical
        UserData entry point, and `tempfile()` is the ergonomic alias.
    ]=]
    new: (opts: TempFileOptions?) -> (TempFile?, string?),
}

export type TempDirConstructor = {
    --[=[
        Construct a new `TempDir`. Same options table as the flat
        `tempfile.tempdir()`.
    ]=]
    new: (opts: TempDirOptions?) -> (TempDir?, string?),
}

--[=[
    The module table returned by `require("lib:tempfile")`.
]=]
export type TempfileModule = {
    --[=[
        Constructor table for the `TempFile` UserData.
    ]=]
    TempFile: TempFileConstructor,

    --[=[
        Constructor table for the `TempDir` UserData.
    ]=]
    TempDir: TempDirConstructor,

    --[=[
        Create a temporary file under the system temp directory (or
        `opts.dir` if provided). Returns `(TempFile, nil)` on success;
        `(nil, "tempfile.TempFile.new: ...")` if the underlying syscall
        fails (e.g. parent directory missing).

        ```lua
        local tempfile = require("lib:tempfile")
        local f = assert(tempfile.tempfile({prefix = "neoc-", suffix = ".tmp"}))
        print(f:path())
        -- file vanishes when `f` is garbage-collected
        ```
    ]=]
    tempfile: (opts: TempFileOptions?) -> (TempFile?, string?),

    --[=[
        Create a temporary directory under the system temp directory
        (or `opts.dir` if provided). Returns `(TempDir, nil)` on
        success; `(nil, "tempfile.TempDir.new: ...")` on failure.

        ```lua
        local tempfile = require("lib:tempfile")
        local d = assert(tempfile.tempdir())
        ```
    ]=]
    tempdir: (opts: TempDirOptions?) -> (TempDir?, string?),

    --[=[
        Return the system's temporary directory path. Mirrors Python
        `tempfile.gettempdir()`, Node `os.tmpdir`, Go `os.TempDir`.
    ]=]
    gettempdir: () -> string,
}

declare function require(name: "lib:tempfile"): TempfileModule
