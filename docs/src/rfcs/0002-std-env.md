---
status: draft
created: 2026-05-05
implemented-in:
---

# 0002 — std:env — Environment, platform constants, and paths

> [!WARNING]
> This RFC is `draft`. It is under refinement and is **not** the contract.

## Summary

Add `std:env` as a first-class module in the neoc standard library. The module exposes platform constants (OS, architecture, ABI family), directory path queries, read-only environment-variable access, and CLI argument access — mirroring `std::env` from the Rust standard library. Environment mutation (`set_var`, `remove_var`) is intentionally excluded; system telemetry (hostname, CPU count, memory) is out of scope and routed to a separate issue.

## Motivation

Scripts that run on multiple platforms need three things: platform detection, environment-variable access, and path resolution. Every major runtime provides these; neoc has none. The absence forces authors to either hard-code paths and platform strings or use workarounds through the Lua `os` table that neoc does not expose.

This module closes that gap in one coherent surface. The name `std:env` is the Rust-faithful choice: Rust's analogous APIs live exclusively in `std::env` (constants, var, args, home_dir, temp_dir, current_dir). The alternative name `std:os` would mislead Rust users — `std::os` in Rust contains platform-specific extension traits, not the APIs proposed here.

## Peer runtime survey

All major runtimes provide these capabilities. The table below anchors the design to industry consensus.

| Concept | Rust | Go | Node | Python | Deno | Bun | Lune |
|---|---|---|---|---|---|---|---|
| OS name | `std::env::consts::OS` | `runtime.GOOS` | `os.platform()` | `sys.platform` | `Deno.build.os` | `process.platform` | `process.os` |
| Architecture | `std::env::consts::ARCH` | `runtime.GOARCH` | `os.arch()` | `platform.machine()` | `Deno.build.arch` | `process.arch` | `process.arch` |
| ABI family | `std::env::consts::FAMILY` | — | — | — | — | — | — |
| Exe suffix | `std::env::consts::EXE_SUFFIX` | — | — | — | — | — | — |
| DLL prefix/suffix | `std::env::consts::DLL_PREFIX/SUFFIX` | — | — | — | — | — | — |
| Home dir | `std::env::home_dir()` | `os.UserHomeDir()` | `os.homedir()` | `Path.home()` | — | — | — |
| Temp dir | `std::env::temp_dir()` | `os.TempDir()` | `os.tmpdir()` | `tempfile.gettempdir()` | — | — | — |
| Working dir | `std::env::current_dir()` | `os.Getwd()` | `process.cwd()` | `os.getcwd()` | `Deno.cwd()` | `process.cwd()` | `process.cwd` |
| Read env var | `std::env::var(k)` | `os.Getenv(k)` | `process.env[k]` | `os.environ[k]` | `Deno.env.get(k)` | `process.env[k]` | `process.env[k]` |
| Write env var | `std::env::set_var(k,v)` | `os.Setenv(k,v)` | `process.env[k]=v` | `os.environ[k]=v` | `Deno.env.set(k,v)` | `process.env[k]=v` | — |
| Remove env var | `std::env::remove_var(k)` | `os.Unsetenv(k)` | `delete process.env[k]` | `del os.environ[k]` | `Deno.env.delete(k)` | — | — |
| All env vars | `std::env::vars()` | `os.Environ()` | `process.env` | `os.environ` | `Deno.env.toObject()` | `process.env` | `process.env` |
| CLI args | `std::env::args()` | `os.Args[1:]` | `process.argv[2:]` | `sys.argv[1:]` | `Deno.args` | `process.argv[2:]` | `process.args` |

**Key observations:**

1. Every runtime provides platform detection, env vars, and args. No runtime omits any of these three.
2. Rust, Go, and Python provide home/temp/cwd resolution; Deno and Node surface these more selectively. neoc should include them (Rust mirror precedence).
3. `set_var`/`remove_var` exist in most runtimes, but Rust made them `unsafe` in Edition 2024 (stabilised in Rust 1.83). neoc excludes them from `std:env` — see the dedicated rationale section below.
4. System telemetry (hostname, CPU count, memory, uptime) is absent from every runtime's `env`-equivalent. It requires platform crates. This separability justifies routing telemetry to a distinct module.
5. Lune (the closest Luau precedent) puts everything under `process`. neoc does not follow Lune here because `std:env` provides a Rust-faithful boundary that separates environment from process lifecycle. Process lifecycle (`exit`, `abort`, `spawn`, `id`) lands in `std:process` (issue #7).

## Detailed design

### Module registration

```
src/lua/std/env.rs        -- module implementation
src/lua/std/mod.rs        -- registration (alphabetical insert)
tests/std/env.test.luau   -- Luau integration tests
```

### Lua surface

```lua
local env = require("std:env")
```

#### Platform constants (read-only properties)

These mirror `std::env::consts`. Values are compile-time constants; they do not change after the runtime starts.

```lua
env.os            -- "linux" | "macos" | "windows" | "freebsd" | ...
env.arch          -- "x86_64" | "aarch64" | "riscv64" | ...
env.family        -- "unix" | "windows"
env.exe_suffix    -- "" (unix) | ".exe" (windows)
env.dll_prefix    -- "lib" (unix) | "" (windows)
env.dll_suffix    -- ".so" | ".dylib" | ".dll"
```

All six values originate from `std::env::consts::{OS, ARCH, FAMILY, EXE_SUFFIX, DLL_PREFIX, DLL_SUFFIX}`. No mapping, no translation — raw values forwarded from Rust.

#### Directory path queries (fallible functions)

```lua
local home, err = env.home_dir()
-- home: string path on success, nil on failure
-- err:  nil on success, "env.home_dir: home directory not found" on failure
--       (there is only one failure mode; tests must assert the full string)

local tmp = env.temp_dir()
-- Returns a string. Infallible — falls back to OS default if $TMPDIR is unset.

local cwd, err = env.current_dir()
-- cwd: string path on success, nil on failure
-- err: nil on success, "env.current_dir: <os_error>" on failure
--   Note: the OS message portion of <os_error> is locale-dependent.
--   Tests must match on the "env.current_dir: " prefix only, not the full string.
```

`home_dir` is fallible: it returns `nil` when `HOME` (Unix) or `USERPROFILE` (Windows) is absent from the environment. An empty-string value for either variable is still considered present and will be returned as-is. The function does not perform filesystem access; "inaccessible" is not a failure mode. Negative tests must `unset HOME` (not set it to `""`), otherwise `home_dir` will return the empty string rather than `nil`. `temp_dir` is infallible — the OS always provides a temp directory. `current_dir` is fallible because the working directory may have been deleted after the process started.

All three return string paths (not `std:fs` `Path` objects). If `std:fs` gains a `Path` type, these will continue returning strings and the caller can wrap them. Avoiding a circular dependency on `std:fs` keeps `std:env` self-contained.

#### Environment variables (read-only)

```lua
local val, err = env.var("HOME")
-- val: string value on success, nil if not set or if the value is not valid UTF-8
-- err: nil on success
--      "env.var: HOME: environment variable not found"              (key absent)
--      "env.var: HOME: environment variable was not valid unicode"  (non-UTF-8 value)

local all = env.vars()
-- Returns a table: { KEY = "value", ... }. Infallible snapshot.
-- Non-UTF-8 entries: pairs where the key or value is not valid UTF-8 are silently skipped.
-- This mirrors the behaviour of Rust's std::env::vars_os() filtered to valid UTF-8.
-- A test that injects a non-UTF-8 var should assert the key is absent from the returned table.
```

`var` follows the `(value, err)` convention — the variable may not exist or may contain non-UTF-8 bytes; both are normal conditions, not bugs. The error strings are the raw `Display` output of `std::env::VarError` wrapped with the `"env.var: KEY: "` prefix: `"environment variable not found"` for `VarError::NotPresent` and `"environment variable was not valid unicode"` for `VarError::NotUnicode`. No custom error override is applied; the implementation must not fabricate different strings.

`vars()` silently skips any environment entry whose key or value is not valid UTF-8 (equivalent to iterating `std::env::vars_os()` and discarding non-UTF-8 pairs). This is the safest default: it avoids panics and avoids surfacing binary garbage to Lua. Tests that need to verify this boundary should unset or control the environment rather than relying on ambient non-UTF-8 entries.

#### Exclusion of `set_var` and `remove_var`

`set_var` and `remove_var` are **intentionally omitted** from this module. The rationale draws on three independent lines of reasoning that all converge on the same conclusion:

**1. Rust safety precedent.** Rust made `std::env::set_var` and `std::env::remove_var` `unsafe` in Edition 2024 (stabilised in Rust 1.83) because calling them while another thread reads env vars is undefined behaviour on POSIX. Mirroring Rust faithfully (design principle 3) means we must not expose these as casual safe calls.

**2. Sendable concurrency model.** RFC 0001 established that cross-engine state sharing must be explicit, validated at spawn time, and follow a DAG rooted at spawn. `env.set_var("KEY", "value")` is process-global mutable state accessible to every engine — the exact pattern the Sendable model was designed to eliminate.

**3. Cross-language evidence:**

- **Systems (Rust, Go, C, C++):** Rust made it `unsafe`. Go locks it but it remains process-global. C/C++ leave it ungated and unsound. The idiomatic pattern in all four is `Command::env()` / `exec.Cmd.Env` — spawn-scoped mutation, never parent-global.
- **Managed (Java, Kotlin, Groovy, Scala, C#):** The JVM never shipped a public `setenv` equivalent — a deliberate omission, with concurrency cited as the deciding factor. `ProcessBuilder.environment()` and `ProcessStartInfo.Environment` are the sanctioned paths; both are spawn-scoped.
- **Scripting (Node, Bun, Deno, Lune):** Node and Bun expose ungated writes and are widely criticised for it — test-isolation bugs in the Node ecosystem for years. Deno, the runtime that corrected Node's mistakes, gates env access behind `--allow-env`. Lune exposes ungated writes, but Lune is single-threaded and does not face multi-engine concurrency.

**Where env mutation belongs:** `std:process` (issue #7), scoped to the child process descriptor:

```lua
local process = require("std:process")
local result = process.spawn("my-tool", { "--flag" }, {
    env = { DATABASE_URL = "postgres://...", PATH = env.var("PATH") },
    cwd = "/some/dir",
})
```

This is where every major runtime agrees env mutation is safe — spawn-scoped, never parent-global.

#### Exclusion of `set_current_dir`

`set_current_dir()` is intentionally omitted for the same reason as `set_var` — it would be process-global mutable state outside the Sendable protocol. Workers that need a different working directory should use `process.spawn()` options.

#### CLI arguments

```lua
local args = env.args()
-- Returns an array table of strings: { "arg1", "arg2", ... }
-- Script name is NOT included (mirrors Deno.args and Lune process.args).
-- Infallible.
-- In library mode (no _NEOC_ARGS global set), returns an empty table — not an error.
```

The script path itself is excluded from `args()`. The rationale: authors want the arguments their script received, not the runtime's own argv. Deno, Lune, and Python's `sys.argv[1:]` convention all agree on this boundary.

**Library-mode test setup:** `args()` reads the `_NEOC_ARGS` Lua global injected by the runner at startup (see Implementation Notes). To test the normal path, set `_NEOC_ARGS = {"arg1", "arg2"}` before calling `args()`. To test library mode (empty table), ensure `_NEOC_ARGS` is not set. These are the only two observable states.

### Error format

All error strings follow the `"module.function: <detail>"` convention documented in the project's key conventions (`CLAUDE.md` at the repository root):

```
"env.var: HOME: environment variable not found"
"env.var: HOME: environment variable was not valid unicode"
"env.home_dir: home directory not found"
"env.current_dir: <os_error>"
```

`var` error strings are the raw `Display` of `std::env::VarError` prefixed with `"env.var: KEY: "`. OS errors (e.g. `current_dir`) are locale-dependent; the `"env.current_dir: "` prefix is stable but the OS message portion is not. Tests must match on the prefix only.

### Rust implementation sketch

The sketch below references two helpers, `ok` and `from_err`, which are defined in `src/lua/helpers.rs` (or equivalent). They wrap the `(value, nil)` / `(nil, err)` Lua return convention used throughout the codebase.

```rust
// src/lua/std/env.rs
use mlua::{Lua, Result, Table, Value};

pub fn module(lua: &Lua) -> Result<Table> {
    let t = lua.create_table()?;

    // Platform constants
    t.set("os",         std::env::consts::OS)?;
    t.set("arch",       std::env::consts::ARCH)?;
    t.set("family",     std::env::consts::FAMILY)?;
    t.set("exe_suffix", std::env::consts::EXE_SUFFIX)?;
    t.set("dll_prefix", std::env::consts::DLL_PREFIX)?;
    t.set("dll_suffix", std::env::consts::DLL_SUFFIX)?;

    // home_dir: fallible, returns (path, nil) or (nil, err)
    t.set("home_dir", lua.create_function(|lua, ()| {
        match std::env::home_dir() {
            Some(p) => ok(lua, p.to_string_lossy().as_ref()),
            None    => from_err(lua, "env.home_dir: home directory not found"),
        }
    })?)?;

    // temp_dir: infallible
    t.set("temp_dir", lua.create_function(|_, ()| {
        Ok(std::env::temp_dir().to_string_lossy().into_owned())
    })?)?;

    // current_dir: fallible
    t.set("current_dir", lua.create_function(|lua, ()| {
        match std::env::current_dir() {
            Ok(p)  => ok(lua, p.to_string_lossy().as_ref()),
            Err(e) => from_err(lua, &format!("env.current_dir: {e}")),
        }
    })?)?;

    // var: fallible — error strings are the raw Display of VarError, not custom strings
    t.set("var", lua.create_function(|lua, key: String| {
        match std::env::var(&key) {
            Ok(v)  => ok(lua, v),
            Err(e) => from_err(lua, &format!("env.var: {key}: {e}")),
        }
    })?)?;

    // vars: infallible snapshot — non-UTF-8 pairs are silently skipped.
    t.set("vars", lua.create_function(|lua, ()| {
        let tbl = lua.create_table()?;
        for (k, v) in std::env::vars_os() {
            if let (Some(k), Some(v)) = (k.to_str(), v.to_str()) {
                tbl.set(k, v)?;
            }
        }
        Ok(tbl)
    })?)?;

    // args: infallible, script args only
    t.set("args", lua.create_function(|lua, ()| {
        let globals = lua.globals();
        match globals.get::<Vec<String>>("_NEOC_ARGS") {
            Ok(args) => Ok(args),
            Err(_)   => Ok(Vec::new()),
        }
    })?)?;

    Ok(t)
}
```

### Type definition stub

```lua
-- types/std/env.d.luau
declare env: {
    os:         string,
    arch:       string,
    family:     string,
    exe_suffix: string,
    dll_prefix: string,
    dll_suffix: string,

    home_dir:    () -> (string?, string?),
    temp_dir:    () -> string,
    current_dir: () -> (string?, string?),

    var:        (key: string) -> (string?, string?),
    vars:       () -> { [string]: string },

    args:       () -> { string },
}
```

> **Type-wideness note:** `(string?, string?)` permits the degenerate cases `(nil, nil)` and `(string, string)`, neither of which is a valid return. The actual contract is `(string, nil) | (nil, string)`, but Luau's type system cannot express this disjunction — `(string?, string?)` is the narrowest expressible approximation. This is a Luau limitation, not a spec ambiguity.

## Security considerations

`std:env` is a capability. Granting it to a script gives that script read access to all environment variables — which may include secrets, credentials, and configuration.

1. **No implicit grant.** The module is only available when the script explicitly `require`s it. Scripts that do not require `std:env` have no access to the process environment.
2. **Read-only by design.** No write access to the process environment is exposed. This eliminates an entire class of env-poisoning attacks where a malicious script modifies `PATH`, `LD_PRELOAD`, or similar variables to affect subsequently spawned processes.

If neoc moves to an explicit capability-grant model (e.g. `--allow-env` à la Deno), `std:env` is the natural boundary at which that gate would be enforced. This RFC does not implement capability gating, but the module boundary is designed so it can be added without API change.

## Drawbacks

- **`home_dir` is deprecated in Rust 1.x** (marked `#[deprecated]` since 1.29.0; see [rust-lang/rust#71898](https://github.com/rust-lang/rust/issues/71898) for the tracking issue). The idiomatic replacement is the `dirs` crate. `std:env` uses `home_dir` rather than pulling in `dirs` because: (a) this is `std:*` — no external crates; (b) the deprecation notice is about edge cases on Windows where the env vars `USERPROFILE`/`HOME` may be stale. For the runtime use-case, `std::env::home_dir` is still the correct, dependency-free choice.
- **Path strings, not Path objects.** Returning raw strings is simpler but loses type-level path safety. Acceptable tradeoff — `std:fs` integration can be done later without breaking this surface.
- **No env mutation in this module.** Scripts that need to set environment variables for child processes must use `std:process` (issue #7). This is intentional and well-motivated (see exclusion rationale above), but it means `std:process` must ship before env-mutation workflows are possible.

## Alternatives

### `std:os` as the module name

Rejected. Rust's `std::os` holds platform-specific extension traits (`std::os::unix`, `std::os::windows`), not the APIs proposed here. Naming this `std:os` violates design principle 3 (mirror Rust first) and would mislead Rust developers.

### Put env vars under `std:process`

Rejected. Issue #7 (`std:process`) covers process lifecycle: `exit`, `abort`, `spawn`, `id`. Merging env vars into `std:process` conflates two distinct Rust modules (`std::env` and `std::process`).

### Expose env vars as a mutable table (Node/Bun style: `process.env.KEY = val`)

Rejected. Luau does not support `__newindex` on shared module tables in a way that hooks cleanly into Rust. The functional API (`env.var`) is explicit, consistent with the `(value, err)` return convention, and maps directly to the Rust backing calls. Magic table mutation is a footgun (principle 2).

### Include `set_var` / `remove_var` (mirroring Rust's original safe API)

Rejected. While most runtimes include env mutation, Rust made it `unsafe` in Edition 2024 for good reason. neoc's Sendable concurrency model independently prohibits process-global mutable state accessible to all engines. The idiomatic answer — `process.spawn({ env = {...} })` — is spawn-scoped and safe. Including `set_var` would violate principle 2 (no footguns) and contradict RFC 0001's concurrency guarantees.

### System telemetry in this module

Rejected. Hostname, CPU count, total/free memory, and uptime require the `sysinfo` crate — outside `std:*` namespace rules (no external crates). They belong in `lib:sysinfo` or `vnd:sysinfo`.

## Open questions

1. **`home_dir` deprecation path.** If neoc introduces a `lib:dirs` module wrapping the `dirs` crate, `env.home_dir()` could delegate to it. For now, `std::env::home_dir` is used directly.

2. **Capability gating.** If neoc adopts Deno-style `--allow-env` flags, `std:env` is the natural boundary. This RFC does not design the capability model.

## Implementation notes

- **File:** `src/lua/std/env.rs` — single `pub fn module(lua: &Lua) -> mlua::Result<Table>`.
- **Registration:** `src/lua/std/mod.rs`, alphabetically between `collections` and `fs`.
- **`ok`/`from_err` helpers:** defined in `src/lua/helpers.rs` (or equivalent); used throughout the codebase for the `(value, nil)` / `(nil, err)` Lua return convention.
- **`args()` wiring:** `src/main.rs` calls `engine.globals().set("_NEOC_ARGS", ...)` at startup with the script-args slice. `env.args()` reads this global. If no such global exists at call time, return an empty table (not an error) — this covers library-mode usage and is a testable contract.
- **`home_dir` deprecation:** Add `#[allow(deprecated)]` at the call site with a comment explaining why `std::env::home_dir` is preferred over `dirs` crate in `std:*` context.
- **Tests:** `tests/std/env.test.luau` — cover all surface functions. Platform-constant assertions are conditional on `env.os`. Tests for `current_dir` error must match on the `"env.current_dir: "` prefix only, not the full OS error string.
- **`.d.luau` stub:** `types/std/env.d.luau` — required per M1 Definition of Done.
- **Criterion bench:** gated behind `bench` feature, even if trivial — keeps the bench suite complete.
- **Sequencing:** No upstream dependency. Can be picked up immediately once this RFC is accepted. Does not block and is not blocked by #7 (`std:process`) — the boundary is clean.
- **Impact on issue #7:** The original issue #7 proposal places `process.env`, `process.args`, and `process.cwd` under `std:process`. With `std:env` owning read access to env/args/cwd, issue #7 narrows to process lifecycle (`exit`, `abort`, `spawn`, `id`) plus spawn-scoped env mutation via `process.spawn({ env = {...} })`.
