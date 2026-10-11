---
status: draft
created: 2026-05-03
implemented-in:
---

# 0001 — Sendable concurrency model

> [!WARNING]
> This RFC is `draft`. Under refinement — **not** the contract.

## Summary

Replace process-global, string-keyed `workers.shared` registry with spawn-time handoff. Types crossing engine boundaries implement internal `Sendable` trait. `thread.spawn` gains `shared = {…}` table — values validated at spawn time, non-Sendable types rejected with clear error. Global registry, `SharableUserData`, all discovery-by-string-key machinery deleted.

## Motivation

Current cross-engine sharing (`std:workers`) has four structural problems:

1. **Mesh sharing graph.** Any engine publishes/retrieves. No canonical construction site. Script doesn't reveal handle origin.
2. **Type erasure via `Arc<dyn Any + Send + Sync>`.** Registry downcasts at runtime. Type safety advisory.
3. **Stringly-typed discovery.** Engines rendezvous on string keys. Typos compile. Types don't help.
4. **Naming lies about identity.** `MapHandle` is a concurrent hash map. `CounterHandle` is an atomic integer. Names hide reality behind implementation detail.

These compound. Debugging cross-engine issues → trace string keys across files, hope downcast matches, mentally rename `MapHandle` to "concurrent map." Model works but violates design principles: no surprises (1), no footguns (2), mirror Rust (3).

Replacement draws from Swift Structured Concurrency. Core insight: **shareability is a type property, not identity**. `Map` is `Map`. Whether it's `Sendable` is internal, enforced at spawn boundary. No naming convention or registry needed — runtime rejects immediately on violation.

## Detailed design

### Design tenets

1. **Types named for what they are.** `Map`, `Counter`, `TcpListener`. Cross-engine capability is internal property — not naming convention, namespace, or prefix.

2. **Namespace by primary abstraction.** Thread-safe map → `std:collections`. Network listener → `std:net`. `std:sync` reserved exclusively for synchronisation primitives (locks, atomics, channels, barriers). "What is this type?" determines home, not "what does it guarantee?"

3. **Cross-engine capability is protocol, not convention.** Types opt in via `Sendable` trait. Spawn-time machinery checks — no string keys, no naming conventions, no registry. Non-Sendable → error names type and constraint.

4. **Sharing graph is DAG rooted at spawn.** State crosses engines once, at spawn time, one direction. Constructor decides recipients. Workers forward to children — privilege local, scoped to what each engine received. No mesh, no global rendezvous.

5. **Lua GC replaces lifetime tracking.** Each Sendable userdata holds `Arc<Inner>`. Last clone across all engines GC'd → `Inner` deallocates. No explicit `:close()` except where resource has external state needing predictable teardown (e.g. TCP listener).

6. **Spawn-time validation, not runtime discovery.** `thread.spawn` walks `shared` table, validates every entry immediately. Violations caught at spawn — not deferred to first use, not silent.

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

Types named for what they **are**. No `Concurrent*` prefix. No `Sync*` prefix. Types live in semantically correct modules.

| Current name | New name | Module | Sendable? |
|---|---|---|---|
| `MapHandle` | `Map` | `std:collections` | Yes |
| `CounterHandle` | `Counter` | `std:collections` | Yes |
| `ListenerHandle` | `TcpListener` | `std:net` | Yes |

Constructors follow Rust mirror pattern:

```lua
local Map      = require("std:collections").Map
local Counter  = require("std:collections").Counter
local Listener = require("std:net").TcpListener

local m = Map.new()
local c = Counter.new(0)
local l = Listener.bind("0.0.0.0:8080")
```

### Rust-side layout

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

When `thread.spawn` called, runtime walks every key in `shared`:

1. Check value is userdata.
2. Attempt `Sendable` extraction (try each known Sendable type).
3. None match → fail immediately, naming offending key, actual type, valid Sendable types.

```
thread.spawn: shared.data is 'JsonObject', which is not Sendable.
Use args for by-value transfer, or use a Sendable type
(Map, Counter, TcpListener).
```

### DAG forwarding

Worker holding Sendable userdata can pass it into its own `thread.spawn { shared = {…} }`. Sharing graph forms DAG — privilege flows parent → child, never reverse.

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

Each Sendable userdata holds `Arc<Inner>`. Last clone across all engines GC'd → `Inner` deallocates. No explicit `:close()` except where resource has external state needing predictable teardown.

### `workers.shared` and friends: deleted

Removed entirely:

- `workers.shared(key, val)`
- `workers.shared_with(key, val)`
- `workers.get_shared(key)`
- `workers.unshare(key)`
- `register_sharable` hook
- `SharableUserData` trait
- `BUILTIN_SHARABLE_TYPES` slice
- String-keyed type-name cache

**Survivor:** `workers.on_shutdown` (and `shutdown_signal` alias) unchanged.

### Single-script invocations

`neoc script.lua` without `thread.spawn` → works unchanged. No import, no ceremony, no cost.

## Drawbacks

- **Breaking change.** All scripts using `workers.shared` must rewrite to spawn-time handoff. Migration plan stages across multiple PRs — `main` stays green.
- **Discoverability.** `Concurrent*` or `std:sync` → naming convention IS documentation. Sendable model requires docs or spawn-time error to learn boundary. Mitigation: clear errors + `.d.luau` type stubs.
- **No static checking.** Swift enforces `Sendable` at compile time; we enforce at spawn time (runtime check). Error is immediate and clear but not caught before execution. Inherent to dynamic typing — acceptable tradeoff.

## Alternatives

### Do nothing

Keep `workers.shared`. Mesh graph, stringly-typed discovery, type-erasure remain. Does not scale to needed concurrency patterns (explicit spawn graphs, DAG sharing, type-safe handoff). Rejected.

### `Concurrent*` prefix convention (Java model)

`ConcurrentMap`, `ConcurrentCounter`, `ConcurrentTcpListener`. Self-documenting but Java-ism designed for flat packages where class name carries all context. Rust rejected this — `std::sync::Mutex` is just `Mutex`. We mirror Rust. Rejected.

### `std:sync` aggregation module (Rust model)

All shareable types under `require("std:sync")`. Mirrors `std::sync` literally but forces types out of natural namespaces. `Map` is a collection — corralling into `sync` because thread-safe obscures what it is. Directive: "SendableMap is a collection." Rejected.

### Retain global registry alongside spawn-time handoff

Keep `workers.shared` as compatibility shim. Two parallel sharing models worse than either alone — scripts must choose, documentation burden doubles. Migration plan stages removal across PRs. Rejected.

## Open questions

_None. Design locked following POC spike (see [issue #40 comments](https://github.com/flying-dice/neoc/issues/40))._

## Implementation notes

### Dependencies

Depends on [#13](https://github.com/flying-dice/neoc/pull/13) (drops `SharableUserData for Object/Array`). **Status: merged.**

### Migration plan

Each step lands as own PR. `main` stays green throughout.

1. **Introduce `Sendable` trait and spawn-time handoff.** Add `src/lua/std/sendable.rs`. Add `shared = {…}` channel to `thread.spawn`. Implement `Sendable` for existing `MapHandle`, `CounterHandle`, `ListenerHandle` (already hold `Arc<Inner>`). `workers.shared` continues in parallel.

2. **Rename types.** `MapHandle` → `Map`, `CounterHandle` → `Counter`, `ListenerHandle` → `TcpListener`. Constructors: `Map.new()`, `Counter.new(initial)`, `TcpListener.bind(addr)`. Update exports so `require("std:collections").Map` works.

3. **Migrate consumers.** Walk every test, bench, example, doc using `workers.shared`. Rewrite onto spawn-time handoff. Update README module table.

4. **Delete registry.** Remove `workers.shared`, `workers.shared_with`, `workers.get_shared`, `workers.unshare`, `register_sharable`, `SharableUserData`, registry, type-name cache. `workers.rs` collapses to `on_shutdown` module.

### Acceptance criteria

1. **`thread.spawn` accepts `shared = {…}`.** Parent passes Sendable userdata → worker observes userdata under `thread.shared` wrapping same `Arc<Inner>`.

2. **Non-Sendable in `shared` errors at spawn time.** Error names offending type: `"type 'JsonObject' is not Sendable"`.

3. **Types named for primary abstraction.** `Map`, `Counter`, `TcpListener` — no prefix/suffix. Under domain module (`std:collections`, `std:net`).

4. **Workers can spawn and forward.** Worker holding Sendable userdata passes into own `thread.spawn { shared = {…} }`. DAG sharing, not mesh.

5. **GC drives deallocation.** `Arc<Inner>` per Sendable userdata. Last clone drops → `Inner` deallocates. No `:close()` except external-state resources.

6. **`workers.shared` and friends deleted.** All registry machinery removed. `workers.on_shutdown` + `shutdown_signal` alias survive.

7. **Single-script unaffected.** `neoc script.lua` without `thread.spawn` works unchanged.
