---
status: accepted
created: 2026-05-18
implemented-in:
---

# 0010 — `lib:csv` — CSV parsing and writing

## Summary

`lib:csv` exposes parse/stringify, a handle-based streaming reader (`csv.reader`), a lazy string iterator (`csv.rows`), and an incremental writer (`csv.writer`) — RFC 4180, configurable delimiter (TSV via `delimiter = "\t"`), schema-driven type coercion, no auto-inference. Backed by the `csv` (BurntSushi) crate.

## Motivation

CSV is the second-most-common data interchange format after JSON: test result exports, coverage reports, benchmark data, log exports, metrics dumps. Without `lib:csv`, scripts shell out or hand-roll a parser — neither acceptable for a self-contained runtime.

Cross-runtime survey:

| Runtime | Module | Notes |
|---|---|---|
| Go | `encoding/csv` | Reader/Writer, configurable delimiter, `[][]string`, errors include line number |
| Python | `csv` | DictReader/DictWriter, no type coercion (pandas wraps for that) |
| Deno | `@std/csv` | `parse(data, { skipFirstRow, columns })`, `stringify(data, { columns })` |
| Node | — | None built-in; npm `csv-parse` / `papaparse` (Papa's `dynamicTyping` widely copied) |
| Lune | — | Not shipped |

Concrete scenarios:

- Parse a benchmark export to filter failing rows (`csv.parse`)
- Stream a multi-GB log dump from disk without loading it into memory — via `csv.reader(handle)` over a `std:fs` file handle
- Lazily iterate parsed rows from an already-in-memory string (`csv.rows(data)`)
- Generate a coverage CSV deterministically for downstream tooling
- Read TSV (`delimiter = "\t"`) without a second module

**Streaming clarification.** Two distinct things, two APIs:
- `csv.reader(handle, opts)` — handle-based, reads incrementally from any `ReaderHandle` (file, socket, pipe). True I/O streaming; the file is never fully resident. Mirrors Rust `csv::Reader::from_reader`, Go `csv.NewReader`, Python `csv.reader`.
- `csv.rows(data, opts)` — string-based, lazy *parsing* over an in-memory buffer. The bytes are already loaded; only the parsed row table is allocated lazily. Use when you already hold the data (e.g. from an HTTP body).

## Detailed design

### Lua API

```lua
local csv = require("lib:csv")

-- Parse string to array of arrays
local rows = csv.parse("name,age\nAlice,30\nBob,25")
--> {{"name","age"}, {"Alice","30"}, {"Bob","25"}}

-- Parse with headers (returns array of tables keyed by column name)
local records = csv.parse("name,age\nAlice,30\nBob,25", { headers = true })
--> {{name="Alice", age="30"}, {name="Bob", age="25"}}

-- Parse with schema-driven type coercion
local records = csv.parse(data, {
    headers = true,
    types   = { duration = "number", passed = "number", active = "boolean" },
})
-- records[1].duration --> 1.23 (number, not "1.23")
-- Untyped columns remain strings. Supported types: "number", "boolean".

-- Full options
csv.parse(data, {
    delimiter = ",",    -- field separator (default ","); use "\t" for TSV
    headers   = false,  -- treat first row as headers
    quote     = '"',    -- quote character
    escape    = '"',    -- escape character
    comment   = nil,    -- comment line prefix
    types     = {},     -- per-column type coercion map
})

-- Lazy iterator over an in-memory string (data already loaded)
for row in csv.rows(data, { headers = true }) do
    if row.status == "FAIL" then
        process_failure(row)
    end
end

-- Handle-based streaming reader (multi-GB files, sockets, pipes)
local fs   = require("std:fs")
local file = fs.open({ path = "huge.csv", mode = "r" })  --> ReaderHandle
local r    = csv.reader(file, { headers = true })
for row in r:rows() do
    process(row)  -- file is read incrementally; never fully resident
end

-- Stringify (headers array controls both header row and output order)
csv.stringify({{name="Alice", age="30"}}, { headers = {"name", "age"} })
--> "name,age\nAlice,30\n"

-- Incremental writer handle
local writer = csv.writer({ headers = {"test", "status", "duration"} })
writer:row({"test_login",  "PASS", "1.23"})
writer:row({"test_signup", "FAIL", "0.45"})
local output = writer:finish()
```

### Function signatures

**`csv.parse(data[, opts]) → (table, nil) | (nil, string)`**

| Param | Type | Required | Description |
|---|---|---|---|
| `data` | `string` | yes | CSV bytes |
| `opts` | `table?` | no | See options table below |

**`csv.rows(data[, opts]) → iterator`**

Returns a Lua iterator over an in-memory string. Each call returns the next row (array or table per `headers`), or `nil` at EOF. Errors raise (not tuple) — iterators cannot return `(nil, err)` ergonomically; raise matches `pairs`/`ipairs` shape. Convenience wrapper over `csv.reader(std:io.from_string(data), opts):rows()`.

**`csv.reader(handle[, opts]) → reader`**

| Param | Type | Required | Description |
|---|---|---|---|
| `handle` | `ReaderHandle` | yes | Stream source from `std:fs.open` (mode `"r"`), `std:io.from_string`, or any future `ReaderHandle` producer |
| `opts` | `table?` | no | Same as `csv.parse` / `csv.rows` |

Methods:

- `reader:headers() → ({string}, nil) | (nil, string)` — return the header row. **Cached**, mirrors Rust `csv::Reader::headers`: the first call (or the first row consumed via `:rows()` when `headers = true`) parses and stores the headers; all subsequent calls return the cached value regardless of reader position. Errors if `headers ~= true`. Safe to call before or after `:rows()`.
- `reader:rows() → iterator` — Lua iterator yielding the next row on each call, `nil` at EOF. Same raise-on-error semantics as `csv.rows`. The reader holds an exclusive borrow on the handle for its lifetime (see Handle borrow enforcement below).

**Handle borrow enforcement.** `csv.reader(handle, opts)` takes an exclusive borrow on the `ReaderHandle` for the lifetime of the returned reader. A second `csv.reader` call with the same handle while the first reader is alive returns a tuple error — no deadlock, no interleaved reads. The borrow flag clears on reader GC (or explicit `:close()` if added in v2). Implementation flips a boolean on the `ReaderHandle` UserData; check is O(1).

Mirrors Rust `csv::Reader::from_reader` / `csv::Reader::records` / `csv::Reader::headers`. The reader is the primary streaming API; `csv.parse` and `csv.rows` are convenience shims for in-memory strings.

**`csv.stringify(records, opts) → (string, nil) | (nil, string)`**

| Param | Type | Required | Description |
|---|---|---|---|
| `records` | `table` | yes | Array of records (tables keyed by column name) |
| `opts.headers` | `{string}` | yes | Column order; also emitted as header row |
| `opts.delimiter` | `string` | no | Default `","` |
| `opts.quote` | `string` | no | Default `'"'` |

**`csv.writer(opts) → writer`**

| Param | Type | Required | Description |
|---|---|---|---|
| `opts.headers` | `{string}` | yes | Column order; emitted as first row on `:finish()` |
| `opts.delimiter` | `string` | no | Default `","` |
| `opts.quote` | `string` | no | Default `'"'` |

Methods:

- `writer:row(values)` — append a row. `values` is **either** an array (positional, matches `headers` order) **or** a table keyed by header name. Returns `(true, nil)` on success, `(nil, string)` on shape error.

  Shape rules (matches backing crate's default `flexible(false)`):

  | Input | Behaviour |
  |---|---|
  | Array shape, length == `#headers` | Written positionally |
  | Array shape, length ≠ `#headers` | Tuple error: `csv.writer: row has N values, expected M (headers length)` |
  | Keyed shape, every header key present | Written in `headers` order |
  | Keyed shape, missing key for a header | Field emitted as empty string (matches `stringify` convention) |
  | Mixed (numeric + string keys, e.g. `{1, 2, name="Alice"}`) | Tuple error: `csv.writer: row values must be array or keyed table, not both` |
- `writer:finish() → string` — flush and return the accumulated CSV string. Subsequent calls error.

### Options reference

| Option | Type | Default | Applies to | Description |
|---|---|---|---|---|
| `delimiter` | `string` | `","` | parse, rows, stringify, writer | Field separator. Use `"\t"` for TSV. Must be a single byte; multi-byte → error. |
| `headers` | `boolean` \| `{string}` | parse: `false`; stringify/writer: required | parse, rows: `boolean`; stringify, writer: `{string}` | On parse: if `true`, first row becomes header names and records are keyed tables. On stringify/writer: explicit column order. |
| `quote` | `string` | `'"'` | all | Quote character. Single byte. |
| `escape` | `string` | `'"'` | parse, rows | Escape character inside quoted fields. Single byte. |
| `comment` | `string?` | `nil` | parse, rows | Lines starting with this byte are skipped. Single byte. |
| `types` | `table` | `{}` | parse, rows | Per-column type coercion: `{ [column_name] = "number" \| "boolean" }`. Untyped columns remain strings. Requires `headers = true`. |

### Error semantics

All errors are tuple-form `(nil, string)` except `csv.rows` / `reader:rows()` iterators (raise).

Error message format follows the repo convention (`docs/src/rfcs/0003-errors-guide.md`): `module.function: <reason>` — no namespace prefix. Line numbers 1-indexed, matches Go `encoding/csv`. Parse errors append the raw source bytes of the offending line truncated to 80 bytes (UTF-8 boundary safe): `<reason>: "<line>"`. Source bytes are captured byte-for-byte from the underlying stream (preserving original quoting and escaping); the snippet, line number, and field counts are all exact. Line truncated mid-field gets a trailing `…`.

| Condition | Message |
|---|---|
| Mismatched field count | `csv.parse: line 47: found 3 fields, expected 5: "a,b,c"` |
| Unterminated quote | `csv.parse: line 12: unterminated quoted field: "\"alice,30…"` |
| Duplicate header names | `csv.parse: line 1: duplicate header "name": "name,age,name"` |
| `types` set without `headers` | `csv.parse: 'types' requires 'headers = true'` |
| Type coercion failed | `csv.parse: line 5: column "duration" cannot coerce "abc" to number: "test_login,abc,PASS"` |
| Boolean coercion accepts | `"true"`, `"false"`, `"1"`, `"0"` (case-insensitive); anything else → error |
| Multi-byte `delimiter`/`quote`/`escape`/`comment` | `csv.parse: 'delimiter' must be a single byte` |
| `stringify` missing `headers` | `csv.stringify: 'headers' is required` |
| `stringify` record missing keyed column | Field emitted as empty string (matches RFC 4180 empty-field semantics) |
| `writer:row` mixed table | `csv.writer: row values must be array or keyed table, not both` |
| `writer:row` array length ≠ headers length | `csv.writer: row has 2 values, expected 3 (headers length)` |
| `writer:row` keyed table missing a header key | Field for missing header emitted as empty string (matches `stringify`) |
| `writer:finish()` called twice | `csv.writer: already finished` |
| `reader:headers()` without `headers = true` | `csv.reader: 'headers' option must be true to call :headers()` |
| `reader` handle is not a `ReaderHandle` | `csv.reader: argument must be a ReaderHandle` |
| `reader` handle already borrowed by another reader | `csv.reader: handle is already in use by another reader` |

`csv.rows` and `reader:rows()` errors raise with the same message body (including raw line), no tuple. `csv.parse` errors use the `csv.parse:` prefix regardless of internal call chain. Options-validation errors (no line context, e.g. multi-byte `delimiter`) omit the `line N:` segment and the raw line.

### Empty fields

`"a,,c"` → `{"a", "", "c"}` (untyped). Empty strings, never `nil` — `nil` in a Lua array breaks `#` and `ipairs`. The `csv` crate returns empty strings natively, no conversion.

**Carve-out for typed columns.** When `types[col]` is set, an empty field for that column produces `nil`, not `""`. `types` requires `headers = true`, so results are keyed tables (`{name="Alice", age=nil}`); `nil` in a keyed table is the Lua-idiomatic "absent value" and is safe — `#` and `ipairs` don't apply. The array-safety rule above only governs positional (untyped) results. This avoids a silent mixed-type return from an explicitly typed column (principle 2: no footguns).

Summary:

| Source | Result |
|---|---|
| Untyped column, empty field | `""` |
| Typed column (`types[col]` set), empty field | `nil` |
| Typed column, non-empty field | Coerced value |
| Typed column, non-empty field that fails coercion | Tuple error |

### Type coercion

Schema-driven only. No `types = true` auto-inference — silently corrupts zip codes, phone numbers, IDs that look numeric.

Supported types:

| Type | Coercion |
|---|---|
| `"number"` | `str::parse::<f64>()`. Failure → tuple error with line + column. |
| `"boolean"` | `"true"`/`"1"` → `true`; `"false"`/`"0"` → `false` (case-insensitive). Failure → tuple error. |

Empty field in a typed column → `nil` (see [Empty fields](#empty-fields)). Callers handle absence as `if row.age ~= nil then`. Coercion failure on a non-empty value still errors via tuple form.

### Acceptance criteria

- [ ] `csv.parse` — parse CSV string to rows or records
- [ ] `csv.rows` — lazy iterator over an in-memory string
- [ ] `csv.reader` — handle-based streaming reader (`ReaderHandle` from `std:fs` / `std:io`); `:headers()`, `:rows()` methods
- [ ] `csv.stringify` — write records; `headers` array controls column order
- [ ] `csv.writer` — incremental writer with `:row()` and `:finish()`; rejects mixed array+keyed tables
- [ ] Configurable `delimiter`, `quote`, `escape`, `comment`
- [ ] RFC 4180: quoted fields, escaped quotes, multiline values
- [ ] Schema-driven type coercion (`"number"`, `"boolean"`); untyped → string
- [ ] Errors use `module.function:` prefix per `0003-errors-guide` — no `lib:` namespace prefix
- [ ] Parse errors include line number, expected vs found column count, and the raw source bytes of the offending line truncated to 80 bytes (`<reason>: "<line>"`); quoting and escaping preserved verbatim
- [ ] Duplicate header names → hard error, not silent shadow
- [ ] Untyped empty field → `""`; typed empty field → `nil`
- [ ] `csv.reader` enforces exclusive borrow on the `ReaderHandle`; second concurrent reader on the same handle → tuple error, no deadlock
- [ ] `reader:headers()` cached — first call (or first `:rows()` consumption) parses and stores; subsequent calls return cached value
- [ ] `writer:row` array length mismatch → tuple error; keyed missing key → empty string field
- [ ] Spec + unit tests + type stub

## Drawbacks

- Surface adds five entry points (`parse`, `rows`, `reader`, `stringify`, `writer`) vs a leaner `encode`/`decode` pair. Streaming I/O, lazy-parse, and incremental writing each have a distinct shape and don't fold into the symmetric encode/decode used by `lib:compress` / `lib:base64`.
- Reader is handle-based (`csv.reader(handle)`); writer is in-memory accumulator (`csv.writer({headers})`). The asymmetry is deliberate — handle-based writer is deferred until `std:io` exposes a buffer sink (so the in-memory case has a clean expression). Tracked as a v2 candidate in [Alternatives](#alternatives).
- `types` map requires headers — coupling two options that are otherwise independent.
- Iterators (`csv.rows`, `reader:rows()`) raise instead of tuple-erroring, diverging from the rest of the module. Required by Lua iterator protocol.
- Writer state (`:finish` consumes) adds a footgun — well-typed but easy to misuse in long-lived scripts.

## Alternatives

**Do nothing.** Forces every caller to shell out or hand-roll. Rejected — CSV is too common.

**Symmetric `encode`/`decode` only (no streaming, no writer).** Cleaner surface; matches `lib:compress`. Rejected — log/benchmark exports can be multi-GB. Forcing whole-file load is a non-starter.

**`filter` callback on parse.** Considered. Rejected — `csv.rows` iterator composes with any userland logic; callback only covers one pattern.

**Auto type inference (`types = true`).** Considered. Rejected — silently corrupts zip codes, phone numbers, leading-zero IDs. Papa Parse precedent is not worth the data loss.

**`csv.parse_tsv` alias.** Rejected — `delimiter = "\t"` is one option, not a second function.

**`stringify` auto-detects headers from first record's keys.** Rejected — Lua table key order is not guaranteed. Deterministic output requires explicit `headers`.

**Handle-based writer (`csv.writer(handle, opts)`) in v1.** Considered. Deferred — requires a `std:io.buffer()` sink (or similar) to express the in-memory case cleanly. Without one, callers writing to a string would need extra ceremony for the common path. v1 keeps the in-memory accumulator; v2 will add `csv.writer(handle, opts)` once `std:io` grows a buffer sink, restoring full read/write handle symmetry.

**`csv.reader` returning the iterator directly (no `:rows()` method).** Considered. Rejected — the reader needs more than one method (`:headers()` for explicit header access, future `:position()`/`:close()`); collapsing it to a single iterator callable forecloses that surface.

**Single `csv.rows` accepting both string and handle.** Rejected — overloads on first-arg type are surprising in Lua and conflate two distinct cost models (lazy parse vs I/O streaming). Two named functions make the choice explicit at the call site.

## Open questions

_None._

## Implementation notes

- Backing crate: `csv` (BurntSushi) — RFC 4180 compliant, streaming reads native, position info on errors.
- Module location: `src/lib/csv/` (per existing `lib:*` layout).
- `csv.reader(handle, opts)` takes an exclusive borrow on the `ReaderHandle` and wraps it with `csv::Reader::from_reader`. Returns a `CsvReader` `UserData`. The `ReaderHandle` gains a `borrowed: AtomicBool` field (set on borrow, cleared on `CsvReader` drop). `csv.reader` checks the flag first; if already set → tuple error `csv.reader: handle is already in use by another reader`. No `MutexGuard` is held across calls — only the inner trait object is moved into the reader, which holds it for its lifetime. This eliminates both deadlock and interleaved-read failure modes.
- `csv.rows(data, opts)` is a thin wrapper: constructs an internal `Cursor`-backed `ReaderHandle` (or reuses `std:io.from_string`) and delegates to `csv.reader(…):rows()`. Same `csv::Reader::records()` underneath; no separate code path. The handle is owned by the iterator closure, so the borrow flag is irrelevant in this path.
- `reader:headers()` is backed by `csv::Reader::headers()` directly — the backing crate already caches the headers on first parse and returns a borrowed `StringRecord` thereafter. We clone into a fresh Lua table per call. Calling `:headers()` before `:rows()` triggers the initial parse; calling it after triggers no extra I/O (cached). Documented to match Rust semantics 1:1.
- `csv.writer` wraps `csv::Writer<Vec<u8>>`; `:finish()` consumes and returns `String::from_utf8`. Built with `flexible(false)` so column-count mismatches surface as errors rather than producing ragged output.
- `writer:row` shape detection: walk keys once. `1..n` integer keys only → array; string keys only → keyed lookup against `headers`; both present → tuple error. Array length compared to `#headers`; mismatch → tuple error with both counts. Keyed shape: iterate `headers`, look up each in the table; absent key → empty string field (matches `stringify`).
- Parse errors append the raw source bytes of the offending line truncated to 80 bytes. Truncation walks UTF-8 boundaries via `str::is_char_boundary`; truncated lines get a trailing `…`. Line numbers come from `csv::Error::position()` / `csv::ByteRecord::position()`. Source bytes come from a bounded rolling capture (`RawCapture`, 64 KiB) fed by a `Read` wrapper around the underlying source; the error path slices `[record.byte(), reader.byte())` and strips the trailing record terminator. If the bytes have aged out of the capture (cap exceeded — only possible for records longer than 64 KiB) the line falls back to a best-effort reconstruction from parsed fields.
- Single-byte option validation happens at the Lua→Rust boundary, before constructing the `ReaderBuilder` / `WriterBuilder`. Options-validation errors omit the `line N:` segment and raw line (no position context).
- Error prefixes follow `0003-errors-guide` — `module.function:` form, set via `from_err(lua, "csv.parse", …)` etc.
- Type stub for `lib:csv` (mirror existing module stubs).
