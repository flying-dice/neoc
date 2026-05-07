# The sandboxing model

This guide explains how neoc sandboxes the Luau engine: which parts of Lua's standard library are stripped, why those parts are removed, and what the curated module surface replaces them with. Familiarity with Lua and basic operating-system concepts (filesystems, processes, network sockets) is assumed.

## Why sandbox

Luau is a sandbox-friendly dialect of Lua. The standard distribution still ships modules that expose unrestricted operating-system access — `io.open`, `os.execute`, `package.loadlib`, and the `debug` library, among others. A general-purpose host runtime that simply embeds Luau and exposes those modules inherits whatever the script chooses to do with them.

neoc takes the opposite default. The unsafe halves of the standard library are stripped at engine construction, and every capability a script needs — file access, network I/O, subprocess control, third-party crate functionality — is reintroduced through *explicit modules* under the `std:*`, `lib:*`, and `vnd:*` namespaces. Scripts cannot reach a capability that the host has not registered.

## What is stripped

The following standard Lua surface is unavailable inside neoc's engine.

| Stripped surface | Reason |
|---|---|
| `io.*` | File and stream I/O is reintroduced through [`std:fs`](/std/reference/fs) and [`std:io`](/std/reference/io). |
| `os.*` | Process and time control is reintroduced through `std:thread`, `std:net`, and the worker model. |
| `package.*` | The `require` resolver is replaced; see [The module system](/guides/module-system). |
| `debug.*` | Reflection over running coroutines, registries, and upvalues is unsafe in a multi-tenant runtime. |
| `loadfile`, `dofile` | File-driven script loading is replaced by the binary's argument-driven model. |

The remaining standard surface — `string`, `table`, `math`, `coroutine`, `bit32`, `utf8`, `buffer`, `vector`, the basic `print` family — is available unchanged.

## What replaces it

Every capability that scripts need is reintroduced through one of three namespaces, chosen by the kind of capability:

`std:*`
:   Capabilities that mirror Rust's standard library. Filesystem, networking, threading, collections, I/O. The surface is shaped so a reader fluent in Rust's `std::*` recognises the methods and semantics. See [`std:*`](/std/).

`lib:*`
:   Blessed gap-fillers and project-native modules. JSON document handles, base64 encoding, the test runner, the VCR fixture system. See [`lib:*`](/lib/).

`vnd:*`
:   Individual vendored Rust crates exposed to Lua. The module name matches the upstream crate name verbatim — `vnd:hyper` is the `hyper` crate, `vnd:serde_json` is the `serde_json` crate. See [`vnd:*`](/vnd/).

> **Note:** A script cannot invent its own access to a stripped capability. Reaching into the host through FFI, loading shared libraries, or executing subprocesses are not available. The host runtime is the only thing that can extend the script's capability surface, and it does so by registering modules at engine construction.

## The worker model

Scripts execute inside independent Luau virtual machines called *workers*. Each worker is a self-contained interpreter: globals, locals, upvalues, and any userdata constructed inside a worker are private to that worker. A main script binds resources and spawns workers explicitly via [`thread.spawn`](/std/reference/thread).

Cross-worker state follows the *Sendable* protocol. A script passes shared state to a spawned worker through the `shared` table argument of `thread.spawn`. Only userdata types that implement the Sendable trait — `Map` and `Counter` from [`std:collections`](/std/reference/collections), and the TCP listener from [`std:net`](/std/reference/net) — may appear in the `shared` table. Other userdata types — including [`lib:json`](/lib/reference/json) Object and Array handles and [`json.null`](/lib/reference/json#json-null) — are rejected at spawn time. Plain Lua values (strings, numbers, tables) can be passed to workers via the `args` field instead.

For process-level coordination (shutdown signals), see [`std:workers`](/std/reference/workers). For worker spawning and message-passing, see [`std:thread`](/std/reference/thread).

## See also

- [The module system](/guides/module-system) — How `require("ns:name")` resolves and the rules each namespace follows.
- [Error conventions](/guides/error-conventions) — How modules signal failures.
- [`std:workers`](/std/reference/workers) — Process-level coordination (shutdown signal).
- [`std:thread`](/std/reference/thread) — Worker spawning and message-passing channels.
- [`std:fs`](/std/reference/fs) — Filesystem reads, writes, and metadata.
- [`std:net`](/std/reference/net) — TCP listeners and sockets.
