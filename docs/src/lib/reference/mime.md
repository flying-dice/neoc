# `lib:mime`

**Stable**

Maps between file extensions, filenames, and MIME type strings. Wraps [`mime_guess`](https://crates.io/crates/mime_guess) with a curated preference layer for canonical extensions.

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/mime.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/mime.rs) |
| Tests | [`crates/neoc/tests/lib/mime.test.lua`](https://github.com/flying-dice/neoc/blob/main/crates/neoc/tests/lib/mime.test.lua) |
| Stability | Stable |

## Syntax

```lua
local mime = require("lib:mime")
```

## Description

All functions return `(result, err)`. On success: result is string, err is `nil`. On failure: result is `nil`, err prefixed with function name.

Runtime preference map applied by `to_extension` only (e.g. `image/jpeg` → `.jpg` not `.jfif`). `from_extension` and `detect` delegate directly to `mime_guess`.

## Module functions

#### mime.from_extension

```lua
mime.from_extension(ext: string): (string?, string?)
```

Returns MIME type for extension. Leading dot optional — `".json"` and `"json"` both resolve to `"application/json"`.

#### mime.to_extension

```lua
mime.to_extension(mime_type: string, prefs?: table): (string?, string?)
```

Returns canonical extension (with dot) for MIME type. Precedence:

1. User `prefs` table (keyed by MIME type)
2. Runtime preference map
3. `mime_guess` fallback

#### mime.detect

```lua
mime.detect(filename: string): (string?, string?)
```

Returns MIME type inferred from filename extension. Accepts bare names, paths, compound names (`archive.tar.gz`). Extension only — no content inspection.

## Errors

| Function | Example |
|---|---|
| `mime.from_extension` | `"mime.from_extension: unknown extension: .xyz"` |
| `mime.to_extension` | `"mime.to_extension: no extension known for: application/x-custom"` |
| `mime.detect` | `"mime.detect: cannot determine MIME type for: Makefile"` |

## Examples

### Extension to MIME

```lua
local mime = require("lib:mime")

local result, err = mime.from_extension(".json")
-- "application/json", nil

local result2, err2 = mime.from_extension(".xyz")
-- nil, "mime.from_extension: unknown extension: .xyz"
```

### MIME to extension

```lua
local mime = require("lib:mime")

local result, err = mime.to_extension("image/jpeg")
-- ".jpg", nil  (runtime preference)

local prefs = { ["text/html"] = ".xhtml" }
local result2, err2 = mime.to_extension("text/html", prefs)
-- ".xhtml", nil
```

### Filename detection

```lua
local mime = require("lib:mime")

local result, err = mime.detect("style.css")        -- "text/css"
local result2, err2 = mime.detect("archive.tar.gz") -- "application/gzip"
local result3, err3 = mime.detect("Makefile")       -- nil, err
```

## Acceptance

Exercised by [`crates/neoc/tests/lib/mime.test.lua`](https://github.com/flying-dice/neoc/blob/main/crates/neoc/tests/lib/mime.test.lua):

1. Dot-prefixed and bare extensions equivalent.
2. Unknown extension → `(nil, err)`.
3. Empty string → `(nil, err)`.
4. Dot-only → `(nil, err)`.
5. Runtime prefs produce canonical extensions (`image/jpeg` → `.jpg`).
6. User prefs take highest priority.
7. User prefs fall through to runtime/mime_guess.
8. Invalid MIME type → `(nil, err)`.
9. `detect` resolves by extension only.
10. `detect` with no extension → `(nil, err)`.
11. Empty/dot-only inputs to `detect` → `(nil, err)`.

## See also

- [The module system](/guides/module-system)
- [`lib:*`](/lib/)
