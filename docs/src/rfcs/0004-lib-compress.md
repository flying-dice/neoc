---
status: draft
created: 2026-05-18
implemented-in:
---

# 0004 — `lib:compress` — compression and decompression

> [!WARNING]
> This RFC is `draft`. Under refinement — **not** the contract.

## Summary

`lib:compress` exposes `encode(format, data[, opts])` and `decode(format, data[, opts])` to Luau scripts — unified compression/decompression for gzip, zlib, zstd, and brotli. Format is a string parameter, not per-format functions. Returns `(result, nil)` on success, `(nil, error)` on failure per tuple-error convention.

## Motivation

Compression is table-stakes for any runtime handling HTTP, archives, or wire protocols. Every major runtime ships it: Node `zlib`, Deno `CompressionStream`, Bun `gzipSync`, Go `compress/*`, Python `gzip`/`zlib`. Lune uses `serde.compress(format, data)` / `serde.decompress(format, data)`.

Without `lib:compress`, scripts that receive gzip'd HTTP responses or need to reduce payload sizes must shell out or send uncompressed data. Both are unacceptable for a self-contained runtime.

Concrete scenarios:

- Decompress `Content-Encoding: gzip` HTTP response bodies from `vnd:hyper`
- Compress payloads before writing to storage or sending over the wire
- Process `.gz` / `.zst` files from disk via `std:fs`
- Reduce memory/bandwidth in `std:net` socket communication

## Detailed design

### Luau API

```lua
local compress = require("lib:compress")

-- Encode (compress)
local compressed, err = compress.encode("gzip", data)
local compressed, err = compress.encode("zlib", data)
local compressed, err = compress.encode("brotli", data)
local compressed, err = compress.encode("zstd", data)

-- With compression level
local compressed, err = compress.encode("gzip", data, { level = 9 })
local compressed, err = compress.encode("zstd", data, { level = 3 })

-- Decode (decompress)
local decompressed, err = compress.decode("gzip", compressed)
local decompressed, err = compress.decode("zlib", compressed)
local decompressed, err = compress.decode("brotli", compressed)
local decompressed, err = compress.decode("zstd", compressed)

-- With max output size (decompression bomb protection)
local decompressed, err = compress.decode("gzip", compressed, { max_size = 1048576 }) -- 1 MB
```

Two functions. Format is always the first argument — parameterisable, no per-format aliases. Matches Lune's `serde.compress` / `serde.decompress` pattern but uses `encode` / `decode` to align with `lib:base64`.

### Function signatures

**`compress.encode(format, data[, opts]) → (string, nil) | (nil, string)`**

| Param | Type | Required | Description |
|---|---|---|---|
| `format` | `string` | yes | `"gzip"`, `"zlib"`, `"zstd"`, `"brotli"` |
| `data` | `string` | yes | Raw bytes to compress |
| `opts` | `table?` | no | `{ level = number }` |

`level` semantics per format:

| Format | Default | Range | Backing |
|---|---|---|---|
| gzip | 6 | 0–9 | `flate2::Compression` |
| zlib | 6 | 0–9 | `flate2::Compression` |
| zstd | 3 | 1–22 | `zstd::DEFAULT_COMPRESSION_LEVEL` (note: zstd supports -7 to -1 for fast mode; excluded for v1 surface simplicity) |
| brotli | 6 | 0–11 | brotli quality parameter |

Out-of-range level → `(nil, "compress.encode: level out of range for {format}: {n}")`.

Non-integer `level` value (e.g. `{ level = "fast" }`) → `(nil, "compress.encode: invalid level type")`. Luau numbers that are not whole (e.g. `6.7`) are truncated to integer via `as i32` — this matches Rust's standard numeric coercion and is documented as intentional.

**`compress.decode(format, data[, opts]) → (string, nil) | (nil, string)`**

| Param | Type | Required | Description |
|---|---|---|---|
| `format` | `string` | yes | `"gzip"`, `"zlib"`, `"zstd"`, `"brotli"` |
| `data` | `string` | yes | Compressed bytes to decompress |
| `opts` | `table?` | no | `{ max_size = number }` |

`max_size`: maximum decompressed output size in bytes. Default: 268435456 (256 MB). Exceeding → `(nil, "compress.decode: output exceeds max_size (256 MB)")`. The streaming internals check `output.len()` during writes and bail early — near-zero cost on the happy path.

Rationale: `decode` receives untrusted data in most real-world scenarios (HTTP responses, external files). A 1 KB gzip can expand to 1 GB+ (decompression bomb). Unguarded decode is a memory exhaustion vector. The default is generous enough for legitimate payloads; callers who know their bounds can override.

### Error handling

Returns `(nil, error_string)` on failure via `from_err(lua, "compress.encode", e)` / `from_err(lua, "compress.decode", e)`. Never throws. Compressed data from external sources is untrusted — `pcall`-free validation is the expected pattern.

Error cases:
- Unknown format string → `"compress.encode: unknown format 'lz4'"`
- Corrupt/truncated compressed data → `"compress.decode: {underlying_error}"`
- Out-of-range level → `"compress.encode: level out of range for {format}: {n}"`
- Invalid level type → `"compress.encode: invalid level type"`
- Output exceeds max_size → `"compress.decode: output exceeds max_size (256 MB)"`
- Empty input to `decode` → `(nil, "compress.decode: {underlying_error}")` (no valid compressed stream is 0 bytes)

### Edge cases

- `encode("")` returns a valid compressed empty stream. All four formats produce headers/trailers even for empty content (e.g. gzip emits a 20-byte header+footer).
- `decode("")` returns `(nil, error)`. An empty byte sequence is not a valid compressed stream in any supported format.

### Binary data convention

Input and output are `mlua::String` (Luau strings are byte sequences). Matches `lib:base64`, `std:fs`, and all other binary I/O in the runtime. No special buffer type.

### Backing crates

| Crate | Formats | Notes |
|---|---|---|
| `flate2` | gzip, zlib | De-facto Rust standard. Pure-Rust `miniz_oxide` backend (no C dependency). |
| `zstd` | zstd | Wraps `libzstd` via `zstd-safe`. Vendored C build — consistent with `rusqlite` bundled approach. |
| `brotli` | brotli | Pure-Rust implementation. No C dependency. |

Three crates for four formats. All well-maintained, widely-used in the Rust ecosystem.

### Rust implementation sketch

```rust
pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    t.set("encode", lua.create_function(|lua, (format, data, opts): (String, mlua::String, Option<Table>)| {
        let bytes = data.as_bytes();
        let level = match opts {
            Some(t) => match t.contains_key("level")? {
                true => Some(t.get::<i32>("level").map_err(|_| mlua::Error::runtime("compress.encode: invalid level type"))?),
                false => None,
            },
            None => None,
        };
        match format.as_str() {
            "gzip" => encode_gzip(lua, bytes, level),
            "zlib" => encode_zlib(lua, bytes, level),
            "zstd" => encode_zstd(lua, bytes, level),
            "brotli" => encode_brotli(lua, bytes, level),
            _ => err(lua, format!("compress.encode: unknown format '{format}'")),
        }
    })?)?;

    t.set("decode", lua.create_function(|lua, (format, data, opts): (String, mlua::String, Option<Table>)| {
        let bytes = data.as_bytes();
        let max_size = match &opts {
            Some(t) => t.get::<usize>("max_size").unwrap_or(256 * 1024 * 1024),
            None => 256 * 1024 * 1024, // 256 MB default
        };
        match format.as_str() {
            "gzip" => decode_gzip(lua, bytes, max_size),
            "zlib" => decode_zlib(lua, bytes, max_size),
            "zstd" => decode_zstd(lua, bytes, max_size),
            "brotli" => decode_brotli(lua, bytes, max_size),
            _ => err(lua, format!("compress.decode: unknown format '{format}'")),
        }
    })?)?;

    Ok(t)
}
```

Each `encode_*` function uses streaming internals (write to `Vec<u8>` via `std::io::Write` adapters), then returns `ok(lua, lua.create_string(&output)?)` or `from_err(...)`. Each `decode_*` function takes `max_size` and checks `output.len()` during writes — bailing with an error if the limit is exceeded.

### Streaming internals

The Luau API is buffer-in/buffer-out but the Rust implementation uses streaming I/O internally:

```rust
fn encode_gzip(lua: &Lua, data: &[u8], level: Option<i32>) -> mlua::Result<(Value, Value)> {
    let level = validate_level("gzip", level, 0, 9, 6)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level as u32));
    encoder.write_all(data).map_err(|e| /* ... */)?;
    let compressed = encoder.finish().map_err(|e| /* ... */)?;
    ok(lua, lua.create_string(&compressed)?)
}
```

This avoids requiring the entire output to be pre-allocated. For v1, input is still fully in memory (Luau strings are contiguous). A future streaming API (`compress.encoder("gzip", opts)` returning a stream handle) can be added alongside without breaking `encode`/`decode`.

### Format validation

Format strings are case-sensitive, lowercase only. `"GZIP"` → error. Rationale: every cross-runtime precedent uses lowercase (`Content-Encoding: gzip`, Lune format strings, Python module names). Case-insensitive matching adds code for zero real-world benefit.

## Drawbacks

- **Three new crate dependencies.** `flate2` pulls `miniz_oxide` (pure Rust, small). `zstd` pulls `zstd-sys` (vendored C, ~200 KB compressed source). `brotli` is pure Rust. Total binary size increase estimated 200–400 KB stripped. Acceptable for table-stakes functionality.
- **No streaming Luau API.** Scripts processing very large compressed data (>100 MB) will hold both input and output in memory simultaneously. Acknowledged limitation for v1 — streaming API deferred.
- **`zstd-sys` vendored C build.** Adds ~5s to clean build. Consistent with existing `rusqlite` bundled approach. Cross-compilation works via `cc` crate.

## Alternatives

| Alternative | Verdict |
|---|---|
| Per-format functions (`compress.gzip()`, `compress.gunzip()`) | Rejected — naming inconsistency (`gunzip` vs `inflate` vs `brotli_decompress`), not parameterisable, wider surface for no capability gain |
| `compress` / `decompress` function names | Rejected — `encode` / `decode` aligns with `lib:base64` and is idiomatic for format transformation |
| Streaming-only API (v1) | Rejected — over-engineering for v1. Buffer API covers 95% of use cases. Streaming deferred. |
| lz4 in v1 format set | Deferred — niche (game engines, internal storage). Add on demand. |
| Format auto-detection on `decode` | Deferred — caller typically knows format from context (HTTP headers, file extension). `compress.detect(data)` planned for v2. |
| Do nothing | Rejected — scripts cannot handle compressed HTTP responses or reduce payload sizes. Table-stakes gap. |

## Resolved questions

- **Decompression size limit.** Resolved: `decode` enforces `max_size` (default 256 MB) via opts table. Streaming internals check `output.len()` during writes and bail early. See `decode` signature above.
- **`decode` opts slot.** Resolved: `decode(format, data[, opts])` — consistent with `encode` signature, forward-compatible for future options.
- **Level type coercion.** Resolved: non-number types (e.g. `{ level = "fast" }`) → explicit error. Float-to-int truncation (e.g. `6.7` → `6`) is documented as intentional Rust `as i32` behaviour.

## Open questions

- **`zstd` dictionary support.** `zstd` supports pre-trained dictionaries for small-payload compression. Defer to v2 or include `{ dictionary = buffer }` option now?

## Implementation notes

- `src/lua/lib/compress.rs` — module; exports `pub fn module(lua) -> mlua::Result<Table>`
- `src/lua/lib/mod.rs` — `pub mod compress` added alphabetically
- `src/lua/modules.rs` — `lib:compress` registered
- `Cargo.toml` — add `flate2`, `zstd`, `brotli`
- `tests/lib/compress.test.luau` — integration tests: round-trip each format, level option, error cases, cross-format incompatibility
- `benches/lib/compress.rs` — Criterion benchmarks: three-tier (library / into_out_lua / through_lua) for encode + decode per format at default and max levels
- `.d.luau` type stub for `lib:compress`
