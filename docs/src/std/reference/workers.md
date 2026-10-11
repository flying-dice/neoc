# `std:workers`

**Stable**

Process-level coordination. Primary surface: [`workers.on_shutdown()`](#workers-on-shutdown) — observable signal on Ctrl-C/SIGINT for graceful exit. Pool sized at startup; construction/lifecycle not exposed to scripts.

| Property | Value |
|---|---|
| Namespace | `std` |
| Source | [`src/lua/std/workers.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/std/workers.rs) |
| Tests | In-source `#[cfg(test)]` in `workers.rs` |
| Stability | Stable |
| Mirror | tokio runtime (no `std::` equivalent) |

## Syntax

```lua
local workers = require("std:workers")
```

## Description

neoc runs scripts in a pool of independent Lua VMs — *workers*. Each worker: private globals, locals, upvalues, userdata. Pool sized at startup, tasks dispatch transparently.

`std:workers` provides process-level primitives above the worker boundary. Currently: shutdown observation.

Cross-worker data: [`std:thread`](/std/reference/thread). Background: [sandboxing model](/guides/sandboxing-model#the-worker-model).

## Module functions

#### workers.on_shutdown

```lua
workers.on_shutdown(): ShutdownSignal
```

Returns process-singleton signal that fires on shutdown (Ctrl-C/SIGINT). Repeated calls return same signal.

#### workers.shutdown_signal (deprecated)

```lua
workers.shutdown_signal(): ShutdownSignal
```

Legacy alias for `on_shutdown()`. Prefer `on_shutdown()`.

## ShutdownSignal methods

`ShutdownSignal:is_fired()`
:   `true` once fired, `false` otherwise. Non-blocking.

`ShutdownSignal:wait()`
:   Resumes on fire. Returns immediately if already fired. Safe from multiple workers/coroutines.

## Examples

### Graceful shutdown

```lua
local workers = require("std:workers")
local hyper   = require("vnd:hyper")
local net     = require("std:net")

local server = hyper.serve(net.TcpListener.bind("0.0.0.0:3000"), function(req)
    return { status = 200, body = "ok" }
end)

workers.on_shutdown():wait()
server:close()
server:wait()
```

### Polling in a loop

```lua
local workers = require("std:workers")
local signal  = workers.on_shutdown()

while not signal:is_fired() do
    -- periodic work
end
-- cleanup
```

## Acceptance

Exercised by in-source `#[cfg(test)]` in `workers.rs`:

1. **Singleton identity.** Multiple `on_shutdown()` calls → same signal.
2. **Alias equivalence.** `shutdown_signal()` and `on_shutdown()` → same signal.
3. **Idempotent wait.** `wait()` after fire returns immediately.
4. **is_fired reflects state.** `false` before fire, `true` after.

## See also

- [The sandboxing model](/guides/sandboxing-model#the-worker-model)
- [`std:thread`](/std/reference/thread)
