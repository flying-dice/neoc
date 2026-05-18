--!strict
--
-- Type definitions for `lib:compress` — paired with `src/lua/lib/compress.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Options for `compress.encode`.
]=]
export type EncodeOpts = {
    level: number?,
}

--[=[
    Options for `compress.decode`.
]=]
export type DecodeOpts = {
    max_size: number?,
}

--[=[
    Supported compression formats.
]=]
export type Format = "gzip" | "zlib" | "zstd" | "brotli"

--[=[
    The module table returned by `require("lib:compress")`.
]=]
export type CompressModule = {
    --[=[
        Compress `data` using `format`.

        Returns `(string, nil)` on success; `(nil, "compress.encode: ...")` on
        failure.

        Supported formats: `"gzip"`, `"zlib"`, `"zstd"`, `"brotli"`.

        ```lua
        local compress = require("lib:compress")
        local compressed, err = compress.encode("gzip", data)
        local compressed, err = compress.encode("zstd", data, { level = 3 })
        ```
    ]=]
    encode: (format: Format, data: string, opts: EncodeOpts?) -> (string?, string?),

    --[=[
        Decompress `data` using `format`.

        Returns `(string, nil)` on success; `(nil, "compress.decode: ...")` on
        failure.

        `max_size` (default 256 MB) caps decompressed output to guard against
        decompression bombs.

        ```lua
        local compress = require("lib:compress")
        local decompressed, err = compress.decode("gzip", compressed)
        local decompressed, err = compress.decode("gzip", compressed, { max_size = 1048576 })
        ```
    ]=]
    decode: (format: Format, data: string, opts: DecodeOpts?) -> (string?, string?),
}

declare function require(name: "lib:compress"): CompressModule
