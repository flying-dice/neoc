---
status: draft
created: 2026-05-03
implemented-in:
---

# 0001 — Sendable concurrency model

> [!WARNING]
> This RFC is `draft`. It is under refinement and is **not** the contract.

## Summary

Replace the process-global, string-keyed `workers.shared` registry with a spawn-time handoff model. Types that can cross engine boundaries declare that capability by implementing an internal `Sendable` trait. `thread.spawn` gains a `shared = {…}` table whose values are validated at spawn time — non-Sendable types are rejected with a clear error. The global registry, `SharableUserData`, and all discovery-by-string-key machinery are deleted.

## Motivation

The current cross-engine sharing model (`std:workers`) has four structural problems:

1. **Mesh sharing graph.** Any engine publishes; any engine retrieves. No canonical construction site. Reading a script tells you nothing about where a handle came from.
2. **Type erasure via `Arc<dyn Any + Send + Sync>`.** The registry downcasts at runtime. Type safety is advisory.
3. **Stringly-typed discovery.** Engines rendezvous on string keys. Typos compile. Types don't help.
4. **Naming lies about identity.** `MapHandle` is a concurrent hash map. `CounterHandle` is an atomic integer. The names hide what they are behind an implementation detail.

These problems compound. A script author debugging a cross-engine issue must trace string keys across files to find who registered what, hope the downcast matches, and mentally rename `MapHandle` to "actually this is a concurrent map." The model works — it shipped — but it doesn't meet the bar set by the project's design principles: no surprises (principle 1), no footguns (principle 2), mirror Rust (principle 3).

The replacement model draws from Swift's Structured Concurrency. The core insight: **shareability is a property of a type, not its identity**. `Map` is `Map`. Whether it's `Sendable` is an internal property, enforced at the spawn boundary. You don't need a naming convention or a registry when the runtime will tell you immediately if you get it wrong.

## Detailed design

### Design tenets

1. **Types are named for what they are.** `Map` is `Map`. `Counter` is `Counter`. `TcpListener` is `TcpListener`. Whether a type can cross an engine boundary is an internal property — not a naming convention, not a namespace, not a prefix.

2. **Namespace by primary abstraction.** A thread-safe map is still a collection — it lives in `std:collections`. A network listener lives in `std:net`. The `std:sync` namespace (if it exists at all) is reserved exclusively for synchronisation primitives: locks, atomics, channels, barriers. The question "what is this type?" determines its home, not "what does it guarantee?"

3. **Cross-engine capability is a protocol, not a convention.** Types opt into cross-engine sharing by implementing the `Sendable` trait. The spawn-time machinery checks this — no string keys, no naming conventions, no runtime registry. If a type isn't `Sendable`, the error names the type and the constraint.

4. **The sharing graph is a DAG rooted at spawn.** State crosses engines exactly once, at spawn time, in one direction. Whoever constructed a resource decides who receives a handle to it. Workers can forward to their own children — privilege is local, scoped to what each engine was handed. No mesh, no global rendezvous.

5. **Lua GC replaces lifetime tracking.** Each Sendable userdata holds `Arc<Inner>`. When the last clone across all engines is GC'd, `Inner` deallocates. No explicit `:close()` required except where the resource has external state needing predictable teardown (e.g. a TCP listener).

6. **Spawn-time validation, not runtime discovery.** The moment `thread.spawn` is called, the runtime walks the `shared` table and validates every entry. Violations are caught immediately — not deferred to first use, not silent.

### Lua surface

```lua
-- main.lua
local Map      = require("std:collections").Map
local Counter  = require("std:collections").Counter
local Listener = require("std:net").TcpListener

local sessions = Map.new()
local hits     = Counter.new(0)
local listener = Listener.bind("0.0.0.0:8080")

-- Spawn-time: the runtime checks that every value in `shared`
-- conforms to the Sendable protocol. If it doesn't, spawn fails
-- with a clear error: "type 'JsonObject' is not Sendable"
for i = 1, 4 do
    thread.spawn({
        file   = "worker.lua",
        args   = { worker_id = i },
        shared = { listener = listener, sessions = sessions, hits = hits },
    })
end
```

```lua
-- worker.lua
local args   = ...
local shared = thread.shared  -- { listener, sessions, hits }

while true do
    local conn = shared.listener:accept()
    shared.hits:increment()
    shared.sessions:set(conn:addr(), { connected_at = os.time() })
    handle(conn)
end
```

**What does NOT work:**

```lua
local obj = json.parse('{"a":1}')
thread.spawn({ shared = { data = obj } })
-- → Error: type 'JsonObject' is not Sendable
```

### Type naming and namespaces

Types are named for what they **are**, not how they're used. No `Concurrent*` prefix. No `Sync*` prefix. Types live in the modules where they semantically belong.

| Current name | New name | Module | Sendable? |
|---|---|---|---|
| `MapHandle` | `Map` | `std:collections` | Yes |
| `CounterHandle` | `Counter` | `std:collections` | Yes |
| `ListenerHandle` | `TcpListener` | `std:net` | Yes |

Constructor API follows the Rust mirror pattern:

```lua
local Map      = require("std:collections").Map
local Counter  = require("std:collections").Counter
local Listener = require("std:net").TcpListener

local m = Map.new()
local c = Counter.new(0)
local l = Listener.bind("0.0.0.0:8080")
```

### Rust-side layout

Expected layout after implementation:

```
src/lua/std/
  sendable.rs        -- the Sendable trait + spawn-time validation
  collections.rs     -- Map, Counter (both implement Sendable)
  net.rs             -- TcpListener (implements Sendable)
  thread.rs          -- spawn, WorkerHandle, shared channel
  workers.rs         -- on_shutdown only (registry deleted)
```

### The `Sendable` trait

```rust
// src/lua/std/sendable.rs

/// The Sendable protocol.
///
/// Conforming types are safe to transfer across engine boundaries.
/// No string key. No global registry. The spawn-time check walks the
/// `shared` table and asks each value: "are you Sendable?" If yes,
/// extract the Arc. If no, spawn fails with a clear error.
pub trait Sendable: UserData + IntoLua + Sized + 'static {
    /// Extract the inner Arc for handoff. Cheap (Arc::clone).
    fn into_shared(&self) -> Arc<dyn Any + Send + Sync>;

    /// Reconstruct a fresh userdata wrapping the same Arc,
    /// bound to a different engine's Lua state.
    fn from_shared(arc: &Arc<dyn Any + Send + Sync>) -> Option<Self>;
}
```

### Spawn-time validation

When `thread.spawn` is called, the runtime walks every key in the `shared` table:

1. Check that the value is a userdata.
2. Attempt to extract a `Sendable` implementation (try each known Sendable type).
3. If none match, fail immediately with a message naming the offending key, the actual type, and the set of valid Sendable types.

```
thread.spawn: shared.data is 'JsonObject', which is not Sendable.
Use args for by-value transfer, or use a Sendable type
(Map, Counter, TcpListener).
```

### DAG forwarding

A worker holding a Sendable userdata can pass it into its own `thread.spawn { shared = {…} }`. The sharing graph forms a DAG — privilege flows from parent to child, never the reverse.

```lua
-- worker.lua (receives shared.listener from parent)
local shared = thread.shared

-- Forward the listener to a sub-worker
thread.spawn({
    file   = "sub_worker.lua",
    shared = { listener = shared.listener },
})
```

### GC-driven deallocation

Each Sendable userdata holds `Arc<Inner>`. When the last clone across all engines is garbage-collected, `Inner` deallocates. No explicit `:close()` required except where the resource has external state needing predictable teardown.

### `workers.shared` and friends: deleted

The following are removed entirely:

- `workers.shared(key, val)`
- `workers.shared_with(key, val)`
- `workers.get_shared(key)`
- `workers.unshare(key)`
- `register_sharable` hook
- `SharableUserData` trait
- `BUILTIN_SHARABLE_TYPES` slice
- The string-keyed type-name cache

**Survivor:** `workers.on_shutdown` (and the `shutdown_signal` alias) remains unchanged.

### Single-script invocations

`neoc script.luau` that never calls `thread.spawn` continues to work without ceremony. No import required, no ceremony, no cost.

## Drawbacks

- **Breaking change.** Every script using `workers.shared` must be rewritten to use spawn-time handoff. The migration plan (below) stages this across multiple PRs to keep `main` green throughout.
- **Discoverability.** With `Concurrent*` or `std:sync`, the naming convention IS the documentation — a script author scanning the API immediately knows what's shareable. With the Sendable model, you need docs or the spawn-time error to learn the boundary. The mitigation is clear error messages and `.d.luau` type stubs.
- **No static checking.** Swift enforces `Sendable` at compile time. We enforce it at spawn time — a runtime check. The error is immediate and clear, but it's not caught before execution. This is inherent to a dynamically-typed language and is an acceptable tradeoff.

## Alternatives

### Do nothing

Keep `workers.shared`. The mesh graph, stringly-typed discovery, and type-erasure problems remain. Scripts work today, but the model does not scale to the concurrency patterns the runtime needs (explicit spawn graphs, DAG sharing, type-safe handoff). Rejected.

### `Concurrent*` prefix convention (Java model)

Types named `ConcurrentMap`, `ConcurrentCounter`, `ConcurrentTcpListener`. Self-documenting names, but a Java-ism (`ConcurrentHashMap`, `ConcurrentLinkedQueue`) designed for a world where packages are flat and the class name must carry all context. Rust rejected this pattern; `std::sync::Mutex` is just `Mutex`. We are a Rust-mirror runtime and should follow suit. Rejected.

### `std:sync` aggregation module (Rust model)

All shareable types live under `require("std:sync")` — `sync.Map`, `sync.TcpListener`, etc. Mirrors Rust's `std::sync` module literally. Elegant, but forces types out of their natural namespaces. A `Map` is a collection; corralling it into `sync` because it happens to be thread-safe obscures what it is. Jonathan's directive: "I don't want all Sendable variants of UserData to have to originate from that namespace because a SendableMap is a collection." Rejected.

### Retain the global registry alongside spawn-time handoff

Keep `workers.shared` as a compatibility shim while introducing `shared = {…}`. Two parallel sharing models is worse than either model alone — scripts would have to choose, and the documentation burden doubles. Rejected; the migration plan stages the removal across multiple PRs to avoid a big-bang breaking change.

## Open questions

_None. The design is locked following the POC spike (see [issue #40 comments](https://github.com/flying-dice/neoc/issues/40))._

## Implementation notes

### Dependencies

Depends on [#13](https://github.com/flying-dice/neoc/pull/13) landing first (drops `SharableUserData for Object/Array`). **Status: merged.**

### Migration plan

Each step lands as its own PR. `main` stays green throughout.

1. **Introduce `Sendable` trait and spawn-time handoff.**
   Add `src/lua/std/sendable.rs` with the `Sendable` trait. Add `shared = {…}` channel to `thread.spawn`. Implement `Sendable` for existing `MapHandle`, `CounterHandle`, `ListenerHandle` (they already hold `Arc<Inner>`). `workers.shared` continues to work in parallel.

2. **Rename types to their true identities.**
   `MapHandle` → `Map` (in `std:collections`). `CounterHandle` → `Counter` (in `std:collections`). `ListenerHandle` → `TcpListener` (in `std:net`). Constructor API: `Map.new()`, `Counter.new(initial)`, `TcpListener.bind(addr)`. Update module exports so `require("std:collections").Map` works.

3. **Migrate consumers.**
   Walk every test, bench, example, and doc using `workers.shared`. Rewrite each onto spawn-time handoff via `shared = {…}`. Update README module table.

4. **Delete the registry.**
   Remove `workers.shared`, `workers.shared_with`, `workers.get_shared`, `workers.unshare`, `register_sharable`, `SharableUserData`, the registry, the type-name cache. `workers.rs` collapses to the `on_shutdown` module.

### Acceptance criteria

1. **`thread.spawn` accepts a `shared = {…}` channel.** Given a parent script constructs a Sendable userdata, when it passes that userdata in `shared = {…}`, then the worker observes a userdata under `thread.shared` wrapping the same `Arc<Inner>`.

2. **Passing a non-Sendable userdata in `shared = {…}` errors at spawn time.** Error message names the offending type: `"type 'JsonObject' is not Sendable"`.

3. **Types are named for their primary abstraction.** `Map`, `Counter`, `TcpListener` — no prefix, no suffix. Namespaced under their domain module (`std:collections`, `std:net`).

4. **Worker engines can spawn and forward.** A worker holding a Sendable userdata can pass it into its own `thread.spawn { shared = {…} }`. DAG sharing, not mesh.

5. **Lua GC drives `Inner` deallocation.** Each Sendable userdata holds `Arc<Inner>`. Last clone across all engines drops → `Inner` deallocates. No explicit `:close()` required (except for resources with external state needing predictable teardown).

6. **`workers.shared` and friends deleted.** `workers.shared`, `workers.shared_with`, `workers.get_shared`, `workers.unshare`, `register_sharable`, `SharableUserData`, the registry, the type-name cache — all removed. `workers.on_shutdown` (and `shutdown_signal` alias) survives.

7. **Single-script invocations unaffected.** `neoc script.luau` that never calls `thread.spawn` continues to work without ceremony.
