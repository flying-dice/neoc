# Architecture

How `neoc` is structured: a LuaJIT runtime with a curated module surface (unsafe Lua globals stripped, only registered modules load; not a security boundary), and a thin binary over it.

## Shape

```
neoc run script.lua
   │  clap parse, glob expand        crates/neoc
   ▼
engine construction (sandbox)        crates/neoc-vm  lua/sandbox.rs
   │  strip unsafe stdlib, install `require`
   ▼
LuaJIT VM  ── require("ns:name") ──▶ std:* / lib:* / vnd:* modules
                                     crates/neoc-vm  lua/{std,lib,vnd}/
```

Each script runs in its own LuaJIT VM on a tokio runtime the binary owns. There are no ambient globals: `require("namespace:name")` is the only way to a module. See [ADR 0002](docs/src/adrs/0002-strip-unsafe-lua-stdlib.md) and [ADR 0003](docs/src/adrs/0003-three-namespace-module-structure.md).

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| `neoc-vm` | the LuaJIT runtime library: engine construction, sandbox, `require`, script execution support; owns the `mlua`/`tokio` stack and every `std:*` / `lib:*` / `vnd:*` module | (runtime deps) |
| `neoc` | the `neoc` binary: the `clap` tree (`run`), glob expansion, the tokio runtime, process exit policy, embedded-payload executables | vm |

The dependency graph is acyclic and `neoc-vm` depends on no other workspace crate.

## Modules

`crates/neoc-vm/src/lua/` holds one file per module, registered in `modules.rs`:

- `std:*` — mirrors Rust's standard library.
- `lib:*` — blessed gap-fillers and custom modules.
- `vnd:*` — vendored crates, named after the upstream crate.

`facade.rs` is the registration builder: every function gets a per-call `tracing` span and, for the fallible forms, the `(value, error)` return shape ([ADR 0005](docs/src/adrs/0005-tuple-error-convention.md)). It also records each function's signature and doc. Nothing renders that record yet; it is kept for tooling such as editor type generation.

## Testing

- **Rust** — `#[cfg(test)] mod tests` next to each module; `crates/neoc/tests/cli.rs` drives the built binary.
- **Lua** — `crates/neoc/tests/<ns>/<name>.test.lua`, run with `neoc run`. Suites that need Docker (`sqlx_mysql`, `sqlx_postgres`, `testcontainers_modules`) are excluded from CI.
- **CLI spec** — `tooling/cli/*.feature`, Gherkin driven against the built binary by `crates/neoc/tests/cucumber.rs`.
- **Benches** — Criterion suites in `crates/neoc-vm/benches`, behind the `bench` feature.

## Boundaries (enforced)

- Every module and every module dependency lives in `neoc-vm`. The binary uses `mlua` only for the error type a failing script surfaces and `tokio` for the runtime it drives the engine on.
- A module's tracing target is `lua::lib::<module>` regardless of namespace.
- `unsafe_code` is denied workspace-wide.

## History

neoc was also a language that compiled to Lua. NEOC-106 removed it; the last state is preserved under the `neoc-lang-final` tag. See [ADR 0012](docs/src/adrs/0012-drop-the-neoc-language.md).
