---
status: draft
created: 2026-05-05
implemented-in:
---

# 0004 — std:env — Environment, platform constants, and paths

> [!WARNING]
> This RFC is `draft`. Under refinement — **not** the contract.

## Summary

`std:env` exposes platform constants, directory queries, read-only env-var access, CLI args. Mirrors `std::env` from Rust. Mutation excluded. Telemetry out of scope.

## Motivation

Cross-platform scripts need platform detection, env-var access, path resolution. Every major runtime provides all three; neoc has none.

`std:env` closes that gap. Name follows Rust (`std::env` owns constants, var, args, dirs). `std:os` rejected — Rust's `std::os` = platform-specific extension traits.

## Peer runtime survey

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

**Observations:**

1. Platform detection + env vars + args — universal across runtimes.
2. Rust/Go/Python include home/temp/cwd → neoc includes (Rust mirror precedence).
3. `set_var`/`remove_var` — Rust made `unsafe` in Edition 2024. neoc excludes.
4. Telemetry absent from all env-equivalents → routes to distinct module.
5. Lune uses `process` for everything. neoc separates env from process lifecycle (Rust-faithful). Process lifecycle → `std:process` (#7).

## Detailed design

### Module registration

```
src/lua/std/env.rs        -- module implementation
src/lua/std/mod.rs        -- registration (alphabetical insert)
tests/std/env.test.lua   -- Lua integration tests
```

### Lua surface

```lua
local env = require("std:env")
```

#### Platform constants (read-only)

Mirror `std::env::consts`. Compile-time, immutable.

```lua
env.os            -- "linux" | "macos" | "windows" | "freebsd" | ...
env.arch          -- "x86_64" | "aarch64" | "riscv64" | ...
env.family        -- "unix" | "windows"
env.exe_suffix    -- "" (unix) | ".exe" (windows)
env.dll_prefix    -- "lib" (unix) | "" (windows)
env.dll_suffix    -- ".so" | ".dylib" | ".dll"
```

All six from `std::env::consts::{OS, ARCH, FAMILY, EXE_SUFFIX, DLL_PREFIX, DLL_SUFFIX}`. Raw values, no mapping.

#### Directory path queries (fallible)

```lua
local home, err = env.home_dir()
-- home: string path on success, nil on failure
-- err:  nil on success, "env.home_dir: home directory not found" on failure
--       (single failure mode; tests assert full string)

local tmp = env.temp_dir()
-- Infallible — falls back to OS default if $TMPDIR unset.

local cwd, err = env.current_dir()
-- cwd: string path on success, nil on failure
-- err: nil on success, "env.current_dir: <os_error>" on failure
--   OS message portion is locale-dependent.
--   Tests match "env.current_dir: " prefix only.
```

**Semantics:**

- `home_dir` — returns `nil` when `HOME` (Unix) / `USERPROFILE` (Windows) absent. Empty string = present, returned as-is. No filesystem access. Negative tests: `unset HOME`, not `""`.
- `temp_dir` — infallible. OS always provides one.
- `current_dir` — fallible. CWD may be deleted after process start.

All return string paths (not `std:fs` `Path` objects). No circular dep on `std:fs`.

#### Environment variables (read-only)

```lua
local val, err = env.var("HOME")
-- val: string on success, nil if absent or non-UTF-8
-- err: nil on success
--      "env.var: HOME: environment variable not found"              (absent)
--      "env.var: HOME: environment variable was not valid unicode"  (non-UTF-8)

local all = env.vars()
-- Table: { KEY = "value", ... }. Infallible snapshot.
-- Non-UTF-8 pairs silently skipped.
```

**`var`** — `(value, err)` convention. Error strings = raw `Display` of `std::env::VarError` with `"env.var: KEY: "` prefix.

**`vars()`** — iterates `std::env::vars_os()`, discards non-UTF-8 pairs. Avoids panics/binary garbage in Lua.

#### Exclusion of `set_var` and `remove_var`

Intentionally omitted. Three converging rationales:

**1. Rust safety precedent.** `set_var`/`remove_var` = `unsafe` in Edition 2024 (concurrent reads during mutation → UB on POSIX). Principle 3 prohibits exposing as safe.

**2. Sendable concurrency model.** RFC 0001: cross-engine state = explicit, validated at spawn, DAG-rooted. `env.set_var()` = process-global mutable state visible to all engines — exactly what Sendable eliminates.

**3. Cross-language evidence:**

- **Systems (Rust, Go, C, C++):** Rust → `unsafe`. Go → locked but process-global. C/C++ → ungated, unsound. Idiom: `Command::env()` / `exec.Cmd.Env` — spawn-scoped.
- **Managed (Java, Kotlin, Groovy, Scala, C#):** JVM never shipped public `setenv` — concurrency-motivated. `ProcessBuilder.environment()` / `ProcessStartInfo.Environment` — spawn-scoped.
- **Scripting (Node, Bun, Deno, Lune):** Node/Bun ungated → test-isolation bugs. Deno gates behind `--allow-env`. Lune ungated but single-threaded.

**Env mutation belongs in `std:process` (#7)**, spawn-scoped:

```lua
local process = require("std:process")
local result = process.spawn("my-tool", { "--flag" }, {
    env = { DATABASE_URL = "postgres://...", PATH = env.var("PATH") },
    cwd = "/some/dir",
})
```

Every major runtime agrees: spawn-scoped mutation, never parent-global.

#### Exclusion of `set_current_dir`

Omitted — same rationale as `set_var`. Process-global mutable state outside Sendable. Use `process.spawn()` `cwd` option.

#### CLI arguments

```lua
local args = env.args()
-- Array table of strings: { "arg1", "arg2", ... }
-- Script name excluded (mirrors Deno.args, Lune process.args).
-- Absent global (library mode): returns {} — not an error.
-- Malformed global (wrong type): raises error() — runner bug.
```

Script path excluded — authors want their args, not runtime argv. Deno, Lune, Python `sys.argv[1:]` agree.

| `_NEOC_ARGS` state | Result |
|---|---|
| Absent (`nil`) | `{}` — library mode |
| Present, `{string}` | Returns args table |
| Present, wrong type | `error("env.args: _NEOC_ARGS: expected array of strings")` — runner bug |

**Test setup:** Normal → set `_NEOC_ARGS = {"arg1", "arg2"}`. Library mode → ensure unset. Error → set to non-table, assert error string.

### Error format

Convention: `"module.function: <detail>"` (per `CLAUDE.md`).

```
"env.var: HOME: environment variable not found"
"env.var: HOME: environment variable was not valid unicode"
"env.home_dir: home directory not found"
"env.current_dir: <os_error>"
```

`var` errors — raw `Display` of `VarError`, prefixed `"env.var: KEY: "`. OS errors locale-dependent; prefix stable, message not. Tests match prefix only.

### Rust implementation sketch

Helpers `ok`/`from_err` from `src/lua/helpers.rs` — `(value, nil)` / `(nil, err)` convention.

```rust
// src/lua/std/env.rs
use mlua::{Lua, Result, Table, Value};

pub fn module(lua: &Lua) -> Result<Table> {
    let t = lua.create_table()?;

    t.set("os",         std::env::consts::OS)?;
    t.set("arch",       std::env::consts::ARCH)?;
    t.set("family",     std::env::consts::FAMILY)?;
    t.set("exe_suffix", std::env::consts::EXE_SUFFIX)?;
    t.set("dll_prefix", std::env::consts::DLL_PREFIX)?;
    t.set("dll_suffix", std::env::consts::DLL_SUFFIX)?;

    t.set("home_dir", lua.create_function(|lua, ()| {
        match std::env::home_dir() {
            Some(p) => ok(lua, p.to_string_lossy().as_ref()),
            None    => from_err(lua, "env.home_dir: home directory not found"),
        }
    })?)?;

    t.set("temp_dir", lua.create_function(|_, ()| {
        Ok(std::env::temp_dir().to_string_lossy().into_owned())
    })?)?;

    t.set("current_dir", lua.create_function(|lua, ()| {
        match std::env::current_dir() {
            Ok(p)  => ok(lua, p.to_string_lossy().as_ref()),
            Err(e) => from_err(lua, &format!("env.current_dir: {e}")),
        }
    })?)?;

    t.set("var", lua.create_function(|lua, key: String| {
        match std::env::var(&key) {
            Ok(v)  => ok(lua, v),
            Err(e) => from_err(lua, &format!("env.var: {key}: {e}")),
        }
    })?)?;

    t.set("vars", lua.create_function(|lua, ()| {
        let tbl = lua.create_table()?;
        for (k, v) in std::env::vars_os() {
            if let (Some(k), Some(v)) = (k.to_str(), v.to_str()) {
                tbl.set(k, v)?;
            }
        }
        Ok(tbl)
    })?)?;

    t.set("args", lua.create_function(|lua, ()| {
        let globals = lua.globals();
        match globals.get::<mlua::Value>("_NEOC_ARGS") {
            Ok(mlua::Value::Nil) => Ok(Vec::new()),
            Ok(mlua::Value::Table(tbl)) => {
                tbl.sequence_values::<String>()
                    .collect::<mlua::Result<Vec<String>>>()
                    .map_err(|_| mlua::Error::runtime(
                        "env.args: _NEOC_ARGS: expected array of strings",
                    ))
            }
            Ok(_) => Err(mlua::Error::runtime(
                "env.args: _NEOC_ARGS: expected array of strings",
            )),
            Err(_) => Ok(Vec::new()),
        }
    })?)?;

    Ok(t)
}
```

### Type definition stub

```lua
-- types/std/env.d.lua
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

> **Type-wideness:** `(string?, string?)` permits `(nil, nil)` and `(string, string)` — neither valid. Actual contract: `(string, nil) | (nil, string)`. Luau cannot express this disjunction. Narrowest approximation.

## Security considerations

`std:env` = capability granting read access to all env vars (may include secrets).

1. **No implicit grant.** Only via explicit `require`. No require = no access.
2. **Read-only.** No writes → eliminates env-poisoning (`PATH`, `LD_PRELOAD` modification).

Future `--allow-env` gate (Deno-style) would enforce at this module boundary. Not designed here; boundary supports it without API change.

## Drawbacks

- **`home_dir` deprecated in Rust** (since 1.29.0, [rust-lang/rust#71898](https://github.com/rust-lang/rust/issues/71898)). Idiomatic: `dirs` crate. Used here because: `std:*` = no external crates; deprecation = Windows edge cases only.
- **String paths, not Path objects.** Simpler, loses type safety. `std:fs` integration later without breaking surface.
- **No env mutation.** Must use `std:process` (#7) for child-process env. Intentional — `std:process` ships first for mutation workflows.

## Alternatives

### `std:os` as module name

Rejected. `std::os` in Rust = platform-specific extension traits. Violates principle 3.

### Env vars under `std:process`

Rejected. #7 = process lifecycle (`exit`, `abort`, `spawn`, `id`). Conflates two distinct Rust modules.

### Mutable table (Node/Bun `process.env.KEY = val`)

Rejected. Lua `__newindex` doesn't hook cleanly into Rust. Functional API explicit, consistent with `(value, err)`. Magic table mutation = footgun (principle 2).

### Include `set_var` / `remove_var`

Rejected. Rust `unsafe` in Edition 2024. Sendable prohibits process-global mutable state. `process.spawn({ env = {...} })` = spawn-scoped, safe. Violates principle 2, contradicts RFC 0001.

### System telemetry here

Rejected. Requires `sysinfo` crate — outside `std:*` rules. Belongs in `lib:sysinfo` or `vnd:sysinfo`.

## Open questions

1. **`home_dir` deprecation path.** Future `lib:dirs` could replace. For now, `std::env::home_dir` used directly.
2. **Capability gating.** `--allow-env` would gate at `std:env` boundary. Not designed in this RFC.

## Implementation notes

- **File:** `src/lua/std/env.rs` — single `pub fn module(lua: &Lua) -> mlua::Result<Table>`.
- **Registration:** `src/lua/std/mod.rs`, alphabetically between `collections` and `fs`.
- **Helpers:** `ok`/`from_err` in `src/lua/helpers.rs`.
- **`args()` wiring:** `src/main.rs` sets `_NEOC_ARGS` global at startup. Absent → empty table. Wrong type → `error()`.
- **`home_dir`:** `#[allow(deprecated)]` at call site.
- **Tests:** `tests/std/env.test.lua` — all surface functions. Platform assertions conditional on `env.os`. `current_dir` error tests match prefix only.
- **`.d.lua`:** `types/std/env.d.lua` — required per M1 DoD.
- **Bench:** Criterion, gated behind `bench` feature.
- **Sequencing:** No upstream dep. Not blocked by #7. Boundary clean.
- **Impact on #7:** Original #7 placed `process.env`, `process.args`, `process.cwd` under `std:process`. With `std:env` owning read access, #7 narrows to lifecycle + spawn-scoped env mutation.
