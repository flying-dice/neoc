# `lib:mime` module

**Stable**

The **`lib:mime`** module maps between file extensions, filenames, and MIME type strings. It wraps [`mime_guess`](https://crates.io/crates/mime_guess) with a curated preference layer so canonical extensions are consistent regardless of what `mime_guess` returns first.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/mime.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/mime.rs) |
| Tests | [`tests/lib/mime.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/mime.test.luau) |
| Stability | Stable |

## Syntax

```lua
local mime = require("lib:mime")
```

## Description

All three functions follow the standard neoc fallible-function convention: they return two values, `(result, err)`. On success `result` is the string answer and `err` is `nil`. On failure `result` is `nil` and `err` is a string beginning with `"<function>:"` — for example `"mime.from_extension: unknown extension: .xyz"`.

A built-in *runtime preference map* sits in front of `mime_guess` for a handful of types where the alphabetically-first candidate is not what users expect — for instance `image/jpeg` maps to `.jpg`, not `.jfif`. The preference layer is applied by `to_extension` only; `from_extension` and `detect` delegate directly to `mime_guess`.

## Module functions

#### mime.from_extension

```lua
mime.from_extension(ext: string): (string?, string?)
```

Returns the MIME type for the given file extension. The leading dot is optional — both `".json"` and `"json"` resolve to `"application/json"`.

Returns `(nil, err)` when the extension is not recognised.

#### mime.to_extension

```lua
mime.to_extension(mime_type: string, prefs?: table): (string?, string?)
```

Returns the canonical file extension (with leading dot) for the given MIME type string. Extension selection follows this precedence:

1. **User preference map** — the optional `prefs` table, keyed by MIME type string. Takes highest priority.
2. **Runtime preference map** — the built-in curated overrides (e.g. `image/jpeg` → `".jpg"`).
3. **`mime_guess` fallback** — the first extension returned by `mime_guess`.

Returns `(nil, err)` when `mime_type` cannot be parsed as a valid MIME type, or when no extension is known for it.

#### mime.detect

```lua
mime.detect(filename: string): (string?, string?)
```

Returns the MIME type inferred from the extension of `filename`. The filename may be a bare name (`"style.css"`), a path (`"assets/images/logo.png"`), or a compound name (`"archive.tar.gz"`). Only the extension is examined — file contents are not read.

Returns `(nil, err)` when no extension can be resolved to a MIME type.

## Errors

All errors are strings with the function name as prefix, followed by a colon and a description:

| Function | Example error |
|---|---|
| `mime.from_extension` | `"mime.from_extension: unknown extension: .xyz"` |
| `mime.to_extension` | `"mime.to_extension: no extension known for: application/x-custom"` |
| `mime.detect` | `"mime.detect: cannot determine MIME type for: Makefile"` |

## Examples

### Extension to MIME type

```lua
local mime = require("lib:mime")

local result, err = mime.from_extension(".json")
-- result == "application/json", err == nil

local result2, err2 = mime.from_extension("html")
-- result2 == "text/html", err2 == nil

local result3, err3 = mime.from_extension(".xyz")
-- result3 == nil, err3 == "mime.from_extension: unknown extension: .xyz"
```

### MIME type to extension

```lua
local mime = require("lib:mime")

local result, err = mime.to_extension("image/jpeg")
-- result == ".jpg", err == nil  (runtime preference map)

local result2, err2 = mime.to_extension("application/json")
-- result2 == ".json", err2 == nil

-- Override with user preferences
local prefs = { ["text/html"] = ".xhtml" }
local result3, err3 = mime.to_extension("text/html", prefs)
-- result3 == ".xhtml", err3 == nil
```

### Filename detection

```lua
local mime = require("lib:mime")

local result, err = mime.detect("style.css")
-- result == "text/css", err == nil

local result2, err2 = mime.detect("archive.tar.gz")
-- result2 == "application/gzip", err2 == nil

local result3, err3 = mime.detect("Makefile")
-- result3 == nil, err3 == "mime.detect: cannot determine MIME type for: Makefile"
```

## Acceptance

The following scenarios must hold. They are exercised by the integration tests under [`tests/lib/mime.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/mime.test.luau).

1. **Dot-prefixed and bare extensions are equivalent.** `mime.from_extension(".json")` and `mime.from_extension("json")` both return `"application/json"`.
2. **Unknown extension returns `(nil, err)`.** The error string begins with `"mime.from_extension:"`.
3. **Empty string returns `(nil, err)`.** `mime.from_extension("")` is an unknown extension.
4. **Dot-only input returns `(nil, err)`.** `mime.from_extension(".")` resolves to an empty bare extension, which is unknown.
5. **Runtime preference map produces canonical extensions.** `mime.to_extension("image/jpeg")` returns `".jpg"`, not `".jfif"`.
6. **User preference map takes highest priority.** Passing `{ ["text/html"] = ".xhtml" }` returns `".xhtml"` for `text/html`.
7. **User prefs fall through.** A MIME type absent from the user prefs table resolves via the runtime preference map or `mime_guess`.
8. **Invalid MIME type returns `(nil, err)`.** The error string begins with `"mime.to_extension:"`.
9. **`detect` resolves by extension only.** File contents are not read; only the extension of the filename determines the MIME type.
10. **`detect` with no recognisable extension returns `(nil, err)`.** The error string begins with `"mime.detect:"`.
11. **Empty string and dot-only inputs to `detect` return `(nil, err)`.** Neither has a recognisable extension.

## See also

- [The module system](/guides/module-system) — How `require("lib:mime")` resolves.
- [`lib:*`](/lib/) — Other modules in this namespace.
