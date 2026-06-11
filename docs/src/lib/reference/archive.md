# `lib:archive`

**Stable**

Create and extract `tar` (optionally gzip/zstd-compressed) and `zip` archives, in memory or on disk. Wraps [`tar`](https://crates.io/crates/tar) + [`flate2`](https://crates.io/crates/flate2)/[`zstd`](https://crates.io/crates/zstd) and [`zip`](https://crates.io/crates/zip).

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/archive.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/archive.rs) |
| Tests | [`tests/lib/archive.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/archive.test.luau) |
| Stability | Stable |

## Syntax

```lua
local archive = require("lib:archive")
```

## Description

All functions return `(result, err)`. On success `err` is `nil`; on failure `result` is `nil` and `err` is prefixed with the function name.

Buffer variants (`*_bytes`) are the primary API — the dominant agent/pipeline pattern is bytes in memory (a tarball from an HTTP response body), not files on disk. The file-oriented wrappers (`tar_create`, `tar_extract`, …) are convenience on top and share the same core.

Compression is handled internally: you never `require("lib:compress")` to open a `.tar.gz`.

- **On create**, the tar codec is inferred from the file extension (`.tar.gz`/`.tgz` → gzip, `.tar.zst` → zstd, bare `.tar` and all in-memory tars → none). `opts.compression` (`"gzip"`, `"zstd"`, `"none"`) overrides it; a clash between the explicit option and a compression-bearing extension is an error.
- **On read** (extract/list), the codec is auto-detected from the stream's magic bytes, so the same call transparently opens a raw, gzipped, or zstd-compressed tar.

`path_or_bytes` arguments accept either: a valid UTF-8 string naming an existing file is read from disk; anything else is treated as raw archive bytes.

### Security

Extraction is safe by default. Every entry name is validated before any byte is written; a rejected entry fails the whole call with `(nil, err)`. Rejected:

- traversal that escapes the destination (`../../etc/passwd`),
- absolute paths (`/etc/passwd`, `C:\Windows\...`) — including Windows drive prefixes on any host,
- entries named `.` or `..`, or containing a null byte,
- names longer than 4096 bytes,
- Windows `\` separators are normalised to `/` *before* the checks above,
- symlink/hardlink targets that resolve outside the destination.

A name that *normalises* back inside the destination is accepted (`safe/../safe/file.txt` → `safe/file.txt`).

Decompression bombs are bounded by `opts.max_output_size` on every extract function — total decompressed output in bytes. The default is **256 MiB** (the same ceiling as `lib:compress`), so an agent that sets no limit is still protected.

## Module functions

### Tar

#### archive.tar_create_bytes

```lua
archive.tar_create_bytes(entries: { { path: string, archive_path: string? } }, opts?: table): (string?, string?)
```

Builds a tar archive in memory and returns its bytes. Each entry reads file `path` from disk; `archive_path` overrides the stored name (defaults to `path`). A `path` that is a directory is walked recursively. `opts.compression` defaults to `"none"`. `opts.strip_prefix` removes a leading prefix from every stored name.

#### archive.tar_create

```lua
archive.tar_create(path: string, entries: { ... }, opts?: table): (boolean?, string?)
```

Same as `tar_create_bytes` but writes to `path`. Compression is inferred from the extension unless `opts.compression` overrides it.

#### archive.tar_extract

```lua
archive.tar_extract(path_or_bytes: string, dest: string, opts?: table): (number?, string?)
```

Extracts an archive under `dest`, returning the number of entries written. `opts.include` is a list of glob patterns (matched with [`globset`](https://crates.io/crates/globset), the same engine behind `lib:glob` — `*` is not separator-aware); only matching entries are written. `opts.max_output_size` caps total output.

#### archive.tar_extract_bytes

```lua
archive.tar_extract_bytes(bytes: string, name: string, opts?: table): (string?, string?)
```

Returns the contents of a single named entry from an in-memory archive. `opts.max_output_size` caps output.

#### archive.tar_list / archive.tar_list_bytes

```lua
archive.tar_list(path_or_bytes: string): (table?, string?)
archive.tar_list_bytes(bytes: string): (table?, string?)
```

Returns one metadata table per entry: `{ name, size, mode, timestamp, is_dir, compression_ratio }`. For tar, `compression_ratio` is always `nil` — a `.tar.gz` is one solid stream with no per-entry compressed size. Callers ignore fields they don't need.

### Zip

#### archive.zip_create_bytes

```lua
archive.zip_create_bytes(entries: { [string]: string }): (string?, string?)
```

Builds a zip archive in memory from a name → contents map and returns its bytes.

#### archive.zip_create

```lua
archive.zip_create(path: string, entries: { string }, opts?: table): (boolean?, string?)
```

Writes a zip to `path` from a list of filesystem paths (files or directories, walked recursively). `opts.strip_prefix` applies.

#### archive.zip_extract

```lua
archive.zip_extract(path_or_bytes: string, dest: string, opts?: table): (number?, string?)
```

As `tar_extract` (same `include` / `max_output_size` options and traversal protection).

#### archive.zip_extract_bytes

```lua
archive.zip_extract_bytes(bytes: string, name: string, opts?: table): (string?, string?)
```

Returns the contents of a single named entry.

#### archive.zip_list / archive.zip_list_bytes

```lua
archive.zip_list(path_or_bytes: string): (table?, string?)
archive.zip_list_bytes(bytes: string): (table?, string?)
```

Same metadata as `tar_list`. For zip, `compression_ratio` is the per-entry `compressed_size / size` (lower means better compression), or `nil` for a zero-byte entry.

## Examples

```lua
-- Receive a tarball over HTTP, list it, pull one file out — no temp files.
local entries = archive.tar_list_bytes(response_body)
for _, e in entries do
    print(e.name, e.size)
end
local main = archive.tar_extract_bytes(response_body, "app/main.lua")

-- Pack a directory to a gzipped tarball on disk.
archive.tar_create("dist.tar.gz", {
    { path = "build/", archive_path = "app/" },
}, { strip_prefix = "app/" })

-- Extract only Lua files, safely, with a tight bomb cap.
archive.zip_extract(zip_bytes, "out/", {
    include = { "*.lua" },
    max_output_size = 8 * 1024 * 1024,
})
```

## Out of scope

Streaming extraction, password-protected zip, and 7z/rar are not supported.
