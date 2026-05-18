# Error conventions

How neoc modules signal failure. Two shapes, when each applies, message format, handling patterns.

## Two shapes of failure

`(value, err)` tuple
:   On success: `value` holds result, `err` is `nil`. On failure: `value` is `nil`, `err` is a string. Used for *recoverable* failures — conditions scripts inspect and react to.

Raised error
:   Calls `error(...)`, propagates until caught by `pcall`/`xpcall`. Used for *programming errors* — script bugs, not runtime conditions.

Per-function shape is documented on its module reference page. A function consistently uses one shape or the other, never both.

## When each applies

**Tuple return** — failure the caller might reasonably recover from: I/O failures (`std:fs.read_to_string` on missing file), parse failures (`vnd:serde_json.from_str` on malformed input), network failures (`vnd:hyper.get` on unreachable host).

**Raise** — script misused the surface: wrong argument type, method on closed handle, non-Sendable in `shared` table. These are bugs; suppressing them masks the bug.

## Error message format

Every error begins with a qualified prefix: `"module.function:"` (no namespace prefix).

```text
fs.read_to_string: No such file or directory (os error 2)
serde_json.from_str: expected value at line 1 column 1
hyper.get: connection refused
thread.spawn: shared.k is 'JsonNull', which is not Sendable. Use args for by-value transfer, or use a Sendable type (Map, Counter, TcpListener).
```

Consistent across every module. Prefix is matchable programmatically.

## Handling failures

### Tuple return

```lua
local fs = require("std:fs")

local body, err = fs.read_to_string("config.json")
if err then
    return
end
-- use `body`
```

Same pattern everywhere. No status codes, no exception objects, no wrapper types.

### Raised error

```lua
local thread = require("std:thread")
local json   = require("lib:json")

local ok, err = pcall(function()
    thread.spawn({ source = "return 1", shared = { v = json.null } })
end)
if not ok then
    -- err contains qualified message
end
```

Use `pcall` when genuine recovery from a programming error is needed — test runners, top-level supervisors.

## Suggestions and enrichment

When an error involves a name lookup (module, method, field), the runtime provides "did you mean" suggestions using Levenshtein edit distance with threshold `max(1, name.len() / 3)`.

**Structure:**

```text
module.function: what went wrong[ — did you mean "X"?][ available: a, b, c]
```

**Rules:**

- Suggestions appear only when edit distance is within threshold — never garbage matches.
- Suggestions never echo the user's original input.
- Available options are enumerated when the candidate set has ≤ 10 items.
- Type mismatch errors list accepted types: `"expected SQL string, got table — accepted: string"`.

All suggestion computation lives in the error path. Zero cost on the happy path.

### CLI diagnostics

Errors that surface through the CLI (not through Lua `(nil, string)` tuples) are rendered with source context, error codes, and help text using `miette`. Each error code links to a docs page.

```text
  × require: unknown module "lib:jsn"
  help: did you mean "lib:json"?
        available: lib:base64, lib:compile, lib:http, lib:ini, lib:json, lib:mime, lib:test, lib:vcr

  neoc::E0001 — https://neoc.dev/errors/E0001
```

Error codes are flat-namespaced (`neoc::ENNNN`), sequential, and stable across versions. See [error code reference](/errors/).

## See also

- [The sandboxing model](/guides/sandboxing-model)
- [The module system](/guides/module-system) — how `require` signals failure
- [`std:fs`](/std/reference/fs) — tuple-returning module
- [`std:thread`](/std/reference/thread) — raises on misuse
