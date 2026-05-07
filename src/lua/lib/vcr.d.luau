--!strict
--
-- Type definitions for `lib:vcr` — paired with `src/lua/lib/vcr.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## VCR modes
--
-- | mode      | behaviour                                                   |
-- |-----------|-------------------------------------------------------------|
-- | `"record"` | wraps real functions; captures args + results to disk      |
-- | `"replay"` | returns stored results; real function is never called      |
-- | `"off"`    | pass-through; zero overhead; same shape as record cassette |
--
-- Setting `NEOC_VCR=off` forces all constructors to pass-through mode
-- regardless of the `mode` field, so production code can leave VCR calls in
-- place.
--
-- ## Usage
--
-- ```lua
-- local vcr = require("lib:vcr")
-- local b64 = require("lib:base64")
--
-- local cassette = vcr.cassette({ path = "cassettes/b64.json", mode = "record" })
-- local encode   = cassette:wrap("encode.hello", b64.encode)
-- encode("hello")  -- recorded
--
-- local cassette2 = vcr.cassette({ path = "cassettes/b64.json", mode = "replay" })
-- local encode2   = cassette2:wrap("encode.hello", b64.encode)
-- encode2("anything")  -- returns recorded result
-- ```
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Options passed to `vcr.cassette(opts)`.
]=]
export type CassetteOpts = {
    --[=[
        Cassette mode. Required.
        - `"record"` — write results to `path` after each call.
        - `"replay"` — read results from `path`; real function not called.
        - `"off"`    — pass-through; `path` ignored.
    ]=]
    mode: "record" | "replay" | "off",

    --[=[
        File path for the cassette JSON file.
        Required in `"record"` and `"replay"` modes; ignored in `"off"` mode.
    ]=]
    path: string?,
}

--[=[
    A cassette handle returned by `vcr.cassette(opts)`. Exposes `wrap` to
    instrument functions and `dump` for explicit flush (no-op except in
    `"record"` mode).
]=]
export type Cassette = {
    --[=[
        Wrap `fn` under a stable `key`.

        - In `"record"` mode: calls the real `fn`, captures its args and
          return values to the cassette keyed by `key`, and returns the
          real result. Errors if `key` is registered more than once.
        - In `"replay"` mode: returns a closure that ignores its arguments
          and returns the stored result for `key`. Errors if `key` has no
          cassette entry.
        - In `"off"` mode: returns `fn` unchanged. Zero overhead.

        ```lua
        local encode = cassette:wrap("base64.encode.hello", b64.encode)
        ```
    ]=]
    wrap: (key: string, fn: (...any) -> ...any) -> (...any) -> ...any,

    --[=[
        Explicit flush (no-op). In `"record"` mode the cassette is written
        to disk after every call; `dump` exists for symmetry and to support
        future eager-flush use cases.
    ]=]
    dump: () -> (),
}

--[=[
    The module table returned by `require("lib:vcr")`.
]=]
export type VcrModule = {
    --[=[
        Create a cassette in the given mode. See `CassetteOpts` for options.

        If the `NEOC_VCR=off` environment variable is set, all cassettes are
        forced into pass-through mode regardless of `opts.mode`.

        ```lua
        local vcr = require("lib:vcr")
        local c = vcr.cassette({ path = "cassettes/foo.json", mode = "record" })
        ```
    ]=]
    cassette: (opts: CassetteOpts) -> Cassette,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local vcr = require("lib:vcr")` resolves `vcr` to `VcrModule`.
declare function require(name: "lib:vcr"): VcrModule
