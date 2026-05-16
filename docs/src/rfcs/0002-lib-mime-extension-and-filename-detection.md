---
status: draft
created: 2026-05-04
implemented-in:
---

# 0002 — `lib:mime` — extension-to-MIME mapping via `mime_guess`, extension preference layer

> [!WARNING]
> This RFC is `draft`. Under refinement — **not** the contract.

## Summary

`lib:mime` maps file extensions to MIME types and back via `mime_guess = "2"`. Extension-only; magic-byte detection out of scope. Three-level preference resolution (user → runtime → crate) handles canonical tie-breaking for `to_extension`.

## Motivation

Scripts serving files or processing uploads need extension↔MIME mapping. Surfaced from `lib:http` static-file serving and future upload handlers: filename → `Content-Type`; `Content-Type` → file extension.

Without this module: hard-coded mappings or direct vendored crate usage — fragile, inconsistent with curated module surface.

Originating issue: [flying-dice/neoc#33](https://github.com/flying-dice/neoc/issues/33)

## Detailed design

### Detection strategy: extension-only

| Runtime | Strategy |
|---|---|
| Deno `@std/media-types` | Extension-only. IANA + Apache `mime.types` database. |
| Bun built-in | Extension-only for `Bun.file().type`. |
| Go `mime` (stdlib) | Extension-only. System MIME DB (Unix) / registry (Windows). |
| Python `mimetypes` | Extension-only + optional database. |
| Node (no stdlib) | npm `mime` extension-only; `file-type` magic-bytes separate. |
| Rust `infer` | Magic bytes only. |
| Rust `tree_magic_mini` | Magic bytes + MIME hierarchy. |
| Rust `mime_guess` | Extension-only. IANA + Apache, compiled in. |

Magic-byte detection requires file I/O → cannot detect from filename alone. All runtimes working on filenames use extension-only. Magic-byte detection is a separate concern with different I/O implications → future module (`lib:magic` or file-reading option) when needed.

### Backing crate: `mime_guess`

| Crate | Strategy | Compile-time DB | MIME→ext | Notes |
|---|---|---|---|---|
| `mime_guess` | Extension | Yes (IANA + Apache) | Yes | Zero I/O, no system deps, WASM-compatible |
| `mime` | Parsing only | — | No | Types only; no lookup DB |
| `infer` | Magic bytes | No | No | Requires file content |
| `tree_magic_mini` | Magic bytes | No | No | Requires file content; system DB optional |

Only Rust crate that is extension-based + compiled-in DB + bidirectional. No system deps, WASM-compatible, tracks IANA + Apache — same sources as Deno and Go.

### API surface

Three synchronous functions, `(value, err)` tuple convention ([ADR 0005](/adrs/0005-tuple-error-convention)):

```lua
local mime = require("lib:mime")

-- Extension to MIME type. Accepts ".json" or "json" (leading dot optional).
-- Returns (string, nil) on success, (nil, string) on failure.
local mt, err = mime.from_extension(".json")   --> "application/json", nil

-- MIME type to canonical extension. Optional user preference table.
-- Resolution order: user prefs → runtime prefs → mime_guess.
-- Returns (string, nil) on success, (nil, string) on failure.
local ext, err = mime.to_extension("text/html")           --> ".html", nil
local ext, err = mime.to_extension("text/html", { ["text/html"] = ".htm" })  --> ".htm", nil

-- Filename-based detection. Handles compound extensions (e.g. "file.tar.gz").
-- Returns (string, nil) on success, (nil, string) on failure.
local mt, err = mime.detect("archive.tar.gz")  --> "application/gzip", nil
```

Unknown extensions / unparseable MIME strings → recoverable `err`. Wrong type → raises (programming error).

### Extension preference resolution (`to_extension`)

`mime_guess` returns alphabetically-first extension → unnatural results: `image/jpeg` → `.jfif` not `.jpg`; `text/html` → `.htm` not `.html`. All peer runtimes curate a preference list.

Three-level resolution:

1. **User preference table** (optional arg) — `{ ["mime/type"] = ".ext" }`. Highest precedence.
2. **Runtime preference table** — static compiled-in overrides where `mime_guess` default is non-canonical (`.html` not `.htm`, `.jpg` not `.jfif`, `.js` not `.es`).
3. **`mime_guess` fallback** — alphabetical-first from compiled DB.

### Async posture

Synchronous throughout. No I/O, never yields. Consistent with ADR 0006 hidden-async policy.

## Drawbacks

- No magic-byte detection. Missing/misleading extensions require a second module when that use case arises. Intentional scope boundary.
- Runtime preference list is compile-time constant → new entries require code change + release.

## Alternatives

- **Do nothing** — scripts hard-code or use `vnd:mime_guess` directly. Inconsistent, duplicated, no preference layer.
- **Hard-coded defaults only** — inflexible; no path for callers wanting `.htm` for legacy reasons.
- **External database file** — runtime-configurable like Go's system MIME DB. External dependency, inappropriate for compile-everything-in runtime.
- **Include magic-byte detection** — broadens scope, introduces I/O, mixes concerns. All peer runtimes treat these as separate.

## Open questions

_None._

## Implementation notes

- Add `mime_guess = "2"` to `Cargo.toml`.
- Create `src/lua/lib/mime.rs` with `pub fn module(lua: &Lua) -> mlua::Result<Table>`.
- Register in `src/lua/lib/mod.rs`.
- Runtime preference table: `phf::Map` or `match` block — whichever clearer at implementation time.
- `lib:http` and future upload handlers depend on `lib:mime` for `Content-Type` resolution without additional deps.
