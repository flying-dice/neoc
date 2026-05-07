--!strict
--
-- Type definitions for `std:collections` — paired with
-- `src/lua/std/collections.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Concurrent data structures
--
-- All three types implement the `Sendable` protocol: they can be passed to
-- spawned workers via `thread.spawn({ shared = {…} })` and both VMs share
-- the same underlying `Arc<Inner>`. Primitive Lua values and plain tables
-- cannot be shared this way.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    A concurrent key-value map backed by a `RwLock<HashMap<String, JsonValue>>`.

    Values round-trip via `serde_json::Value`, so any JSON-serialisable Lua
    value (strings, numbers, booleans, tables, nested tables) can be stored.
    Lua userdata (e.g. `JsonObject`) cannot — serialize first.

    `Map` implements the Sendable protocol and can be shared across workers.

    ```lua
    local Map = require("std:collections").Map
    local m = Map.new()
    m:set("key", "value")
    print(m:get("key"))  -- "value"
    ```
]=]
export type Map = {
    --[=[
        Set `key` to `value`. Overwrites any existing entry.
        `value` may be any JSON-serialisable Lua type.
    ]=]
    set: (self: Map, key: string, value: any) -> (),

    --[=[
        Get the value for `key`. Returns `nil` if the key is absent.
    ]=]
    get: (self: Map, key: string) -> any,

    --[=[
        Remove `key`. Returns `true` if the key existed, `false` if absent.
    ]=]
    delete: (self: Map, key: string) -> boolean,

    --[=[
        Returns `true` if `key` is present in the map.
    ]=]
    has: (self: Map, key: string) -> boolean,

    --[=[
        Number of entries in the map.
    ]=]
    size: (self: Map) -> number,

    --[=[
        Returns all keys as a 1-indexed Lua table of strings.
        Order is unspecified (HashMap — not sorted).
    ]=]
    keys: (self: Map) -> {string},

    --[=[
        Remove all entries from the map.
    ]=]
    clear: (self: Map) -> (),
}

--[=[
    A thread-safe integer counter backed by an `AtomicI64`.

    All operations are atomic with `Relaxed` memory ordering — suitable for
    counting, not for sequencing other memory operations.

    `Counter` implements the Sendable protocol and can be shared across workers.

    ```lua
    local Counter = require("std:collections").Counter
    local c = Counter.new(0)
    c:increment()
    print(c:get())  -- 1
    ```
]=]
export type Counter = {
    --[=[
        Return the current value.
    ]=]
    get: (self: Counter) -> number,

    --[=[
        Add 1 and return the **new** value.
    ]=]
    increment: (self: Counter) -> number,

    --[=[
        Subtract 1 and return the **new** value.
    ]=]
    decrement: (self: Counter) -> number,

    --[=[
        Add `n` (may be negative) and return the **new** value.
    ]=]
    add: (self: Counter, n: number) -> number,

    --[=[
        Atomically store `n` as the new value. No return value.
    ]=]
    set: (self: Counter, n: number) -> (),
}

--[=[
    A yielding async mutex for coroutine coordination.

    `Mutex` holds no data — it's a coordination gate. Use it to protect a
    critical section across coroutines without blocking the worker thread.
    Acquiring the lock yields the calling coroutine until the lock is free;
    other coroutines on the same engine continue running.

    Unlike `Map` and `Counter`, `Mutex` is **not** currently Sendable and
    cannot be passed to spawned workers via `shared`.

    ```lua
    local Mutex = require("std:collections").Mutex
    local mu = Mutex.new()

    mu:with_lock(function()
        -- critical section — only one coroutine runs here at a time
    end)
    ```
]=]
export type Mutex = {
    --[=[
        Acquire the lock, run `fn` with no arguments, then release the lock.
        Yields the calling coroutine until the lock is acquired.
        The lock is released even if `fn` errors; the error propagates to
        the caller after release.

        Returns whatever `fn` returned (as a tuple).
    ]=]
    with_lock: (self: Mutex, fn: () -> ...any) -> ...any,
}

--[=[
    Constructor tables for the three collection types.
]=]
export type MapConstructor = {
    --[=[
        Construct an empty `Map`.
    ]=]
    new: () -> Map,
}

export type CounterConstructor = {
    --[=[
        Construct a `Counter` with an optional initial value (defaults to 0).
    ]=]
    new: (initial: number?) -> Counter,
}

export type MutexConstructor = {
    --[=[
        Construct an unlocked `Mutex`.
    ]=]
    new: () -> Mutex,
}

--[=[
    The module table returned by `require("std:collections")`.
]=]
export type CollectionsModule = {
    --[=[
        Constructor table for `Map`. Call `.new()` to create an instance.

        ```lua
        local m = (require("std:collections")).Map.new()
        ```
    ]=]
    Map: MapConstructor,

    --[=[
        Constructor table for `Counter`. Call `.new(initial?)` to create an instance.

        ```lua
        local c = (require("std:collections")).Counter.new(0)
        ```
    ]=]
    Counter: CounterConstructor,

    --[=[
        Constructor table for `Mutex`. Call `.new()` to create an instance.

        ```lua
        local mu = (require("std:collections")).Mutex.new()
        ```
    ]=]
    Mutex: MutexConstructor,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local collections = require("std:collections")` resolves to `CollectionsModule`.
declare function require(name: "std:collections"): CollectionsModule
