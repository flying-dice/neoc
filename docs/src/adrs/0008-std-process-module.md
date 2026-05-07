---
status: Proposed
date: 2026-05-05
---

# 0008. `std:process` — environment, arguments, and child-process spawning

Date: 2026-05-05

## Status

Proposed

## Context

neoc strips `os.*` and `io.*` at engine construction (ADR 0002). This eliminates `os.execute`, `io.popen`, `os.getenv`, and `os.exit` — the Lua standard library's only mechanisms for reading environment state, accessing CLI arguments, and invoking subprocesses. Scripts that need any of these capabilities have no sanctioned path today.

The gap is a practical blocker for CLI tools, migration runners, build scripts, and any server application that reads configuration from the environment (`DATABASE_URL`, `PORT`, `LOG_LEVEL`). These are bread-and-butter use cases for a general-purpose Luau runtime.

Every major runtime ships a process module. The design space is not large, but the details vary in ways that matter — especially around subprocess I/O, security gating, and async semantics.

### Cross-runtime survey

| Runtime | Env | Args | Exit | Spawn surface |
|---|---|---|---|---|
| **Rust `std::process`** | `std::env::var` / `vars` | `std::env::args` | `std::process::exit(code)` | `Command::new(cmd).args(...).stdout(Stdio::piped()).output()` — builder pattern, blocks until child exits |
| **Lune** | `process.env` (table proxy) | `process.args` (table) | `process.exit(code)` | `process.spawn(cmd, args, opts)` — returns `{ok, code, stdout, stderr}`; async, waits for completion |
| **Deno** | `Deno.env.get(key)` / `Deno.env.set(key, val)` | `Deno.args` | `Deno.exit(code)` | `new Deno.Command(cmd, opts).output()` — awaitable; `spawn()` returns a child handle with readable streams |
| **Bun** | `Bun.env` / `process.env` | `process.argv` (Node compat) | `process.exit(code)` | `Bun.spawn(cmd, opts)` / `Bun.$`shell template — `spawn` returns a child handle |
| **Go `os/exec`** | `os.Getenv` / `os.Setenv` | `os.Args` | `os.Exit(code)` | `exec.Command(name, args...).Output()` — builder, blocks; `Start()`/`Wait()` for async |
| **Node `child_process`** | `process.env` (plain object) | `process.argv` | `process.exit(code)` | `spawn(cmd, args, opts)` returns EventEmitter streams; `execSync` / `spawnSync` for blocking variants |

**Key observations:**

1. **Env is always a flat, readable map.** Every runtime exposes environment variables as a key/value lookup. Writability is present in most (Node, Lune, Deno, Bun) — Rust's `std::env::set_var` has known unsafety in multi-threaded contexts but is present. neoc is single-VM-per-worker, so write safety is not the same issue, but write is low-value and opens questions about child process inheritance.

2. **Args exclude the runtime binary and script path.** Lune and Deno drop the interpreter prefix; Node's `process.argv[0]` and `argv[1]` are the runtime and script — scripts typically slice from index 2. The Lune/Deno convention is cleaner for scripting use.

3. **Spawn is synchronous-looking at the call site.** Lune's `process.spawn` awaits completion and returns a result table directly, consistent with ADR 0006. Deno's `Command.output()` is awaited, but looks synchronous at a Luau call site when the hidden-async model applies. Rust's `output()` blocks. The result shape is consistent: exit code, stdout buffer, stderr buffer, ok/status flag.

4. **Security gating varies.** Deno requires `--allow-run` to call `Deno.Command` at all; without the flag the call throws. Lune has no such gate. Bun has no such gate. Node has no such gate. Go has no such gate at the language level (binary-level permissions apply). The Deno model is the only one with a runtime permission flag, and it receives consistent praise in the security community — but it requires a flag-passing API at the runtime level that neoc does not yet have.

5. **Streaming vs. wait-for-completion.** Deno and Bun expose both modes: `output()` waits for the child to exit and buffers stdout/stderr; `spawn()` returns a child handle with readable stream properties for real-time output. Node exposes streams by default. Lune and Rust `output()` wait for completion. The wait-for-completion shape covers the vast majority of script use cases (run a command, get its output, continue). Streaming is useful for long-running processes and interactive tools but adds significant API surface.

### API shape options

**Option A — flat module with `spawn` as a synchronous-looking call (Lune-aligned)**

```lua
local process = require("std:process")

local db_url = process.env.DATABASE_URL  -- read (write not supported in v1)

for _, arg in process.args do print(arg) end

print(process.cwd)

process.exit(0)

local result = process.spawn("ls", { "-la" }, {
    cwd  = "/tmp",
    env  = { PATH = "/usr/bin" },
    stdin  = "null",
    stdout = "pipe",
    stderr = "pipe",
})
-- result.ok     boolean
-- result.code   integer
-- result.stdout string
-- result.stderr string
```

Closest to Lune. Env is a metatable proxy over Rust `std::env`. `spawn` hides the async wait behind ADR 0006 semantics.

**Option B — `Command` builder type (Rust/Deno-aligned)**

```lua
local process = require("std:process")

local result = process.Command.new("ls")
    :args({ "-la" })
    :cwd("/tmp")
    :stdout("pipe")
    :stderr("pipe")
    :output()   -- blocks via hidden async
-- result.ok, result.code, result.stdout, result.stderr
```

Closer to `std::process::Command`. Method-chained builder; `:output()` is the terminal call. Env, args, cwd remain as flat properties on the module itself.

**Option C — split surface: `process.run` for simple cases, `Command` builder for full control**

```lua
local process = require("std:process")

-- simple: just run it
local ok, err = process.run("npm", { "install" })

-- full control
local result = process.Command.new("npm")
    :args({ "run", "build" })
    :env({ NODE_ENV = "production" })
    :stdout("pipe")
    :output()
```

More surface, more decisions about which variant to reach for. Adds a simple happy path at the cost of two mental models.

### The security question

Should `process.spawn` (under any option) require an explicit permission flag?

Deno's `--allow-run` flag is the only prior art with a hard runtime gate. The argument for gating: subprocess spawning is the single capability in this module with the widest ambient risk — it is how a compromised or malicious script escapes the sandbox to reach the host OS. The argument against: neoc's security model (ADR 0002) is currently binary — the module is registered or it is not. Scripts have no way to pass per-capability flags, and the runtime has no CLI flag system today. Introducing one just for this module would be out of place.

Pragmatic conclusion: the security gate question is a **separate architectural decision** that applies to the whole permission model, not just process. For `std:process` v1, the decision is binary: if the module is registered, spawn is available; if not, it is not. The registration point (inside `modules.rs`) is where future per-capability gating would be wired. The ADR records this constraint explicitly rather than ignoring it.

## Decision

`std:process` is introduced as **Option A** — a flat module with a `spawn` function that returns a result table, consistent with the Lune convention and ADR 0006 (hidden async).

The module surface is:

```
process.env          — metatable proxy; read access to environment variables.
                       Write is NOT supported in v1. The multi-process inheritance
                       semantics are underspecified; write can be added in a follow-on
                       ADR once the use cases are concrete.

process.args         — read-only table; CLI arguments passed to the script,
                       excluding the runtime binary name and the script path.
                       Matches Lune and Deno conventions.

process.cwd          — string; current working directory, captured eagerly at
                       module load time (not lazily on first access). Read-only.
                       Does not reflect any subsequent chdir. Matches Lune
                       behaviour; implementors use std::env::current_dir() in
                       the module constructor.

process.exit(code)   — terminates the process with the given exit code.
                       Mirrors every peer runtime; no alternative is credible.
                       `code` must be an integer number; a non-integer or
                       non-number argument is a Lua error. The code is passed
                       through to the OS; platform truncation applies (Unix:
                       mod 256, so exit(256) delivers 0 and exit(-1) delivers
                       255). Windows accepts a 32-bit unsigned value.

process.spawn(cmd, args, opts) → result
                     — spawns a subprocess, waits for it to exit, and returns
                       a result table. Hidden async per ADR 0006.
```

The `spawn` result table:

```
result.ok      boolean — true if the exit code is 0
result.code    integer — raw exit code
result.stdout  string  — captured stdout (empty string if opts.stdout != "pipe").
                         Always string, never nil — so result.stdout is always
                         a string in Luau strict mode and callers need no nil
                         guards. Intentional divergence from Lune, which returns
                         nil for uncaptured streams.
result.stderr  string  — captured stderr (empty string if opts.stderr != "pipe").
                         Same always-string contract as result.stdout.
```

`spawn` options:

```
opts.cwd     string  — working directory for the child; defaults to process.cwd
opts.env     table   — when provided, replaces the child's environment entirely;
                       the parent environment is NOT inherited (same as Lune).
                       Omit or pass nil to inherit the parent env unchanged.
opts.stdin   string  — "null" (default) | "inherit"
opts.stdout  string  — "null" | "inherit" | "pipe" (default)
opts.stderr  string  — "null" | "inherit" | "pipe" (default)

Note: `stdin = "pipe"` is not supported in v1. Writing to a child's stdin
requires a streaming API that is out of scope for this ADR. If the use case
arises, extend the spawn opts in a follow-on ADR; do not implement pipe
stdin silently.
```

**Option B (Command builder)** is rejected for v1. The builder pattern adds indirection without covering a capability gap: neoc's hidden-async model means there is no reason to separate construction from execution. The builder pattern in Rust solves a real problem (separating the configuration phase from the blocking call); at the Lua layer that separation does not carry the same weight. If streaming output becomes a firm requirement, a `Command` builder can be added as a second surface alongside `spawn` in a later ADR.

**Option C** is rejected. Two calling conventions for the same operation means two mental models. Scripts would need to know which variant applies to their situation, and the criteria are not obvious.

**`process.env` is read-only in v1.** Write access to the environment has non-obvious child-process inheritance behaviour and can surprise script authors when a spawned child does or does not see the written value. The use case (constructing an environment for a child process) is better served by `opts.env` on `process.spawn`. Write can be reconsidered once there are concrete use cases that `opts.env` cannot cover.

**No permission gate in v1.** Capability gating belongs to a permission-model ADR, not here. If `std:process` is registered in `modules.rs`, `spawn` is available. The host application controls registration.

## Consequences

- Scripts can read environment variables, access CLI args, get the working directory, exit cleanly, and run subprocesses. The full acceptance criteria for issue #7 are satisfiable.
- The `spawn` surface covers the dominant use case — run a command, get its output, branch on success. Streaming (real-time stdout/stderr) is not covered; if that becomes a requirement, a `Command` builder is the natural extension point and can be added without breaking the `spawn` surface.
- `process.env` is read-only. Scripts that currently set env vars before spawning a child use `opts.env` instead. This is the more explicit API and avoids surprise inheritance. The constraint is documented; scripts cannot accidentally break child process behaviour by mutating global state.
- `process.exit` is a hard process termination. It bypasses Lua's `pcall`/`xpcall` stack unwinding, drops all open handles without finalisation, and does not run `__gc` metamethods. It also bypasses `__close` finalizers on Luau to-be-closed variables (`local f <close> = openHandle()`): any `<close>` variable in scope at the point `exit` is called will not have its `__close` metamethod invoked — scripts must run explicit cleanup before calling `exit` if handle release matters. Additionally, if `std:thread` worker threads are active, `process.exit` terminates them immediately; any work in flight or pending I/O in those threads is dropped without a grace period. This matches `os.exit` semantics in standard Lua and is the expected behaviour everywhere.
- The `spawn` implementation wraps `tokio::process::Command` and yields the Lua coroutine while the child runs, consistent with ADR 0006. A spawned child's stdout and stderr buffers are collected into strings; very large output (multi-GB) is a memory risk the script author is responsible for managing. No streaming cap is imposed in v1.
- The Rust implementation lives at `src/lua/std/process.rs`. It is registered in `src/lua/std/mod.rs` alongside `fs`, `net`, `thread`, etc. The OS `os.*` surface remains stripped; this module does not re-expose it — it provides a new, intentional surface over `std::process` and `std::env`.
- Security: subprocess spawning is the broadest OS-escape vector this runtime exposes. The decision to gate it only at module-registration time is deliberate but conservative. A future permission-model ADR should address per-script capability grants; `process.spawn` is the primary motivation for that work.
- `process.cwd` is captured eagerly at module load time (not lazily on first access). Two modules that `require("std:process")` at different points in the same script will see the same value. It does not reflect any subsequent `chdir` call, since `chdir` is not exposed to scripts.

## Testing strategy

This module has three fundamentally different testing surfaces, each requiring a different approach:

**`process.env`, `process.args`, `process.cwd`** — these are set by the host process at startup and are read-only inside the VM. In-process `.test.luau` tests cannot mutate them. Tests for these properties use subprocess invocation: the test harness spawns `cargo test --bin neoc` with a controlled environment, known argv, and a known working directory, then asserts the Luau script receives the expected values. The Rust-level test fixture wires a fake env table and argv slice before constructing the engine.

**`process.exit`** — testing that `exit(0)` terminates the process with the correct code requires a subprocess test, not an in-process `.test.luau`. These tests live under `cargo test --bin neoc` and assert exit status using `std::process::Command::status()`. Exit code boundary cases (exit(0), exit(1), exit(255), exit(256)) are each a distinct subprocess test case.

**`process.spawn`** — output capture, exit code propagation, cwd inheritance, env replacement, stdout/stderr separation are each a distinct integration test. CI-safe cross-platform target commands: `echo` (predictable output, no network, no root — available on Linux, macOS, and Windows); for env replacement assertions, spawn a small neoc script that prints `process.env.SOME_VAR` rather than relying on the Unix `env` binary; for cwd assertions, spawn a script that prints `process.cwd` rather than the Unix `pwd` binary. The Unix `env` and `pwd` commands are not available on Windows and must not be used as test targets. Tests live in `tests/std/process.test.luau` and are exercised via `cargo run -- 'tests/**/*.test.luau'`.

## References

- [Lune `process` module](https://lune-org.github.io/docs/api-reference/process/)
- [Deno `Deno.Command` API](https://deno.land/api?s=Deno.Command)
- [Rust `std::process::Command`](https://doc.rust-lang.org/std/process/struct.Command.html)
- [Bun `Bun.spawn`](https://bun.sh/docs/api/spawn)
- [Go `os/exec`](https://pkg.go.dev/os/exec)
- [Deno permission model — `--allow-run`](https://docs.deno.com/runtime/fundamentals/security/)
- [neoc sandboxing model](/guides/sandboxing-model) — documents the stripped `os.*` surface this module replaces
- ADR 0002: [Strip the unsafe halves of the Lua standard library](/adrs/0002-strip-unsafe-lua-stdlib)
- ADR 0006: [Hidden async with synchronous-looking Lua APIs](/adrs/0006-hidden-async-with-sync-looking-apis)
- Originating issue: [flying-dice/neoc#7](https://github.com/flying-dice/neoc/issues/7)
