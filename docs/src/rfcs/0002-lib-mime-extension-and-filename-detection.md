---
status: draft
created: 2026-05-04
implemented-in:
---

# 0002 — `lib:mime` — extension-to-MIME mapping via `mime_guess`, extension preference layer

> [!WARNING]
> This RFC is `draft`. It is under refinement and is **not** the contract.

## Summary

Add a `lib:mime` module that maps file extensions to MIME types and back, backed by the `mime_guess = "2"` crate. Extension-only detection; magic-byte detection is explicitly out of scope. A three-level preference resolution (user → runtime → crate) handles canonical tie-breaking for `to_extension`.

## Motivation

`neoc` scripts that serve files or process uploads need to map file extensions to MIME types and back. The need surfaced directly from `lib:http`'s static-file serving path and any future upload handler: you receive a filename and need to set `Content-Type`; you receive a `Content-Type` header and need to write a file.

Without a dedicated module, scripts must either hard-code MIME mappings or pull in a vendored crate directly — both are fragile and inconsistent with the curated module surface `neoc` provides.

Originating issue: [flying-dice/neoc#33](https://github.com/flying-dice/neoc/issues/33)

## Detailed design

### Detection strategy: extension-only

Peer runtimes land in two camps:

| Runtime | Detection strategy |
|---|---|
| Deno `@std/media-types` | Extension-only. Backed by a large hand-maintained database derived from the IANA registry and Apache's `mime.types`. |
| Bun built-in | Extension-only for `Bun.file().type`. |
| Go `mime` (stdlib) | Extension-only. System MIME database on Unix; registry on Windows. |
| Python `mimetypes` | Extension-only with an optional database. |
| Node (no stdlib module) | npm `mime` is extension-only; `file-type` does magic-byte detection and is separate. |
| Rust `infer` crate | Magic bytes only. |
| Rust `tree_magic_mini` | Magic bytes, MIME hierarchy. |
| Rust `mime_guess` | Extension-only. IANA + Apache database, compiled in. |

Magic-byte detection requires reading file content, which means I/O — you cannot detect a type from a filename alone. Every runtime that must work on a filename or extension without reading the file uses extension-only detection. Magic-byte detection is a separate capability with different I/O implications.

`lib:mime`'s declared purpose (map extensions to MIME types and vice versa, detect from filename) is satisfied entirely by extension-based detection. Magic-byte detection belongs to a future module (e.g. `lib:magic` or as an option on a file-reading module) when a concrete use case requires it.

### Backing crate: `mime_guess`

| Crate | Strategy | Compile-time DB | MIME→extension | Notes |
|---|---|---|---|---|
| `mime_guess` | Extension | Yes (IANA + Apache) | Yes | Zero I/O, no system deps, WASM-compatible |
| `mime` | Parsing only | — | No | Types only; no lookup DB |
| `infer` | Magic bytes | No | No | Requires file content |
| `tree_magic_mini` | Magic bytes | No | No | Requires file content; system DB optional |

`mime_guess` is the only crate in the Rust ecosystem that is extension-based, has a compiled-in database, and supports both directions (extension→MIME and MIME→extension). It has no system dependencies, is WASM-compatible, and its database tracks the IANA registry and Apache `mime.types` — the same sources Deno and Go use.

### API surface

Three functions, all synchronous, all following the `(value, err)` tuple convention ([ADR 0005](/adrs/0005-tuple-error-convention)):

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

Unknown extensions and unparseable MIME strings are recoverable failures returned in the `err` slot; passing the wrong type is a programming error and raises.

### Extension preference resolution (`to_extension`)

`mime_guess` resolves MIME→extension by returning the alphabetically first entry from its database. This produces unnatural extensions in common cases: `image/jpeg` → `.jfif` rather than `.jpg`; `text/html` → `.htm` rather than `.html`. Every peer runtime resolves this with a curated preference list.

`to_extension` applies a three-level resolution:

1. **User preference table** (optional second argument) — a plain Lua table, `{ ["mime/type"] = ".ext" }`. Takes precedence over everything.
2. **Runtime preference table** — a static list compiled into the module for types where `mime_guess`'s alphabetical-first entry is not the canonical expectation (`.html` not `.htm`, `.jpg` not `.jfif`, `.js` not `.es`, etc.).
3. **`mime_guess` fallback** — alphabetical-first extension from the compiled-in database.

### Async posture

`lib:mime` is synchronous throughout. MIME detection from a filename requires no I/O and never yields. This is consistent with ADR 0006's hidden-async policy: a module that does no async work does not expose async functions.

## Drawbacks

- Magic-byte detection — reading the first N bytes of a file to identify its type — is not covered by this module. A script that must identify a binary whose extension is missing or misleading cannot use `lib:mime` for that purpose. This is an intentional scope boundary, but it means a second module will be needed when that use case arises.
- The runtime preference list is a compile-time constant. Adding entries requires a code change and a release, not a runtime configuration update.

## Alternatives

- **Do nothing.** Scripts hard-code MIME mappings or use `vnd:mime_guess` directly. Inconsistent, duplicated, no canonical preference layer.
- **Hard-coded module defaults only** — simple, no API surface. Inflexible: a caller wanting `.htm` for legacy reasons has no path.
- **External database file** — configurable at runtime, like Go's system MIME database. Higher flexibility, external dependency, not appropriate for a runtime that compiles everything in.
- **Include magic-byte detection** — broadens scope, introduces I/O, mixes two distinct concerns. Every peer runtime treats these as separate modules.

## Open questions

_None._

## Implementation notes

- Add `mime_guess = "2"` to `Cargo.toml`.
- Create `src/lua/lib/mime.rs` with the standard `pub fn module(lua: &Lua) -> mlua::Result<Table>` entry point.
- Register in `src/lua/lib/mod.rs`.
- The runtime preference table is a `phf::Map` or a `match` block — whichever is clearer at implementation time.
- `lib:http` and any future upload handler can depend on `lib:mime` for `Content-Type` resolution without taking on additional dependencies.
