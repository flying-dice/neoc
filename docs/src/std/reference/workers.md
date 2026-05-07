# `std:workers` module

**Stable**

The **`std:workers`** module exposes process-level coordination to scripts. Its primary surface is [`workers.on_shutdown()`](#workers-on-shutdown) — an observable signal that fires on Ctrl-C / SIGINT so long-running primitives can exit cleanly. The pool itself is sized at process startup; `std:workers` does not expose pool construction or worker lifecycle to scripts.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/workers.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/workers.rs) |
| Tests | In-source `#[cfg(test)] mod tests` in [`workers.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/workers.rs) |
| Stability | Stable |
| Mirror | tokio runtime — no `std::` equivalent |

## Syntax

```lua
local workers = require("std:workers")
```

The returned table exposes the module functions described below.

## Description

neoc runs scripts in a pool of independent Lua virtual machines — *workers*. Each worker is a self-contained interpreter: globals, locals, upvalues, and any userdata constructed inside it are private to that worker. The pool is sized at process startup and tasks dispatch across workers transparently. Script authors do not pick which worker they run on.

`std:workers` provides process-level coordination primitives that operate above the individual worker boundary. Currently that means one thing: observing when the process is shutting down so scripts can clean up gracefully.

For cross-worker data sharing, see [`std:thread`](/std/reference/thread) which provides message-passing between workers via channels.

For background on the worker model, see [The sandboxing model](/guides/sandboxing-model#the-worker-model).

## Module functions

#### workers.on_shutdown

```lua
workers.on_shutdown(): ShutdownSignal
```

Returns a process-singleton signal that fires once when the process receives a shutdown request (Ctrl-C or SIGINT). Repeated calls return references to the same underlying signal.

#### workers.shutdown_signal (deprecated)

```lua
workers.shutdown_signal(): ShutdownSignal
```

Legacy alias for [`workers.on_shutdown()`](#workers-on-shutdown). Prefer `on_shutdown()` in new code.

## ShutdownSignal methods

`ShutdownSignal:is_fired()`
:   Returns `true` once the signal has fired, `false` otherwise. Non-blocking.

`ShutdownSignal:wait()`
:   Resumes when the signal fires. Returns immediately if the signal has already fired. Safe to call from multiple workers and multiple coroutines concurrently.

## Examples

### Graceful shutdown

The following example blocks the main task until Ctrl-C, then closes a long-running server.

```lua
local workers = require("std:workers")
local hyper   = require("vnd:hyper")
local net     = require("std:net")

local server = hyper.serve(net.listener("0.0.0.0:3000"), function(req)
    return { status = 200, body = "ok" }
end)

workers.on_shutdown():wait()
server:close()
server:wait()
```

### Polling shutdown in a loop

```lua
local workers = require("std:workers")
local signal  = workers.on_shutdown()

while not signal:is_fired() do
    -- do periodic work
end
-- cleanup
```

## Acceptance

The following scenarios must hold. They are exercised by the in-source `#[cfg(test)] mod tests` block in [`workers.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/workers.rs).

1. **Singleton identity.** `workers.on_shutdown()` called multiple times returns handles to the same underlying signal.
2. **Alias equivalence.** `workers.shutdown_signal()` and `workers.on_shutdown()` return the same signal.
3. **Idempotent wait.** Calling `signal:wait()` multiple times after the signal has fired returns immediately each time.
4. **is_fired reflects state.** `signal:is_fired()` returns `false` before the signal fires and `true` after.

## See also

- [The sandboxing model](/guides/sandboxing-model#the-worker-model) — How the worker pool fits into the broader engine.
- [`std:thread`](/std/reference/thread) — Worker spawning and message-passing channels.
