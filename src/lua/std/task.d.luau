--!strict
--
-- Type definitions for `std:task` — paired with `src/lua/std/task.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Cooperative task scheduler
--
-- Five primitives matching the Roblox `task` library: `spawn`, `defer`,
-- `delay`, `wait`, `cancel`. Each scheduling function returns a `Task`
-- handle with `:cancel()`, `:is_finished()`, and `:await()` methods.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Handle returned by `task.spawn`, `task.defer`, and `task.delay`.
    Mirrors `tokio::JoinHandle` — cancel, poll, or block on the result.

    ```lua
    local task = require("std:task")
    local h = task.spawn(function() return 42 end)
    local result = h:await()
    ```
]=]
export type Task = {
    --[=[
        Cancel the task. Returns `true` if this call was the one that
        cancelled; `false` if already finished or previously cancelled.
    ]=]
    cancel: (self: Task) -> boolean,

    --[=[
        Returns `true` if the task has completed (success, error, or cancel).
    ]=]
    is_finished: (self: Task) -> boolean,

    --[=[
        Wait for the task to finish and return its result.

        Returns `(values...)` on success.
        Returns `(nil, error_message)` on error.
        Returns `(nil, "cancelled")` if cancelled.
    ]=]
    await: (self: Task) -> (any, string?),
}

--[=[
    The module table returned by `require("std:task")`.
]=]
export type TaskModule = {
    --[=[
        Spawn a function or coroutine. The first synchronous slice runs
        immediately; if it yields, the continuation runs as a separate
        tokio task. Returns a `Task` handle.

        ```lua
        local task = require("std:task")
        local h = task.spawn(function(a, b)
            task.wait(0.1)
            return a + b
        end, 1, 2)
        local result = h:await()  -- 3
        ```
    ]=]
    spawn: (fn_or_thread: (() -> ...any) | thread, ...any) -> Task,

    --[=[
        Queue a function to run after the current synchronous batch.
        Deferred tasks execute in FIFO order. Returns a `Task` handle.
    ]=]
    defer: (fn_or_thread: (() -> ...any) | thread, ...any) -> Task,

    --[=[
        Schedule a function to run after `secs` seconds.
        `delay(0, ...)` is semantically equivalent to `defer(...)`.
        Returns a `Task` handle.
    ]=]
    delay: (secs: number, fn_or_thread: (() -> ...any) | thread, ...any) -> Task,

    --[=[
        Yield the current coroutine for `secs` seconds. Negative values
        are clamped to 0. Returns the actual elapsed time.

        Must be called from within a spawned/deferred task.
    ]=]
    wait: (secs: number) -> number,

    --[=[
        Cancel a task by handle. Flat Roblox-compatible wrapper around
        `handle:cancel()`. Returns `true` if this call cancelled the task.
    ]=]
    cancel: (handle: Task) -> boolean,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local task = require("std:task")` resolves `task` to `TaskModule`.
declare function require(name: "std:task"): TaskModule
