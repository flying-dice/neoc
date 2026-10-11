# The sandboxing model

What neoc strips from Lua, why, and what replaces it.

## Why sandbox

LuaJIT ships modules exposing unrestricted OS access — `io.open`, `os.execute`, `package.loadlib`, the `debug` library, and the `ffi` and `jit` libraries. Embedding without stripping inherits whatever scripts choose to do.

neoc takes the opposite default. The engine loads an explicit standard-library set and strips unsafe globals at construction. Every capability reintroduced through explicit modules under `std:*`, `lib:*`, `vnd:*`. Scripts cannot reach unregistered capabilities.

## What is stripped

| Surface | Reason |
|---|---|
| `io.*` | Replaced by [`std:fs`](/std/reference/fs) and [`std:io`](/std/reference/io) |
| `os.*` | Replaced by `std:thread`, `std:net`, worker model |
| `package.*` | `require` replaced; see [module system](/guides/module-system) |
| `debug.*` | Reflection unsafe in multi-tenant runtime |
| `ffi.*`, `jit.*` | Arbitrary native memory and VM control; never loaded |
| `load`, `loadstring`, `loadfile`, `dofile`, `string.dump` | Runtime code loading; replaced by argument-driven model |
| `getfenv`, `setfenv`, `newproxy`, `module`, `collectgarbage`, `gcinfo` | Environment and identity tampering, GC control |

Available: the base functions that remain, `string`, `table`, `math`, `coroutine`, `bit`, and the `print` family.

## What replaces it

`std:*`
:   Rust standard library mirrors. Filesystem, networking, threading, collections, I/O. See [`std:*`](/std/).

`lib:*`
:   Gap-fillers and project-native modules. JSON handles, base64, test runner, VCR fixtures. See [`lib:*`](/lib/).

`vnd:*`
:   Vendored Rust crates. Module name matches upstream crate name. See [`vnd:*`](/vnd/).

> **Note:** Scripts cannot invent access to stripped capabilities. No FFI, no shared library loading, no subprocess execution. Only the host extends the capability surface by registering modules at construction.

## The worker model

Scripts execute in independent LuaJIT VMs called *workers*. Each worker: self-contained interpreter with private globals, locals, upvalues, userdata.

Cross-worker state follows *Sendable* protocol via `shared` table in `thread.spawn`. Sendable types: `Map`, `Counter` from [`std:collections`](/std/reference/collections), TCP listener from [`std:net`](/std/reference/net). Non-Sendable types (json handles, `json.null`) rejected at spawn time. Plain Lua values pass via `args` field.

- Process coordination: [`std:workers`](/std/reference/workers)
- Worker spawning: [`std:thread`](/std/reference/thread)

## See also

- [The module system](/guides/module-system)
- [Error conventions](/guides/error-conventions)
- [`std:workers`](/std/reference/workers)
- [`std:thread`](/std/reference/thread)
- [`std:fs`](/std/reference/fs)
- [`std:net`](/std/reference/net)
