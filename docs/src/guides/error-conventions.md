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

## See also

- [The sandboxing model](/guides/sandboxing-model)
- [The module system](/guides/module-system) — how `require` signals failure
- [`std:fs`](/std/reference/fs) — tuple-returning module
- [`std:thread`](/std/reference/thread) — raises on misuse
