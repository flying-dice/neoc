# CLAUDE.md

Guidance for Claude (and any LLM agent) working in **neoc**: one Cargo workspace holding a Lua runtime and, next to it, AgentOS (see "AgentOS" below).

- **Runtime** (`crates/neoc-vm`): an embedded Luau engine (through `mlua`) backed by Rust. Scripts are written in Luau (Lua 5.1 syntax plus type annotations, `continue`, compound assignment and string interpolation, all of which run); their editor types are the generated Luau declarations of every module (see Modules, Types); the base library is Luau's (`bit32`, `table.clone`, no `goto`, `string.format('%s')` takes strings and numbers only (`%*` for anything else): the list is in [README.md](README.md#language)). Scripts run in a VM with a curated module surface (`std:*`, `lib:*`, `vnd:*`). The unsafe halves of the Lua standard library are stripped; everything else comes through explicit modules. Not a security boundary.
- **Binary** (`crates/neoc`): the thin `neoc` CLI over the runtime. `run` only.

neoc used to be a language that compiled to Lua. That is gone; the last state is under the `neoc-lang-final` tag. Restore a path with `git checkout neoc-lang-final -- <path>`.

Read [README.md](README.md) for what neoc is and [CONTRIBUTING.md](CONTRIBUTING.md) for the build, the pre-PR checks, how to add a module and how to release.

## AgentOS

This repository also holds **AgentOS**, an iPadOS-style virtual desktop for agents (to be renamed vos): the host (`crates/vos-*`, the `vos` binary), the apps (`apps/`, plain Lua 5.1 run on `neoc-vm`), the client (`client/`) and the UI kit (`packages/ui`). It has its own gate, boundaries, test layout and skills, all governed by [AGENTS.md](AGENTS.md), which stays as it is: read it before touching any of that. Its skills are in `.claude/skills/`: the `vos-*` ones (`agentos-pre-commit`, `agentos-clean-code-review`, `agentos-refactor`, `agentos-code-review`) and the AgentOS-only `app-design`, `coding-styleguide`, `prebuild-synthetic-ux`, `postbuild-synthetic-usability`, `verify-change`; the unprefixed `pre-commit`, `clean-code-review` and `refactor` (and `change-review`, `pr-review`, `pr-self-review`, ...) are neoc's. `AGENTS.md` is imported below, so its rules load with this file; where it and this file differ (Jira project, merge policy, review skills), NEOC-115 settles it, and for the neoc crates this file wins.

@AGENTS.md

The AgentOS adapter (`crates/vos-lua`) is an embedder of `neoc-vm`: AgentOS policy (apps, `readOnly`, views, `app.*`, the `vos:*` modules) stays in that adapter and never moves into `neoc-vm`. The AgentOS crates keep edition 2021 and inherit the workspace lint table; the neoc crates do not yet (see "Rust style").

## Layout

```
crates/neoc/                 # the `neoc` binary (the AgentOS crates add the `vos` host binary, crate `vos-host`, and the `vos-codegen` tool)
  src/main.rs                # CLI entry: `run`, embedded-payload executables
  tests/                     # Lua suites (*.test.lua, *.spec.lua), cli.rs, cucumber.rs
crates/neoc-vm/              # the runtime library
  tests/script_parse.rs      # compiles (never runs) every Lua script, Docker or not
  src/errors.rs              # runtime error types (NeocError)
  src/lua/sandbox.rs         # engine construction, stripped globals
  src/lua/lifecycle.rs       # the embedding hooks: Life, CallEntry/Baton, async_function/async_method, Role
  src/lua/script_env.rs      # a sealed environment for one script (+ async_coroutines.lua)
  src/lua/facade.rs          # Module builder: registration, spans, (value, error), the recording of each module's surface
  src/lua/facade/luau_decls.rs  # the Luau declarations emitter: a module's recorded surface -> `type` aliases, the `require` overloads
  src/lua/modules.rs         # module registration + `require`
  src/lua/{std,lib,vnd}/     # one file per module
  examples/luau_decls.rs     # writes (or `-- --check`s) the Luau declarations in types/neoc.d.luau
  tests/embedding.rs         # the embedding hooks on Luau: yield boundaries, environments, ownership
  tests/abort_regressions.rs # poisoned error values and recursion through host functions, each in a child process, on `VM_THREAD_STACK` (8 MiB) in a debug build and 2 MiB in a release build
  tests/lifetimes.rs         # what a script leaves running (`neoc run` and under a Life), worker module sets
  tests/signals.rs           # Ctrl-C and the process shutdown signal against serve_static and server:wait
  tests/luau_decls.rs        # drift check of types/neoc.d.luau, no colon in a file name, the luau-lsp fixtures (NEOC_REQUIRE_LUAU_LSP)
  benches/                   # Criterion suites (lib/, vnd/), behind the `bench` feature
types/neoc.d.luau            # GENERATED Luau declarations of every module (one definitions file for luau-lsp, `require` overloads at its end)
types/fixtures/              # luau-lsp fixtures: usage.luau, syntax.luau (clean, strict) and negative/ (expect-error)
(types/fixtures/.luaurc)     # Luau config of the fixtures: strict mode (each fixture also says `--!strict`)
.luarc.json                  # AgentOS's lua-language-server config for the apps (Lua 5.1, library types/generated/luals): neoc itself has none
tooling/cli/                 # Gherkin spec for the CLI, run against the built binary
```

The crate boundary is enforced by `Cargo.toml`. `neoc-vm` owns the `mlua`/`tokio` stack, the sandbox and every module, and depends on no other workspace crate. `neoc` owns the CLI, glob expansion, the tokio runtime and process exit policy. If a need crosses the boundary, lift it into `neoc-vm`'s API; don't shortcut.

## Build and test

```sh
cargo build                                          # debug build
cargo test -p neoc -p neoc-vm                         # Rust, CLI integration and the Gherkin spec
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.lua' 'crates/neoc/tests/**/*.spec.lua'   # all Lua suites (Docker ones need a daemon)
cargo bench -p neoc-vm --features bench --bench lib --bench vnd -- --test   # bench smoke
cargo clippy --workspace --all-targets               # lint
cargo fmt --check                                    # formatting
```

`--workspace` would also build and test the AgentOS crates, whose tests need `bun` and `darklua` on `PATH`; the neoc gate names `-p neoc -p neoc-vm`, and the AgentOS crates have the gate in `AGENTS.md`. The full pre-PR gate, including the Lua suite set that needs no Docker, is in [CONTRIBUTING.md](CONTRIBUTING.md#pre-pr-checks).

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
- **Types.** Every module's `declare` records its surface (functions, parameter and return types, docs, resource types with their methods, fields and constructors) and that record is the source of the editor types, the Luau declarations in `types/neoc.d.luau` (never edited by hand): `cargo run -p neoc-vm --example luau_decls` regenerates, `-- --check` fails on drift. The file is one luau-lsp definitions file: per module an `export type lib_json = { ... }` table type of its functions, constants and the `Statics` of its types (and `lib_json_Object` for each resource type, a table type of its methods with `self` first and its fields), then one overloaded `declare require: ((path: "lib:json") -> lib_json) & ...` that gives `require("lib:json")` that module's type and makes an unknown name a type error. Aliases, not `declare class` (luau-lsp 1.70 does not parse it), and `export type` so scripts can name them (`local o: lib_json_Object`). No file is named after a module key (a colon is not a Windows file name). Docs recorded with each member are `---` comments luau-lsp shows on hover. Keep the record true to the runtime; the drift tests in `modules.rs` check, in both directions, that the live module table and the recorded functions and fields agree and that a live value of each resource type and its recorded members agree (every recorded member exists; for a plain table or a userdata whose methods sit in a table, every live method is recorded; a userdata with fields is checked one way only), and that every named type has an alias (the text of the declarations is scanned: any other name is a failure). They do not check parameter or return types against the runtime, and the types they cannot build (`NOT_BUILT`: the database servers and Docker) are not held to a live value. The vocabulary is `string`, `number`, `bool`, `()` (nothing), `LuaValue` (opaque, `any`), a trailing `?` for optional, `Result<T, string>` for `(value, err)` (declared `(T?, string?)`), `(A, B)` for several returns (a `...` in it, or alone, for any number: `Task:await`), a trailing `("...", "LuaValue")` for variadic arguments (`...any`), a type's own name for a resource type, `std_net.TcpListener` for a resource type of another module, and Luau's own syntax for a function (`(req: vnd_hyper.Request) -> ...(string | { status: number? } | nil)`) a table (`{ [string]: any }`, `{ status: number? }`) or a string literal (`"GET" | "POST"`), each with its names mapped word by word (`LuaValue` is `any`, a bare name is the module's own type) except a label (a parameter or table key; a reserved word is renamed `end_` as a parameter and quoted `["end"]` as a key) and a string literal (copied as it is); `...T` is allowed in a function type (`...LuaValue` is `...any`). A function value in a `Result` stays an optional function, not a function with an optional return. The emitter refuses what it can see it cannot write as Luau (a control character in a doc or a type, a tuple or `...` inside `Result<>`, `...` not last, a malformed string escape, a name that is not an identifier, two records of one name) with an error of `Module::luau_decls`, and the always-run test parser (it also refuses a trailing comma in a group, an optional return pack, a repeated property) `src/lua/facade/luau_decls/grammar.rs` parses the whole generated file (`every_module_yields_valid_declarations`). The emitter is `src/lua/facade/luau_decls.rs`; adding a module is the metadata in its `declare` plus `cargo run -p neoc-vm --example luau_decls` (see CONTRIBUTING).

Adding a module: see [CONTRIBUTING.md](CONTRIBUTING.md#adding-a-new-module).

## Embedding the VM

`neoc run` is one script per engine, and uses none of this: every hook is a no-op by default (`make_engine()`, no `CallEntry`), and it behaves as it did before the hooks existed (`tests/lifetimes.rs` and `tests/signals.rs` pin the parts that are easy to break: what a script leaves running for the next file, a worker's tasks, the signals). A program that embeds `neoc-vm` and runs several scripts (one engine each) gets five generic mechanisms. They carry no policy of their own: what a script may do, and which modules it is offered, stay with the embedder.

- **Lifetimes (`lifecycle::Life`).** The per-engine registry of what a script starts: `std:task` tasks, `std:thread` workers, a `vnd:hyper` server's accept loop and connections, a `std:net` listener's socket. `Life::shutdown` aborts all of it, releases the sockets before it returns (a rebind of the same address succeeds at once), makes a `vnd:hyper` server report itself closed, and refuses anything new (a task started afterwards is cancelled without running); `Life::stop_background` does the same but stays open, and a `lib:http` `serve_static` the script is blocked in ends too (it is a call, not a task: `Life::stopped`). Aborting is cooperative: a worker in `while true do end` keeps its VM and a runtime thread. Build the engine with `sandbox::make_engine_in(life)`, or `make_bare_engine` + `install_runtime_state(&lua, life)`. Without an embedder, a script's `serve_static` also ends at Ctrl-C, which it listens for itself: an embedder that does not want that process-wide handler does not offer `lib:http`.
- **A turn at a time (`lifecycle::CallEntry`, `Baton`).** When the embedder serialises the calls into one VM, callbacks nobody called (an HTTP request to the script's own server, the next slice of a task) take their turn through `set_entry`, and a call lends its turn while it waits for something that does not use the VM. Every async host function is made by `lifecycle::async_function` / `async_method` with a `Role` (clippy's `disallowed-methods` refuses `create_async_function`, `add_async_method` and the other raw async constructors everywhere else; see `clippy.toml` and CONTRIBUTING for choosing the role). `lifecycle::idle` and `hold` are public for a `Role::Mixed` function of the embedder's own. The rules, and which boundaries refuse a yield on Luau, are in the `lifecycle.rs` module doc; `tests/embedding.rs` holds each boundary it lists to a real engine.
- **A script environment (`script_env`).** `new_env` builds a table to load a chunk into (`Chunk::set_environment`): an allow-list of base functions, private copies of the libraries, a private `math.random`, and coroutine functions that let a host call suspend inside a coroutine of the script's own and refuse to resume **or close** one the script did not make (`coroutine.close` is Luau's and kills a suspended thread; `neoc run`'s engine has none, because a task's thread closed from a script would end with its `await` returning nothing). `freeze` seals it: a library, and any host table without a metatable, gets Luau's `readonly` flag (which `rawset`, `table.insert`/`sort`/`move`/`clear` and assignment all respect); a host table **with** a metatable is replaced by a read-only proxy with a locked metatable, because the flag alone leaves `getmetatable(host).__index = ...` and a `__newindex` function open. `make_bare_engine` locks the one shared string metatable. Add the embedder's own additions between the two.
- **The module registry (`modules::names`, `build`, `unknown_module`, `install_allowed`).** Build a module in an engine on request to give the script its own `require` over any subset of the modules. A `std:thread` worker is built with the subset its parent was offered through `install_allowed`; an embedder with its own `require` on a `make_bare_engine` engine says how with `sandbox::set_worker_engine`, and until it does `thread.spawn` refuses to start a worker (fail closed: a worker is never offered more than its parent). `make_engine` workers get the full set, as in `neoc run`. `lib::log::set_log_sink` takes `lib:log` lines instead of stderr (a worker logs to the same sink).
- **Calling script code from Rust, and the thread stack.** Call a script function with `Function::call` (or `call_async` when it may suspend): on Luau an error value whose `__tostring` makes a protected call and then fails is answered `error in error handling` by the engine, once, so the error reaches the caller as an ordinary `mlua::Error`. (On LuaJIT it recursed until the stack was gone and aborted the process; main covered `xpcall` and two call sites with a guard layer, `guarded_xpcall.lua` and `sandbox::call_guarded`, removed with the switch to Luau, and left every other path aborting: the first 17 cases of `tests/abort_regressions.rs` abort on main in 14 of them.) Recursion through a host function that calls back into Lua is bounded by Luau's C-call limit (200), an ordinary error (`C stack overflow`, or `error in error handling` when the message handler fails at that depth): a script cannot recurse deeper than about 200 levels through `__index`, `__tostring`, a `table.sort` comparator or a `string.gsub` callback. **Every thread that polls a VM needs `neoc_vm::VM_THREAD_STACK` (8 MiB) of stack**: build the tokio runtime with `.thread_stack_size(VM_THREAD_STACK)` (workers and blocking pool), and run the top-level future on a worker (`tokio::spawn`), not on a `block_on` thread; `std:thread` workers are tasks on that runtime and need nothing more. The 200 levels have to fit with a Rust host function in each: measured in a debug build a level through `Mutex:with_lock` needs up to 4.5 MiB, `lib:test` `describe` 4 MiB, `lib:cache` `on_evict` 3 MiB, a `vnd:hyper` request options table 1.5 MiB; a release build needs under 1 MiB for every path (tokio's default 2 MiB holds it). `neoc run` builds its runtime that way; `lifecycle.rs` has the table. An async host function must read a script table before it builds its future (as `vnd:hyper` and `std:thread` do), not inside it. `tests/abort_regressions.rs` runs each case in a child process on `VM_THREAD_STACK` in a debug build and on 2 MiB in a release build (`cargo test --release -p neoc-vm --test abort_regressions`).

`neoc_vm::mlua` is re-exported: name that one binding.

## Tests

- **Rust:** `#[cfg(test)] mod tests` next to the code. Integration tests in `tests/` only where they cross crate boundaries, plus `crates/neoc-vm/tests/embedding.rs`, `lifetimes.rs` and `signals.rs` (the embedding hooks, and what `neoc run` keeps doing), `crates/neoc-vm/tests/abort_regressions.rs` (error values and recursion that aborted the host on LuaJIT), `crates/neoc-vm/tests/luau_decls.rs` (the generated declarations and the luau-lsp fixtures), `crates/neoc-vm/tests/hostile_inputs.rs` (every module called with hostile values, fails on any panic) and `forged_len.rs`.
- **Lua:** a new module gets `crates/neoc/tests/<ns>/<name>.test.lua`, run with `neoc run`. (Not every existing module has one yet: `std:net`, `std:workers` and `lib:test` have none, and `std:task` uses a `.spec.lua`.) Per exported function: one happy path, one test per distinct error condition, and an empty or zero value where it applies. The outer `describe` is the bare module name.
- **CLI spec:** `tooling/cli/*.feature`, driven by `crates/neoc/tests/cucumber.rs`.
- **Benches:** three tiers per module (`library`, `into_out_lua`, `through_lua`), behind the `bench` feature.

## Rust style

- Edition 2024 for the neoc crates (the AgentOS crates set edition 2021 themselves). The workspace lint table (`unsafe_code = forbid`, clippy pedantic, `unwrap`/`expect`/`panic` denied) is inherited by the AgentOS crates only; the neoc crates do not inherit it yet (see their `[lints.cargo]` comment: the ~700 pedantic findings and six `unsafe` blocks it mentions were counted when the table was imported, against neoc's table of that time, which did not deny `unwrap`/`expect`/`panic`; this one is stricter, so the count would be higher). `neoc-vm` forbids `unsafe` outside tests (`lib.rs`); the `neoc` binary crate still has some, in its CLI test helper.
- `clippy::all` + `clippy::pedantic`; warnings are errors (`RUSTFLAGS="-D warnings"`). Use `#[allow(...)]` with a comment when you must.
- `thiserror` for library error enums; the CLI renders them with `miette`. No `unwrap()` / `expect()` in library code.
- `///` on public items is full, normal English. Everywhere else, write tight: cut filler, keep every fact. Never abbreviate code, API names, types, commands, paths or error text.
- Plain voice. No marketing language.
- `mlua` is built with `luau` (the engine's source is vendored by `luau0-src`; native code generation, `luau-jit`, is not enabled) and `rusqlite` with `bundled`. The binary supports Linux, macOS and Windows; gate platform code with `#[cfg(unix)]` / `#[cfg(not(unix))]`.

## Workflow

`main` is the only long-lived branch. Work is tracked in [Jira](https://flying-dice.atlassian.net/jira/software/projects/NEOC) (project `NEOC`); code and pull requests are on GitHub at `flying-dice/neoc`.

1. Branch from `main`: `issues/{key}-{slug}` (e.g. `issues/NEOC-6-std-task`).
2. Run the [pre-PR checks](CONTRIBUTING.md#pre-pr-checks) locally. GitHub Actions is not a gate and is not relied on: the local run is the verification.
3. Open a PR into `main` (a draft is fine). Title and commits are Conventional Commits (the squash commit message comes from the PR title for a multi-commit PR, from the commit for a single-commit one). Reference the Jira key in the body or footer (`Refs NEOC-6`).
4. Self-review with the `pr-self-review` skill and post its `Self-review` note on the PR for the head SHA.
5. Hand off to an independent `pr-review` pass: a fresh reviewer with no session context, who posts a verdict note for the head SHA (`READY TO MERGE` or `CHANGES NEEDED`). Fix and repeat from step 2 until `READY TO MERGE`.
6. Squash-merge only on `READY TO MERGE` for the current head, then delete the branch. The one exception is the merge of AgentOS (NEOC-113, PR #34), which lands with a merge commit so that AgentOS's history stays in `main`: never squash it. Mark the Jira ticket Done.

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
