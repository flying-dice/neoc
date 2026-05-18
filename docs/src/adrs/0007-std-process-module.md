---
status: Proposed
date: 2026-05-05
---

# 0007. `std:process` — environment, arguments, and child-process spawning

Date: 2026-05-05

## Status

Proposed

## Context

ADR 0002 strips `os.*`/`io.*` — eliminates `os.execute`, `io.popen`, `os.getenv`, `os.exit`. No sanctioned path for env, args, or subprocesses. Practical blocker for CLI tools, migration runners, build scripts, config-from-env.

Cross-runtime survey (all converge on same shape):

| Runtime | Env | Args | Exit | Spawn |
|---|---|---|---|---|
| Rust | `env::var`/`vars` | `env::args` | `process::exit` | `Command` builder, blocks |
| Lune | `process.env` (proxy) | `process.args` (table) | `process.exit(code)` | `process.spawn` → `{ok,code,stdout,stderr}` |
| Deno | `Deno.env.get` | `Deno.args` | `Deno.exit` | `Command.output()` awaitable |
| Go | `os.Getenv` | `os.Args` | `os.Exit` | `exec.Command.Output()` blocks |

Key observations:
1. Env: flat readable map everywhere. Write low-value and opens inheritance questions.
2. Args: Lune/Deno exclude runtime binary + script path (cleaner for scripting).
3. Spawn: synchronous-looking, returns `{ok, code, stdout, stderr}`.
4. Security: only Deno gates with `--allow-run`. neoc has no permission flag system yet — gate at module registration for v1.

## Decision

`std:process` — **Option A** (flat module, Lune-aligned):

```lua
local process = require("std:process")

process.env.DATABASE_URL     -- read-only metatable proxy (write deferred)
process.args                 -- table, excludes binary + script path
process.cwd                  -- string, captured eagerly at load time
process.exit(code)           -- terminates process

local result = process.spawn("ls", { "-la" }, {
    cwd    = "/tmp",
    env    = { PATH = "/usr/bin" },  -- replaces env entirely (not inherited)
    stdin  = "null",                  -- "null" (default) | "inherit"
    stdout = "pipe",                  -- "null" | "inherit" | "pipe" (default)
    stderr = "pipe",                  -- "null" | "inherit" | "pipe" (default)
})
-- result.ok      boolean (code == 0)
-- result.code    integer
-- result.stdout  string (always string, never nil)
-- result.stderr  string (always string, never nil)
```

**Rejected:** Option B (Command builder — adds indirection without capability gap under hidden-async). Option C (split surface — two mental models).

**Deferred:** `env` write access, `stdin = "pipe"` (streaming), permission gating (separate architectural ADR).

## Consequences

- Scripts can read env, access args, get cwd, exit, spawn subprocesses. Issue #7 fully satisfiable.
- `spawn` covers dominant case (run, get output, branch). Streaming deferred.
- `process.env` read-only — `opts.env` on spawn is the explicit path for child env.
- `process.exit` is hard termination. Bypasses `pcall`, `__gc`, `__close`, kills worker threads. Matches `os.exit`.
- Subprocess spawn is broadest OS-escape vector. Gated at registration only for v1. Future permission-model ADR addresses per-script grants.
- Implementation: `src/lua/std/process.rs`, wraps `tokio::process::Command`.

## References

- [Lune `process`](https://lune-org.github.io/docs/api-reference/process/)
- [Deno `Deno.Command`](https://deno.land/api?s=Deno.Command)
- [Rust `std::process::Command`](https://doc.rust-lang.org/std/process/struct.Command.html)
- ADR 0002, ADR 0006
- Issue: [flying-dice/neoc#7](https://github.com/flying-dice/neoc/issues/7)
