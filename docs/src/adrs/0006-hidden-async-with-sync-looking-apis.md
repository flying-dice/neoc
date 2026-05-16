---
status: Accepted
date: 2026-05-03
---

# 0006. Hidden async with synchronous-looking Lua APIs

Date: 2026-05-03

## Status

Accepted

## Context

neoc runs on Tokio via `mlua`. Many capabilities (HTTP, networking, DB, filesystem) are async at the Rust layer. Need a discipline for surfacing async to Lua.

Options:
- **Explicit `:await()`** — script authors track which calls are async. Alien in Lua (no language-level async/await keywords).
- **Hidden async** — async calls look synchronous. Coroutine yields to Tokio internally. Script never sees yield.
- **Hybrid (callbacks + coroutines)** — split conventions, doubled cognitive load (Neovim cautionary tale).

Every major Lua-on-async-runtime converges on hidden async:

| Runtime | Approach |
|---|---|
| `mlua` | `create_async_function` → Lua calls as plain functions |
| Lune | `net.request` returns response directly |
| OpenResty | Cosockets — non-blocking, synchronous-looking |
| Tarantool | Fibers — all I/O yields current fiber |
| Roblox | Built-in APIs yield-based |

Runtimes with explicit await (Deno, GDScript) have `async/await` as language keywords. Lua does not.

## Decision

Hidden async with synchronous-looking APIs. Two layers:

### Layer 1 — synchronous-looking (default)

Every async operation looks like a plain call. Coroutine yields to Tokio internally.

```lua
local hyper = require("vnd:hyper")
local resp = hyper.get("https://example.com")
print(resp.body)
```

### Layer 2 — concurrency primitives (opt-in)

**Batch APIs** for common patterns:

```lua
local responses = hyper.get_all({
    "https://a.example.com",
    "https://b.example.com",
})
```

**`task.spawn` / `task.await`** for arbitrary concurrent work (fork-join, not promise unwrap):

```lua
local task  = require("std:task")
local hyper = require("vnd:hyper")

local t1 = task.spawn(function() return hyper.get("https://a.example.com") end)
local t2 = task.spawn(function() return hyper.get("https://b.example.com") end)

local r1 = task.await(t1)
local r2 = task.await(t2)
```

`task` module is forward work. This ADR establishes its required shape.

## Consequences

- Scripts read as synchronous. No async colour leaking into call sites or type signatures.
- Zero learning curve — every Lua-on-async-runtime works this way.
- Concurrency requires explicit opt-in (`task.spawn` or batch API).
- Module adapter layer must bridge futures via `create_async_function`. Cannot expose raw futures.
- Until `task` lands, parallelism only via worker pool or ad-hoc coroutines.
- Debugging concurrent scripts harder without explicit `:await()` markers — docs must specify which calls yield.

## References

- [OpenResty cosocket design](https://api7.ai/learning-center/openresty/the-core-of-openresty-cosocket)
- [Lune task scheduler](https://lune-org.github.io/docs/the-book/9-task-scheduler/)
- [`mlua` async support](https://github.com/mlua-rs/mlua)
- [Neovim async patterns](https://dzx.fr/blog/async-lua-in-neovim/) (cautionary tale)
- Issue: [flying-dice/neoc#44](https://github.com/flying-dice/neoc/issues/44)
