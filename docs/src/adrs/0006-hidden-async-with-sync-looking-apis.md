---
status: Accepted
date: 2026-05-03
---

# 0006. Hidden async with synchronous-looking Lua APIs

Date: 2026-05-03

## Status

Accepted

## Context

`neoc` runs scripts on top of a Tokio runtime through `mlua`'s Luau bindings. Many capabilities exposed to scripts — HTTP, networking, database access, filesystem I/O — are async at the Rust layer. The runtime needs a discipline for surfacing async work to Lua.

Three patterns were considered:

- **Explicit async with `:await()`** — async functions return a future or promise object, and scripts call `:await()` to unwrap it. Familiar from JavaScript, Rust, Python. Requires script authors to track which calls are async.
- **Hidden async with synchronous-looking APIs** — async functions look like plain function calls. The coroutine yields to Tokio internally; the script never sees the yield. Adopted by every major Lua-on-async-runtime.
- **Hybrid (callbacks + coroutines)** — both patterns coexist. The Neovim ecosystem went this route and ended up with split conventions and doubled cognitive load.

Every major Lua-on-async-runtime converges on hidden async:

| Runtime | Approach |
|---|---|
| `mlua` (Rust Lua bindings) | `create_async_function` bridges Rust futures to Lua coroutines; the Lua side calls them as plain functions. |
| Lune (Luau on Tokio) | `net.request` returns the response directly. Fully synchronous-looking. |
| OpenResty (ngx_lua) | Cosockets — all I/O non-blocking but synchronous-looking. Gold standard at Cloudflare and Kong scale. |
| Tarantool | Fibers — all I/O implicitly yields the current fiber. The most mature implementation of this pattern. |
| Luau in Roblox | Built-in APIs are yield-based. The community `roblox-lua-promise` package exists, but official APIs do not use it. |

Runtimes that surface explicit `await` to scripts (Deno, GDScript, MicroPython) all do so in languages that have `async`/`await` as language keywords. Lua has coroutines but no syntactic support for futures or promises. Exposing futures into a Lua script creates an alien, non-idiomatic surface and forces every call site to track which functions are async.

The cautionary tale is Luvit, which adopted Node-style callbacks in Lua. The result is widely considered the worst developer experience in the Lua-on-async space. The Neovim ecosystem ended up with both patterns coexisting, producing the split-ecosystem cost the hybrid approach is supposed to avoid.

## Decision

`neoc` exposes async work to Lua scripts as **hidden async with synchronous-looking APIs**. There are two layers:

### Layer 1 — synchronous-looking APIs (the default)

Every async operation looks like a regular function call. The Lua coroutine yields to Tokio internally; the script never sees the yield, and there is no future, promise, or `:await()` to unwrap.

```lua
local hyper = require("vnd:hyper")

local resp = hyper.get("https://example.com")
print(resp.body)
```

A reader cannot tell from the call site whether `hyper.get` is implemented as a blocking call or as a coroutine yield. This is by design.

### Layer 2 — concurrency primitives (opt-in)

For scripts that need parallelism, two mechanisms are available:

**Convenience batch APIs** for common patterns. These cover the majority of concurrency use cases without exposing the underlying primitives.

```lua
local hyper = require("vnd:hyper")

local responses = hyper.get_all({
    "https://a.example.com",
    "https://b.example.com",
    "https://c.example.com",
})
```

**`task.spawn` / `task.await` primitives** for arbitrary concurrent work. This mirrors the OpenResty `ngx.thread.spawn` / `ngx.thread.wait` and Lune `task.spawn` patterns.

```lua
local task  = require("std:task")
local hyper = require("vnd:hyper")

local t1 = task.spawn(function() return hyper.get("https://a.example.com") end)
local t2 = task.spawn(function() return hyper.get("https://b.example.com") end)

local r1 = task.await(t1)
local r2 = task.await(t2)
```

`task.await` on a spawned task handle is semantically distinct from a hypothetical `:await()` on every API call. It joins a concurrent unit of work — fork-join semantics — rather than unwrapping a promise. The async colour does not leak into the surrounding code.

The `task` module is forward work. This decision establishes the shape it must take when it lands.

## Consequences

- Script authors write code that reads as if it is synchronous. Sequential I/O is sequential at the call site, even when the underlying operations yield to Tokio.
- The async-vs-sync distinction does not leak into the script's call sites or type signatures. There is no "async colour" the script must propagate.
- Every Lua-on-async-runtime an experienced reader has used works the same way. There is no learning curve for the basic surface.
- Concurrency requires an explicit opt-in. A script that does not reach for `task.spawn` or a batch API runs sequentially. This is the intended default — most scripts do not need parallelism, and the ones that do should declare it.
- The Lua adapter layer in each module is responsible for bridging Rust futures into Lua coroutines through `mlua`'s `create_async_function`. New `vnd:*` and `std:*` modules wrapping async crates must follow this pattern; the module layer cannot expose raw futures to scripts.
- The `task` module is committed to as a future surface. Until it lands, parallelism is available only through the worker pool ([`std:workers`](/std/reference/workers)) or through ad-hoc Lua coroutine use. Both are workable but not the recommended path.
- Reasoning about a script's behaviour under concurrency is harder than it would be with explicit `:await()` markers. The cost is borne by anyone debugging a script that mixes I/O and shared state; the runtime's documentation must be explicit about which calls yield.

## References

- [OpenResty cosocket design](https://api7.ai/learning-center/openresty/the-core-of-openresty-cosocket)
- [Lune task scheduler](https://lune-org.github.io/docs/the-book/9-task-scheduler/)
- [`mlua` async support](https://github.com/mlua-rs/mlua)
- [Neovim async patterns](https://dzx.fr/blog/async-lua-in-neovim/) (cautionary tale)
- Originating issue: [flying-dice/neoc#44](https://github.com/flying-dice/neoc/issues/44)
