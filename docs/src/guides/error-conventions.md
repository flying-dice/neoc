# Error conventions

This guide describes how neoc modules signal failure to Lua scripts. It covers the two failure-signalling shapes the runtime uses, when each one applies, how error messages are formatted, and how scripts handle them. Familiarity with Lua's `error`, `pcall`, and multi-value return is assumed.

## Two shapes of failure

A function in neoc signals failure in one of two ways.

`(value, err)` tuple
:   The function returns two values. On success, `value` holds the result and `err` is `nil`. On failure, `value` is `nil` and `err` is a string describing the failure. This shape is used for *recoverable* failures — conditions a script is expected to inspect and react to.

Raised error
:   The function calls `error(...)`, propagating up the stack until caught by `pcall` or `xpcall`. This shape is used for *programming errors* — conditions that indicate a script bug rather than a runtime condition the script should handle.

The choice of shape is documented per function on its module's reference page. The two shapes are not interchangeable: a function either consistently uses `(value, err)` or it consistently raises.

## When each shape applies

A function returns the `(value, err)` tuple when its failure mode is something the caller might reasonably want to inspect and recover from. Examples include I/O failures (`std:fs.read_to_string` on a missing file), parser failures (`vnd:serde_json.from_str` on malformed input), and network failures (`vnd:hyper.get` against an unreachable host).

A function raises when its failure mode indicates the script has misused the surface — passing an argument of the wrong type, calling a method on a closed handle, or passing a non-Sendable userdata in the `shared` table of `thread.spawn`. These are bugs in the script, not runtime conditions, and pretending otherwise would mask the bug.

## Error message format

Every error message — whether returned in the `err` slot of a tuple or raised — begins with a qualified prefix that names the module and function that produced it. The prefix has the form `"module.function:"` (no namespace prefix in the message itself).

```text
fs.read_to_string: No such file or directory (os error 2)
serde_json.from_str: expected value at line 1 column 1
hyper.get: connection refused
thread.spawn: shared.k is 'JsonNull', which is not Sendable. Use args for by-value transfer, or use a Sendable type (Map, Counter, TcpListener).
```

This format is consistent across every module. Scripts can match against the prefix programmatically, and readers of stack traces can locate the failing call site without context.

## Handling failures in scripts

### Inspecting a tuple return

A script handles a recoverable failure by inspecting the second return value.

```lua
local fs = require("std:fs")

local body, err = fs.read_to_string("config.json")
if err then
    -- handle the failure: log, retry, fall back, etc.
    return
end
-- use `body`
```

The pattern is identical across every function that returns the tuple shape. There is no separate status code, no exception object, and no wrapping type to unpack.

### Catching a raised error

A script catches a raised error with `pcall` (protected call). The standard Lua idiom applies unchanged.

```lua
local thread = require("std:thread")
local json   = require("lib:json")

local ok, err = pcall(function()
    thread.spawn({ source = "return 1", shared = { v = json.null } })   -- json.null is not Sendable
end)
if not ok then
    -- err contains the qualified message
end
```

Scripts should reach for `pcall` when they genuinely need to recover from a programming error — for example, in a test runner or in a top-level supervisor that should report the failure rather than crash the worker.

## See also

- [The sandboxing model](/guides/sandboxing-model) — Where errors fit into the broader engine model.
- [The module system](/guides/module-system) — How `require` itself signals failure.
- [`std:fs`](/std/reference/fs) — Example of a tuple-returning module.
- [`std:workers`](/std/reference/workers) — Process-level coordination (shutdown signal).
- [`std:thread`](/std/reference/thread) — Example of a module that raises on misuse (non-Sendable values in `shared`).
