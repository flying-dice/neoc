--!strict
--
-- Type definitions for `std:thread` — paired with `src/lua/std/thread.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Worker VMs
--
-- Spawn a fresh Lua engine on the same Tokio runtime as the parent. Modelled
-- after Node's `worker_threads`: pass source (or a file path) plus optional
-- args and shared Sendable state. Communicate via a bidirectional `Channel`
-- exposed as `thread.parent` inside workers.
--
-- ## Sendable protocol
--
-- `thread.spawn` accepts an optional `shared = {…}` table. Every value must
-- be a Sendable userdata: `Map`, `Counter`, or `TcpListener`. Plain tables,
-- strings, numbers, and non-Sendable userdata are rejected at spawn time with
-- a clear error. Use `args` for by-value transfer of plain data.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Handle to a spawned worker VM, as seen from the parent.

    ```lua
    local thread = require("std:thread")
    local w = thread.spawn({ source = "return 42" })
    local result, err = w:await()
    ```
]=]
export type WorkerHandle = {
    --[=[
        Send a JSON-serialisable value to the worker's message queue.
        The worker receives it via `thread.parent:receive()`.

        Errors if the worker has already exited.
    ]=]
    send: (self: WorkerHandle, value: any) -> (),

    --[=[
        Wait for a message from the worker.
        Yields the calling coroutine until a message arrives.

        Returns `(value, nil)` on success; `(nil, "thread.receive: ...")` when
        the worker has exited and no more messages will arrive.
    ]=]
    receive: (self: WorkerHandle) -> (any, string?),

    --[=[
        Wait for the worker to finish and return its result.
        Yields the calling coroutine until the worker exits.

        Returns `(result, nil)` on success; `(nil, "thread.await: ...")` if the
        worker errored, was terminated, or was already awaited.
    ]=]
    await: (self: WorkerHandle) -> (any, string?),

    --[=[
        Abort the worker immediately. No return value.
        A subsequent `:await()` returns `(nil, "thread.await: terminated")`.
    ]=]
    terminate: (self: WorkerHandle) -> (),
}

--[=[
    The worker's end of the parent↔worker message channel.
    Exposed as `(require("std:thread")).parent` inside workers. `nil` at the
    top level and in workers that didn't come from `thread.spawn`.
]=]
export type Channel = {
    --[=[
        Send a JSON-serialisable value to the parent's message queue.
        The parent receives it via `worker:receive()`.
    ]=]
    send: (self: Channel, value: any) -> (),

    --[=[
        Wait for a message from the parent.
        Yields until a message arrives or the parent exits.

        Returns `(value, nil)` on success; `(nil, "channel.receive: ...")` when
        the parent has exited.
    ]=]
    receive: (self: Channel) -> (any, string?),
}

--[=[
    Options table for `thread.spawn`.
]=]
export type SpawnOpts = {
    --[=[
        Inline Luau source string for the worker. Mutually exclusive with `file`.
    ]=]
    source: string?,

    --[=[
        Path to a `.luau` file to load as the worker script.
        Mutually exclusive with `source`.
    ]=]
    file: string?,

    --[=[
        Optional name for the worker, used in stack traces and logs.
        Defaults to the file path (when `file` is set) or `"thread:worker"`.
    ]=]
    name: string?,

    --[=[
        Single JSON-serialisable value passed as the first argument to the
        worker script. Use a table for multiple named args (Node `workerData`
        convention). `nil` if omitted.
    ]=]
    args: any?,

    --[=[
        Shared Sendable userdata. Every value must implement the Sendable
        protocol (`Map`, `Counter`, `TcpListener`). Rejected at spawn time with
        a clear error otherwise. Workers access this via `thread.shared`.
    ]=]
    shared: {[string]: any}?,
}

--[=[
    The module table returned by `require("std:thread")`.

    At the top level, `parent` and `shared` are `nil`. Inside a spawned
    worker, `parent` is the `Channel` back to the spawner and `shared` is the
    reconstructed table of Sendable values (or `nil` if none were passed).
]=]
export type ThreadModule = {
    --[=[
        Spawn a new Lua VM as a worker on the same Tokio runtime.

        Returns `(WorkerHandle, nil)` on success; `(nil, "thread.spawn: ...")` if
        the source is unreadable, the shared table contains non-Sendable values,
        or no Tokio runtime is in scope.

        ```lua
        local thread  = require("std:thread")
        local Counter = require("std:collections").Counter

        local c = Counter.new()
        local w = thread.spawn({
            source = [[
                local shared = (require("std:thread")).shared
                shared.c:increment()
            ]],
            shared = { c = c },
        })
        w:await()
        assert(c:get() == 1)
        ```
    ]=]
    spawn: (opts: SpawnOpts) -> (WorkerHandle?, string?),

    --[=[
        Return the number of logical CPU cores available to the process.
        Useful for sizing worker pools. Returns at least 1.
    ]=]
    available_parallelism: () -> number,

    --[=[
        The `Channel` to the parent VM. `nil` at the top level; set to a
        `Channel` inside spawned workers.
    ]=]
    parent: Channel?,

    --[=[
        The reconstructed table of Sendable values passed via `shared` at spawn
        time. `nil` at the top level and in workers that received no shared state.
    ]=]
    shared: {[string]: any}?,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local thread = require("std:thread")` resolves `thread` to `ThreadModule`.
declare function require(name: "std:thread"): ThreadModule
