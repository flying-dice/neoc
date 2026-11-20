# CLAUDE.md

Guidance for Claude (and any LLM agent) working in **neoc**: one Cargo workspace holding a Lua runtime.

- **Runtime** (`crates/neoc-vm`): an embedded LuaJIT engine backed by Rust. Scripts run in a VM with a curated module surface (`std:*`, `lib:*`, `vnd:*`). The unsafe halves of the Lua standard library are stripped; everything else comes through explicit modules. Not a security boundary.
- **Binary** (`crates/neoc`): the thin `neoc` CLI over the runtime. `run` only.

neoc used to be a language that compiled to Lua. That is gone; the last state is under the `neoc-lang-final` tag. Restore a path with `git checkout neoc-lang-final -- <path>`.

Read [README.md](README.md) for what neoc is and [CONTRIBUTING.md](CONTRIBUTING.md) for the build, the pre-PR checks, how to add a module and how to release.

## AgentOS

This repository also holds **AgentOS**, an iPadOS-style virtual desktop for agents (to be renamed vos): the host (`crates/agentos-*`, the `agentos-host` binary), the apps (`apps/`, plain Lua 5.1 run on `neoc-vm`), the client (`client/`) and the UI kit (`packages/ui`). It has its own gate, boundaries, test layout and skills, all governed by [AGENTS.md](AGENTS.md), which stays as it is: read it before touching any of that. Its skills are the `agentos-*` ones in `.claude/skills/` (`agentos-pre-commit`, `agentos-clean-code-review`, `agentos-refactor`, `agentos-code-review`); the unprefixed `pre-commit`, `clean-code-review` and `refactor` are neoc's.

The AgentOS adapter (`crates/agentos-lua`) is an embedder of `neoc-vm`: AgentOS policy (apps, `readOnly`, views, `app.*`, the `@agentos/*` modules) stays in that adapter and never moves into `neoc-vm`. The AgentOS crates keep edition 2021 and inherit the workspace lint table; the neoc crates do not yet (see "Rust style").

## Layout

```
crates/neoc/                 # the `neoc` binary (the AgentOS host is the other bin, `agentos-host`)
  src/main.rs                # CLI entry: `run`, embedded-payload executables
  tests/                     # Lua suites (*.test.lua, *.spec.lua), cli.rs, cucumber.rs
crates/neoc-vm/              # the runtime library
  tests/script_parse.rs      # compiles (never runs) every Lua script, Docker or not
  src/errors.rs              # runtime error types (NeocError)
  src/lua/sandbox.rs         # engine construction, stripped globals, the guarded `xpcall`
  src/lua/lifecycle.rs       # the embedding hooks: Life, CallEntry/Baton, async_function/async_method, Role
  src/lua/script_env.rs      # a sealed environment for one script (+ async_coroutines.lua)
  src/lua/facade.rs          # Module builder: registration, spans, (value, error), the LuaCATS emitter
  src/lua/modules.rs         # module registration + `require`
  src/lua/{std,lib,vnd}/     # one file per module
  examples/luals.rs          # writes (or `-- --check`s) the LuaCATS definitions in types/luals/
  tests/embedding.rs         # the embedding hooks on LuaJIT: yield boundaries, environments, ownership
  tests/lifetimes.rs         # what a script leaves running (`neoc run` and under a Life), worker module sets
  tests/signals.rs           # Ctrl-C and the process shutdown signal against serve_static and server:wait
  tests/luals.rs             # drift check of types/luals/, no colon in a file name, the lua-language-server fixtures
  benches/                   # Criterion suites (lib/, vnd/), behind the `bench` feature
types/luals/                 # GENERATED LuaCATS definitions, one file per module (`lib/json.lua` for `lib:json`)
types/luals-plugin.lua       # lua-language-server plugin: require("lib:json") -> types/luals/lib/json.lua
types/fixtures/              # lua-language-server fixtures: usage.lua (clean) and negative/ (expect-error)
.luarc.json                  # lua-language-server config for the repo (LuaJIT, library = types/luals, the plugin)
tooling/cli/                 # Gherkin spec for the CLI, run against the built binary
```

The crate boundary is enforced by `Cargo.toml`. `neoc-vm` owns the `mlua`/`tokio` stack, the sandbox and every module, and depends on no other workspace crate. `neoc` owns the CLI, glob expansion, the tokio runtime and process exit policy. If a need crosses the boundary, lift it into `neoc-vm`'s API; don't shortcut.

## Build and test

```sh
cargo build                                          # debug build
cargo test --workspace                               # Rust, CLI integration and the Gherkin spec
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.lua' 'crates/neoc/tests/**/*.spec.lua'   # all Lua suites (Docker ones need a daemon)
cargo bench -p neoc-vm --features bench --bench lib --bench vnd -- --test   # bench smoke
cargo clippy --workspace --all-targets               # lint
cargo fmt --check                                    # formatting
```

`--workspace` also builds and tests the AgentOS crates, whose tests need `bun` and `darklua` on `PATH`; `-p neoc -p neoc-vm` limits a command to neoc's own crates. The full pre-PR gate, including the Lua suite set that needs no Docker, is in [CONTRIBUTING.md](CONTRIBUTING.md#pre-pr-checks).

## Modules

Three namespaces, chosen at registration in `crates/neoc-vm/src/lua/modules.rs`. `require("namespace:name")` is the only way in.

- `std:*` mirrors Rust std (`std:fs`, `std:net`). Dependencies: std plus [blessed.rs](https://blessed.rs/crates).
- `lib:*` holds gap-fillers and custom modules (`lib:base64`, `lib:test`). Adds one focused vendored crate when that is the module.
- `vnd:*` wraps exactly one vendored crate; the name matches the upstream crate (`vnd:hyper`, `vnd:serde_json`).

A non-blessed crate needs the owner's sign-off.

### Design principles

Priority order, 1 > 2 > 3 > 4:

1. **No surprises.** Behave like every other major runtime. If Rust, Go, Python and Node agree, follow them.
2. **No footguns.** Never ship an API that is unintuitive or causes silent mistakes. If a design needs a warning paragraph, redesign it.
3. **Mirror Rust first.** The primary API is `UserData` types mirroring `std::*` 1:1 (same methods, same semantics).
4. **Scripting ergonomics on top.** Flat functions (Python/Node style) delegate to the `UserData` layer and never make their own design decisions.

### Conventions

- Every module exports `pub fn module(lua: &Lua) -> mlua::Result<Table>`. The table is the public surface; Rust items stay private. No globals.
- **Errors.** The shape is fixed per function. A recoverable failure (file missing, network down) returns `(value, err)` through the `ok` / `from_err` helpers: `(value, nil)` on success, `(nil, "module.function: message")` on failure. A programming error (wrong type, closed handle) raises. Infallible functions return the value directly.
- **Error prefixes** are `module.function:` (the bare module name, no namespace), and stable: scripts match on them.
- **Hidden async.** I/O looks synchronous: a call such as `hyper.get(url)` returns its result directly. No futures, no `:await()`, no callbacks. Concurrency is opt-in through `std:task` (and batch functions where a module has them).
- **Naming.** A `UserData` type is named after its Rust type (`Url`, `Hmac`, `Map`). Constructors live on a sub-table, as in `url.Url.parse(...)` and `hmac.Hmac.new(...)`. Mutators follow Rust names (`push`, `set_path`). Avoid `new_with_*` and builders.
- **Constructor names.** Naming guidance for new constructors; the names below are a rule, not a list of what exists today.

  | Situation | Use |
  |---|---|
  | Bare, default | `new()` |
  | Wrapping an existing value | `from_lua()`, `from_file()` |
  | Config struct | `from_options()` |
  | Raw data | `from_snapshot()` |
  | Opinionated preset | `with_stdlib()`, `with_sandbox()` |

  If `new` could be more descriptive without feeling redundant, use the descriptive name.
- **Flat vs `UserData`.** Read methods return Lua values (strings, numbers, tables); methods that return a domain object return `UserData`. Flat functions return strings and tables, never `UserData` (except a `parse()` alias). They use scripting convention (an extension helper returns `".txt"`, with the dot); `UserData` mirrors Rust (the `extension()` method returns `"txt"`). A stateless module (`lib:base64`) needs only the flat surface.
- **Resources.** OS resources are `UserData` handles with methods. Operations across several handles are free functions on the module table.
- **No speculative abstractions.** Add a helper only when 3+ call sites exist.
- A module's tracing target is `lua::lib::<module>` whatever its namespace.
- **Types.** Every module's `declare` records its surface (functions, parameter and return types, docs, resource types with their methods, fields and constructors) and that record is the source of the editor types in `types/luals/`, never edited by hand: `cargo run -p neoc-vm --example luals` regenerates, `-- --check` fails on drift. Keep the record true to the runtime; the drift tests in `modules.rs` check, in both directions, that the live module table and the recorded functions and fields agree and that a live value of each resource type and its recorded members agree (every recorded member exists; for a plain table or a userdata whose methods sit in a table, every live method is recorded; a userdata with fields is checked one way only), and that every named type has a class. They do not check parameter or return types against the runtime, and the types they cannot build (`NOT_BUILT`: the database servers and Docker) are not held to a live value. The vocabulary is `string`, `number`, `bool`, `()` (nothing), `LuaValue` (opaque, `any`), a trailing `?` for optional, `Result<T, string>` for `(value, err)`, `(A, B)` for several returns (a `...` in it, or alone, for any number: `Task:await`), a trailing `("...", "LuaValue")` for variadic arguments, and a type's own name for a resource type.

Adding a module: see [CONTRIBUTING.md](CONTRIBUTING.md#adding-a-new-module).

## Embedding the VM

`neoc run` is one script per engine, and uses none of this: every hook is a no-op by default (`make_engine()`, no `CallEntry`), and it behaves as it did before the hooks existed (`tests/lifetimes.rs` and `tests/signals.rs` pin the parts that are easy to break: what a script leaves running for the next file, a worker's tasks, the signals). A program that embeds `neoc-vm` and runs several scripts (one engine each) gets five generic mechanisms. They carry no policy of their own: what a script may do, and which modules it is offered, stay with the embedder.

- **Lifetimes (`lifecycle::Life`).** The per-engine registry of what a script starts: `std:task` tasks, `std:thread` workers, a `vnd:hyper` server's accept loop and connections, a `std:net` listener's socket. `Life::shutdown` aborts all of it, releases the sockets before it returns (a rebind of the same address succeeds at once), makes a `vnd:hyper` server report itself closed, and refuses anything new (a task started afterwards is cancelled without running); `Life::stop_background` does the same but stays open, and a `lib:http` `serve_static` the script is blocked in ends too (it is a call, not a task: `Life::stopped`). Aborting is cooperative: a worker in `while true do end` keeps its VM and a runtime thread. Build the engine with `sandbox::make_engine_in(life)`, or `make_bare_engine` + `install_runtime_state(&lua, life)`. Without an embedder, a script's `serve_static` also ends at Ctrl-C, which it listens for itself: an embedder that does not want that process-wide handler does not offer `lib:http`.
- **A turn at a time (`lifecycle::CallEntry`, `Baton`).** When the embedder serialises the calls into one VM, callbacks nobody called (an HTTP request to the script's own server, the next slice of a task) take their turn through `set_entry`, and a call lends its turn while it waits for something that does not use the VM. Every async host function is made by `lifecycle::async_function` / `async_method` with a `Role` (clippy's `disallowed-methods` refuses `create_async_function`, `add_async_method` and the other raw async constructors everywhere else; see `clippy.toml` and CONTRIBUTING for choosing the role). `lifecycle::idle` and `hold` are public for a `Role::Mixed` function of the embedder's own. The rules, and which boundaries refuse a yield on LuaJIT, are in the `lifecycle.rs` module doc; `tests/embedding.rs` holds each boundary it lists to a real engine.
- **A script environment (`script_env`).** `new_env` builds a table to load a chunk into (`Chunk::set_environment`): an allow-list of base functions, private copies of the libraries, a private `math.random`, and coroutine functions that let a host call suspend inside a coroutine of the script's own and refuse to resume one the script did not make. `freeze` seals it: read-only library proxies, a guarded `rawset`, and `table.insert`/`remove`/`sort`/`move` that refuse a frozen table. `make_bare_engine` locks the one shared string metatable. Add the embedder's own additions between the two.
- **The module registry (`modules::names`, `build`, `unknown_module`, `install_allowed`).** Build a module in an engine on request to give the script its own `require` over any subset of the modules. A `std:thread` worker is built with the subset its parent was offered through `install_allowed`; an embedder with its own `require` on a `make_bare_engine` engine says how with `sandbox::set_worker_engine`, and until it does `thread.spawn` refuses to start a worker (fail closed: a worker is never offered more than its parent). `make_engine` workers get the full set, as in `neoc run`. `lib::log::set_log_sink` takes `lib:log` lines instead of stderr (a worker logs to the same sink).
- **Calling script code from Rust.** Use `sandbox::call_guarded`, not `Function::call`, for a callback a script gave a host function: on LuaJIT an error value whose `__tostring` makes a protected call and then fails sends mlua's message handler into a recursion that overflows a runtime worker's stack and aborts the process. The engine's `xpcall` is guarded the same way against the same recursion (the handler runs at most 24 times per coroutine; see `guarded_xpcall.lua`; a successful `xpcall` costs about 5 times the builtin's, +32 ns a call on a debug build).
  **Known limit (NEOC-120):** an error value whose `__tostring` makes a protected call and then fails still aborts the process when the error reaches mlua at `exec_async` (the main chunk), a failing `std:task` task, or an embedder's `call_async`. Only the host calls that go through `call_guarded` and the `xpcall` handler are closed. An embedder that runs scripts it does not trust must not rely on a script not being able to abort the host until NEOC-120 lands.

`neoc_vm::mlua` is re-exported: name that one binding.

## Tests

- **Rust:** `#[cfg(test)] mod tests` next to the code. Integration tests in `tests/` only where they cross crate boundaries, plus `crates/neoc-vm/tests/embedding.rs`, `lifetimes.rs` and `signals.rs` (the embedding hooks, and what `neoc run` keeps doing), `crates/neoc-vm/tests/luals.rs` (the generated definitions and the lua-language-server fixtures), `crates/neoc-vm/tests/hostile_inputs.rs` (every module called with hostile values, fails on any panic) and `forged_len.rs`.
- **Lua:** a new module gets `crates/neoc/tests/<ns>/<name>.test.lua`, run with `neoc run`. (Not every existing module has one yet: `std:net`, `std:workers` and `lib:test` have none, and `std:task` uses a `.spec.lua`.) Per exported function: one happy path, one test per distinct error condition, and an empty or zero value where it applies. The outer `describe` is the bare module name.
- **CLI spec:** `tooling/cli/*.feature`, driven by `crates/neoc/tests/cucumber.rs`.
- **Benches:** three tiers per module (`library`, `into_out_lua`, `through_lua`), behind the `bench` feature.

## Rust style

- Edition 2024 for the neoc crates (the AgentOS crates set edition 2021 themselves). The workspace lint table (`unsafe_code = forbid`, clippy pedantic, `unwrap`/`expect`/`panic` denied) is inherited by the AgentOS crates only; the neoc crates do not inherit it yet (see their `[lints.cargo]` comment: adopting it means ~700 pedantic findings and six `unsafe` blocks). `neoc-vm` forbids `unsafe` outside tests (`lib.rs`); the `neoc` binary crate still has some, in its CLI test helper.
- `clippy::all` + `clippy::pedantic`; warnings are errors (`RUSTFLAGS="-D warnings"`). Use `#[allow(...)]` with a comment when you must.
- `thiserror` for library error enums; the CLI renders them with `miette`. No `unwrap()` / `expect()` in library code.
- `///` on public items is full, normal English. Everywhere else, write tight: cut filler, keep every fact. Never abbreviate code, API names, types, commands, paths or error text.
- Plain voice. No marketing language.
- `mlua` is built `vendored` (bundles LuaJIT) and `rusqlite` with `bundled`. The binary supports Linux, macOS and Windows; gate platform code with `#[cfg(unix)]` / `#[cfg(not(unix))]`.

## Workflow

`main` is the only long-lived branch. Work is tracked in [Jira](https://flying-dice.atlassian.net/jira/software/projects/NEOC) (project `NEOC`); code and pull requests are on GitHub at `flying-dice/neoc`.

1. Branch from `main`: `issues/{key}-{slug}` (e.g. `issues/NEOC-6-std-task`).
2. Run the [pre-PR checks](CONTRIBUTING.md#pre-pr-checks) locally. GitHub Actions is not a gate and is not relied on: the local run is the verification.
3. Open a PR into `main` (a draft is fine). Title and commits are Conventional Commits (the squash commit message comes from the PR title for a multi-commit PR, from the commit for a single-commit one). Reference the Jira key in the body or footer (`Refs NEOC-6`).
4. Self-review with the `pr-self-review` skill and post its `Self-review` note on the PR for the head SHA.
5. Hand off to an independent `pr-review` pass: a fresh reviewer with no session context, who posts a verdict note for the head SHA (`READY TO MERGE` or `CHANGES NEEDED`). Fix and repeat from step 2 until `READY TO MERGE`.
6. Squash-merge only on `READY TO MERGE` for the current head, then delete the branch. Mark the Jira ticket Done.

Releases are cut by pushing a `vX.Y.Z` tag on `main` (see CONTRIBUTING.md). Don't push release tags unless asked.

### Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`. Imperative, lowercase, no trailing period; one logical change per commit.

- **Type:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- **Scope** (optional): `cli`, a module (`std:fs`), or a subsystem (`vm`, `install`, `workspace`, `ci`).
- **Footer:** `Refs NEOC-42`. Never `Closes` / `Fixes` / `Resolves`.
- **Breaking change:** `!` after the type/scope **and** a `BREAKING CHANGE:` footer. The Lua surface of a module and the CLI are the user-visible contracts, so changes there almost always need `!`.

```
fix(std:fs): read_to_string reports the path in its error

Previously the error named only the OS cause. Now it prefixes the path
the caller passed.

Refs NEOC-42
```
