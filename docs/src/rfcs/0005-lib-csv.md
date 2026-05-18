---
status: draft
created: 2026-05-18
implemented-in:
---

# 0005 — `lib:csv` — CSV parsing and writing

> [!WARNING]
> This RFC is `draft`. Under refinement — **not** the contract.

## Summary

`lib:csv` exposes parse/stringify and streaming reader/writer handles for CSV — RFC 4180, configurable delimiter (TSV via `delimiter = "\t"`), schema-driven type coercion, no auto-inference. Backed by the `csv` (BurntSushi) crate.

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

- Parse a benchmark export to filter failing rows
- Stream a multi-GB log dump without loading it into memory
- Generate a coverage CSV deterministically for downstream tooling
- Read TSV (`delimiter = "\t"`) without a second module

## Detailed design

### Luau API

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

-- Streaming iterator (large files — avoids loading everything)
for row in csv.rows(data, { headers = true }) do
    if row.status == "FAIL" then
        process_failure(row)
    end
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

Returns a Luau iterator. Each call returns the next row (array or table per `headers`), or `nil` at EOF. Errors raise (not tuple) — iterators cannot return `(nil, err)` ergonomically; raise matches `pairs`/`ipairs` shape.

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

- `writer:row(values)` — append a row. `values` is an array (positional, matches `headers` order) or a table (keyed by header name).
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

All errors are tuple-form `(nil, string)` except `csv.rows` iterator (raises).

Error message format: `lib:csv: <op> error at line N: <reason>` — line numbers 1-indexed, matches Go `encoding/csv`.

| Condition | Message |
|---|---|
| Mismatched field count | `lib:csv: parse error at line 47: found 3 fields, expected 5` |
| Unterminated quote | `lib:csv: parse error at line 12: unterminated quoted field` |
| Duplicate header names | `lib:csv: parse error at line 1: duplicate header "name"` |
| `types` set without `headers` | `lib:csv: parse error: 'types' requires 'headers = true'` |
| Type coercion failed | `lib:csv: parse error at line 5: column "duration" cannot coerce "abc" to number` |
| Boolean coercion accepts | `"true"`, `"false"`, `"1"`, `"0"` (case-insensitive); anything else → error |
| Multi-byte `delimiter`/`quote`/`escape`/`comment` | `lib:csv: invalid option: 'delimiter' must be a single byte` |
| `stringify` missing `headers` | `lib:csv: stringify error: 'headers' is required` |
| `stringify` record missing keyed column | Field emitted as empty string (matches RFC 4180 empty-field semantics) |
| `writer:finish()` called twice | `lib:csv: writer already finished` |

### Empty fields

`"a,,c"` → `{"a", "", "c"}`. Empty strings, never `nil`. `nil` in a Lua array breaks `#` and `ipairs`; the `csv` crate returns empty strings natively, no conversion.

### Type coercion

Schema-driven only. No `types = true` auto-inference — silently corrupts zip codes, phone numbers, IDs that look numeric.

Supported types:

| Type | Coercion |
|---|---|
| `"number"` | `str::parse::<f64>()`. Failure → tuple error with line + column. |
| `"boolean"` | `"true"`/`"1"` → `true`; `"false"`/`"0"` → `false` (case-insensitive). Failure → tuple error. |

Empty string is preserved as empty string even when type-coerced — coercion is opt-in per column, but empties stay empty (matches `NULL`-equivalent semantics in CSV). Callers handle empties explicitly.

### Acceptance criteria

- [ ] `csv.parse` — parse CSV string to rows or records
- [ ] `csv.rows` — streaming iterator for large-file use
- [ ] `csv.stringify` — write records; `headers` array controls column order
- [ ] `csv.writer` — incremental writer with `:row()` and `:finish()`
- [ ] Configurable `delimiter`, `quote`, `escape`, `comment`
- [ ] RFC 4180: quoted fields, escaped quotes, multiline values
- [ ] Schema-driven type coercion (`"number"`, `"boolean"`); untyped → string
- [ ] Errors include line number, expected vs found column count, raw line (truncated)
- [ ] Duplicate header names → hard error, not silent shadow
- [ ] Empty fields → empty strings, never `nil`
- [ ] Spec + unit tests + `.d.luau` type stub

## Drawbacks

- Surface adds four entry points (`parse`, `rows`, `stringify`, `writer`) vs a leaner `encode`/`decode` pair. Streaming and incremental writing don't fold cleanly into the symmetric encode/decode shape used by `lib:compress` / `lib:base64`.
- `types` map requires headers — coupling two options that are otherwise independent.
- Iterator `csv.rows` raises instead of tuple-erroring, diverging from the rest of the module. Required by Luau iterator protocol.
- Writer state (`:finish` consumes) adds a footgun — well-typed but easy to misuse in long-lived scripts.

## Alternatives

**Do nothing.** Forces every caller to shell out or hand-roll. Rejected — CSV is too common.

**Symmetric `encode`/`decode` only (no streaming, no writer).** Cleaner surface; matches `lib:compress`. Rejected — log/benchmark exports can be multi-GB. Forcing whole-file load is a non-starter.

**`filter` callback on parse.** Considered. Rejected — `csv.rows` iterator composes with any userland logic; callback only covers one pattern.

**Auto type inference (`types = true`).** Considered. Rejected — silently corrupts zip codes, phone numbers, leading-zero IDs. Papa Parse precedent is not worth the data loss.

**`csv.parse_tsv` alias.** Rejected — `delimiter = "\t"` is one option, not a second function.

**`stringify` auto-detects headers from first record's keys.** Rejected — Lua table key order is not guaranteed. Deterministic output requires explicit `headers`.

## Open questions

_None._

## Implementation notes

- Backing crate: `csv` (BurntSushi) — RFC 4180 compliant, streaming reads native, position info on errors.
- Module location: `src/lib/csv/` (per existing `lib:*` layout).
- `csv.rows` wraps `csv::Reader::records()` lazily; do not materialise into a `Vec`.
- `csv.writer` wraps `csv::Writer<Vec<u8>>`; `:finish()` consumes and returns `String::from_utf8`.
- Single-byte option validation happens at the Luau→Rust boundary, before constructing the `ReaderBuilder` / `WriterBuilder`.
- Type stub: `docs/src/lib/csv.d.luau` (mirror existing module stubs).
