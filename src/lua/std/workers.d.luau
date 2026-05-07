--!strict
--
-- Type definitions for `std:workers` — paired with `src/lua/std/workers.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Process-level coordination
--
-- `workers.on_shutdown()` returns a process-global `ShutdownSignal` that
-- fires on Ctrl-C. Long-running primitives (e.g. `hyper.serve`'s `wait()`)
-- listen on this to exit cleanly.
--
-- `workers.shutdown_signal()` is a deprecated alias for `on_shutdown()`.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Process-global shutdown signal. One instance exists per process; every
    call to `on_shutdown()` returns the same underlying signal.

    Fires when the CLI receives SIGINT (Ctrl-C) on Unix, or the equivalent
    console event on Windows.

    ```lua
    local workers = require("std:workers")
    local sig = workers.on_shutdown()

    -- check without blocking
    if sig:is_fired() then
        -- already shutting down
    end

    -- block until fired
    sig:wait()
    ```
]=]
export type ShutdownSignal = {
    --[=[
        Return `true` if the signal has already fired, `false` otherwise.
        Non-blocking.
    ]=]
    is_fired: (self: ShutdownSignal) -> boolean,

    --[=[
        Yield the calling coroutine until the signal fires. If the signal
        has already fired, returns immediately.
    ]=]
    wait: (self: ShutdownSignal) -> (),
}

--[=[
    The module table returned by `require("std:workers")`.
]=]
export type WorkersModule = {
    --[=[
        Return the process-global `ShutdownSignal`. Fires on Ctrl-C (SIGINT).
        Preferred over the deprecated `shutdown_signal()` alias.

        ```lua
        local workers = require("std:workers")
        local hyper   = require("vnd:hyper")
        local net     = require("std:net")

        local listener = net.TcpListener.bind("0.0.0.0:3000")
        local server   = hyper.serve(listener, handler)
        workers.on_shutdown():wait()  -- block until Ctrl-C
        server:close()
        ```
    ]=]
    on_shutdown: () -> ShutdownSignal,

    --[=[
        Deprecated alias for `on_shutdown()`. Prefer `on_shutdown()` in new
        code.
    ]=]
    shutdown_signal: () -> ShutdownSignal,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local workers = require("std:workers")` resolves `workers` to `WorkersModule`.
declare function require(name: "std:workers"): WorkersModule
