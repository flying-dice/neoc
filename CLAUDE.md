# CLAUDE.md

Instructions for Claude (and any LLM agent) working in this repository. It is one Cargo workspace and one Bun workspace holding
two products, and one set of rules for both:

- **neoc**: a fast, lightweight [Luau](https://luau.org/) runtime. `crates/neoc-vm` is the embedded engine (through `mlua`):
  Luau scripts in a VM with a curated module surface (`std:*`, `lib:*`, `vnd:*`); the unsafe halves of the Lua standard
  library are stripped and everything else comes through explicit modules. Not a security boundary. `crates/neoc` is the
  thin `neoc` CLI over it (`run` only). Scripts are Luau (Lua 5.1 syntax plus type annotations, `continue`, compound
  assignment and string interpolation); their editor types are the generated Luau declarations of every module
  (`types/neoc.d.luau`, checked by luau-lsp); the base library is Luau's (`bit32`, `table.clone`, no `goto`,
  `string.format('%s')` takes strings and numbers only: the list is in [README.md](README.md#language)). neoc used to be a
  language that compiled to Lua; that is gone, its last state is the `neoc-lang-final` tag (`git checkout neoc-lang-final --
  <path>` restores a path).
- **vos** (Virtual OS): an iPadOS-style virtual desktop for agents, a self-hosted home lab: apps can provide LAN services such
  as DNS and container-backed services, so vos can stand in for the services people run on a home NAS. Three parts:
  - **The host** (`crates/vos-*`, the `vos` binary): a Rust hexagon. It runs the apps, owns their data, serves the client, and
    speaks JSON-RPC 2.0 over WebSocket at `/rpc`, AG-UI over HTTP + SSE at `/agui` (agent chats), MCP at `/mcp` (the tools
    agents call) and ACP to the agent processes it launches. It binds the LAN and every client must pair as a device first.
  - **The apps** (`apps/<id>/`): server-side code authored in typed Luau (`app.json` + `*.luau`, `--!strict`, checked by
    luau-lsp against `types/generated/vos.d.luau`). `bun run build:apps` bundles an app into one `app.luau` with darklua (types
    and comments stripped); the host runs it in one `neoc-vm` VM per app (the `vos-lua` adapter), loaded from an app ref over
    `file://`, `http://` or `https://`. An app holds its state, data, tools, notifications and UI description. Examples:
    `apps/calculator`, `apps/agents`; `apps/README.md` is the reference for writing one.
  - **The client** (`client/`, a Vite + Svelte 5 PWA): renders the view trees the host sends with the kit in `packages/ui`
    (atomic layers) through adapters in `client/src/view/`, holds client-managed state, and runs **no app code**.

[README.md](README.md) says what the products are and how to build, run and test them; [CONTRIBUTING.md](CONTRIBUTING.md) has
the pre-PR checks, how to add a module and how to release. This file is the rest: the rules.

## Layout

```
crates/neoc/                 # the `neoc` binary (CLI entry src/main.rs: `run`, embedded-payload executables)
  tests/                     # Lua suites (*.test.lua, *.spec.lua), cli.rs, cucumber.rs
crates/neoc-vm/              # the runtime library
  src/errors.rs              # runtime error types (NeocError)
  src/lua/sandbox.rs         # engine construction, stripped globals
  src/lua/lifecycle.rs       # the embedding hooks: Life, CallEntry/Baton, async_function/async_method, Role
  src/lua/script_env.rs      # a sealed environment for one script (+ async_coroutines.lua)
  src/lua/facade.rs          # Module builder: registration, spans, (value, error), the recording of each module's surface
  src/lua/facade/luau_decls.rs  # the Luau declarations emitter: a module's recorded surface -> `type` aliases, the `require` overloads
  src/lua/modules.rs         # module registration + `require`
  src/lua/{std,lib,vnd}/     # one file per module
  examples/luau_decls.rs     # writes (or `-- --check`s) types/neoc.d.luau
  tests/                     # e.g. embedding, abort_regressions, lifetimes, signals, luau_decls, hostile_inputs, forged_len, script_parse
  benches/                   # Criterion suites (lib/, vnd/), behind the `bench` feature
crates/vos-domain/           # vos: the pure model (ids, app refs, tools, the view tree and component schema, protocol messages)
crates/vos-application/      # vos: ports (traits) and use cases, in-memory fakes in `testing`
crates/vos-{lua,sqlite,fetch,acp,agui,http,mcp}/   # vos: adapters, one port or protocol each (`lua` embeds neoc-vm)
crates/vos-codegen/          # vos: dev-time generator, Rust schema -> types/generated/*
crates/vos-host/             # vos: composition root and the `vos` binary
apps/<id>/                   # vos apps (app.json, *.luau, JOURNEYS.md, e2e/); apps/README.md
client/                      # the shell and the managed renderer (client/src/view); client/journeys/*.md for built-in pages
packages/ui/, packages/storybook/   # the UI kit (atoms, molecules, organisms, templates, tokens, stories); the shared Storybook config
e2e/                         # Playwright: stories, desktop, journeys against the real host; e2e/tools (session.mjs, contact-sheet.mjs)
scripts/                     # lint and build scripts (check-*.ts, build-apps.ts, lint-luau.ts, ...), pre-pr.ts (the gate), check-neoc.sh
types/neoc.d.luau            # GENERATED by neoc-vm: Luau declarations of every module (one definitions file, `require` overloads last)
types/fixtures/              # luau-lsp fixtures of those declarations: usage.luau, syntax.luau (clean, strict) and negative/ (expect-error)
types/generated/             # GENERATED by vos-codegen and scripts/codegen-lua.ts: vos.d.luau (the app-facing declarations: vos's
                             #   `app`/`ui` API plus neoc.d.luau's modules minus NOT_FOR_APPS), vos.ts, ui.lua, prelude.lua
lua/prelude.lua              # the prelude embedded in every app VM (polyfills: `typeof`, `utf8`)
tooling/cli/                 # Gherkin spec for the CLI, run against the built binary
ux/<app>/                    # UX research output written by the UX skills (discovery.md, review-<n>.md, round-<n>.md)
.claude/                     # skills, agents, the ux-research workflow, output style; they are the process docs of this repo
```

Never edit `types/neoc.d.luau` or `types/generated/*` by hand: `cargo run -p neoc-vm --example luau_decls` and `bun run codegen`
write them (`bun run codegen:check` fails when they are stale). There are no other long-form docs in the repo: the code, its
tests, the generated types, the READMEs, this file and the skills are the documentation. (The old `docs/` tree, a pre-NEOC-115
copy, is in git history at `51f47f4`: `git show 51f47f4:docs/architecture/ui-system.md`.)

## Boundaries

- **Crate dependency direction** (inward only). vos is a hexagon: `vos-domain` is pure and depends on no other crate;
  `vos-application` (ports and use cases) depends on domain; each adapter (`lua`, `sqlite`, `mcp`, `acp`, `agui`, `fetch`,
  `http`, `codegen`) depends on application and domain and **never on another adapter**; `vos-host` is the composition root and
  the only crate that sees them all. `neoc-vm` is a support library, not a vos adapter: the Lua VM (mlua on Luau, built from
  source), depending on no other crate here; the `neoc` CLI is its other client. **The no-vos-leak rule: `neoc` and `neoc-vm`
  never depend on a `vos-*` crate and take no vos concept** (apps, `readOnly`, views, `app.*`, the `vos:*` modules, a type or
  name only vos needs). vos policy lives in `vos-lua`, the embedder; `neoc-vm` gets only a generic mechanism an embedder needs (a
  hook, a registry, `Life`). Among the `vos-*` crates only `vos-lua` may depend on `neoc-vm` (it reaches mlua through
  neoc-vm's re-export, so mlua stays out of every vos crate). Enforced by `bun run lint:deps` (`scripts/check-deps.ts`, an
  allow-list over `cargo metadata`), `cargo deny check` (`deny.toml`: each framework crate is pinned to its adapter) and, for
  the no-vos-leak edge, `scripts/check-neoc.sh`. A new crate is placed in the allow-list on purpose.
- **Rust hygiene.** The workspace lint table (`[workspace.lints]`: clippy pedantic with `-D warnings`, `unsafe_code` forbidden,
  `unwrap`, `expect` and `panic!` denied outside tests) is inherited by the vos crates; the neoc crates do not inherit it yet
  (see "Rust style") but follow the same practice. `thiserror` in library crates, `anyhow` only in a host binary.
- **Apps are the person's own code on their own host. The VM keeps their Luau state apart; it is not a security boundary.** An
  app runs as Luau in its own VM and its own environment table: its globals, its standard libraries (read-only) and its module
  tables are its own, so apps do not share state by accident, and it has no `load*`/`getfenv`/`setfenv`. That is all the
  separation there is. Through `std:fs`, `vnd:rusqlite` and the other modules an app can read and write other apps' data and the
  host's own database under `~/.vos`. There are **no permissions** and **no budgets** (no CPU, memory or time limits): a runaway
  app (a loop that never yields, a `std:thread` worker) can hang the host, not just itself, and can outlive its uninstall,
  because unloading aborts work only at the points where the app yields to the host. That is accepted. It reaches the host
  through the `app` table (typed by `types/generated/vos.d.luau`) and through **every `std:`, `lib:` and `vnd:` module of
  `neoc-vm` except `lib:test`, `lib:compile` and `std:workers`** (files, network, servers, tasks, threads, SQLite, TLS), next to
  `vos:app` and `vos:ui`. All of them are typed in the declarations; the refused three are `vos_domain::NOT_FOR_APPS`, the one
  list: the adapter refuses them and the declarations type each as unusable. What an app starts (tasks, servers, workers)
  belongs to it: unloading or replacing the app aborts it and closes its listeners, and what a task or a request handler runs
  queues behind the app's other calls (one call at a time per VM). Still enforced: a tool declared `readOnly` cannot write
  through the `app.*` API (db, kv, state, notify, shell, windows, and registering tools, windows and handlers), and neither can
  the tasks it starts or the requests it triggers through the app's own server (a request is read-only while any read-only call
  is acting: running, queued, or waiting at something in flight such as a request; one asleep at a timer does not count, so a
  poller that also makes requests still taints webhooks intermittently: put long-lived work that must write in a writable
  call, not a `readOnly` tool; `crates/vos-lua/tests/hardening.rs` and `chaos.rs` pin it, and the failed-reload edge is
  accepted in the `AppRuntime::load` port doc). That guarantee does **not** cover the neoc modules (`std:fs`, `vnd:rusqlite`,
  `std:net`, ...), which are open to every tool. Agent auto-approval of `readOnly` app tools relies on this `app.*`-only
  guarantee (owner decision: auto-approval stays on). SQL `ATTACH` is denied through `app.db`/`app.kv` (not through
  `vnd:rusqlite`), a cyclic or absurdly shared table handed back to the host is an error (never a stack overflow), a Lua number
  can not panic a module or size an unbounded allocation (an error instead: a host panic aborts every app), and chunks load as
  text only.
- **Luau, the stack and what an error value can do.** An error value whose `__tostring` makes a protected call and then fails
  (which aborted the host on LuaJIT) is answered `error in error handling` once, so it reaches the adapter as an ordinary error
  on every path (`crates/vos-lua/tests/poison.rs` holds one test per path). Recursion through a Rust host function that calls
  back into Lua stops at Luau's C-call limit (200) with `C stack overflow`, provided the thread's stack holds those levels:
  **every thread that polls a VM has `neoc_vm::VM_THREAD_STACK` (8 MiB)**, which `vos_host::runtime` gives the workers and the
  blocking pool (the host re-exports it through `vos_lua`, since the host may not depend on neoc-vm), and the top-level future
  runs on a worker, not on the `block_on` thread (`crates/vos-host/tests/stack.rs`). A new thread of the host's own that calls
  into Lua is built with that stack too. (A Tera panic in `lib:template` aborts under `release-host`'s `panic = "abort"`:
  NEOC-125.)
- **The client runs no app code.** The renderer draws view trees and sends events; nothing executable crosses the wire. Kit
  knowledge of apps, and app knowledge in the client, are both bugs.
- **Agent and tool output is untrusted:** render it as text. No `{@html}` or `innerHTML` for it (the only `{@html}` is
  `packages/ui/src/atoms/Icon.svelte`, over the constant icon map). RPC params, MCP tool inputs, and anything an app or agent
  sends are validated at the edge.
- **Missing a capability?** Extend the host API: the domain type and schema, the application port and use case, the adapter, the
  codegen output and the tests, in that order. Never a side channel (no extra route, raw socket or global); client and host talk
  only through the protocol (JSON-RPC on `/rpc`, AG-UI on `/agui`).
- **Release what you open.** The host registers and unregisters apps at runtime, so an adapter's resources (VMs, DB handles,
  watchers, agent processes) are dropped on unload; a neoc module registers whatever it starts with the engine's `Life`.

`bun run lint:boundaries` (`scripts/check-boundaries.ts`) enforces on the client and the kit: no relative import leaves its
package (the client never reaches into `crates/`), the client and kit import only `@vos/ui` (stories also `@vos/storybook`), no
Node or Bun builtins in browser code, `new WebSocket` only in `client/src/view/rpc/websocket.ts`, and no `{@html}`, `innerHTML`,
`outerHTML`, `insertAdjacentHTML` or `document.write`. Also atomic layers (`lint:atomic`, in `packages/ui` and `client/src`:
atoms, molecules, organisms, templates, pages, each importing only strictly lower layers through the layer entry points) and
test ids from the first commit (`lint:testids`).

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

- **neoc Rust:** `#[cfg(test)] mod tests` next to the code. Integration tests in `tests/` only where they cross crate boundaries, plus `crates/neoc-vm/tests/embedding.rs`, `lifetimes.rs` and `signals.rs` (the embedding hooks, and what `neoc run` keeps doing), `abort_regressions.rs` (error values and recursion that aborted the host on LuaJIT), `luau_decls.rs` (the generated declarations and the luau-lsp fixtures), `hostile_inputs.rs` (every module called with hostile values, fails on any panic) and `forged_len.rs`.
- **neoc Lua:** a new module gets `crates/neoc/tests/<ns>/<name>.test.lua`, run with `neoc run`. (Not every existing module has one yet: `std:net`, `std:workers` and `lib:test` have none, and `std:task` uses a `.spec.lua`.) Per exported function: one happy path, one test per distinct error condition, and an empty or zero value where it applies. The outer `describe` is the bare module name.
- **CLI spec:** `tooling/cli/*.feature`, driven by `crates/neoc/tests/cucumber.rs`.
- **Benches:** three tiers per module (`library`, `into_out_lua`, `through_lua`), behind the `bench` feature.
- **vos Rust** (`cargo nextest run --workspace --exclude neoc --exclude neoc-vm`): domain unit tests with no I/O; use cases in `vos-application` against the in-memory port fakes in `vos_application::testing` (no mocking library: write a fake); adapters through the real library in `crates/<adapter>/tests/` (the shipped apps too: `crates/vos-lua/tests/calculator_app.rs` builds `apps/calculator` with darklua and runs the bundle, which needs `bun` and `darklua`); the real wiring on an ephemeral loopback port in `crates/vos-host/tests` (`/rpc`, `/agui`, `/mcp`, pairing, the Agents app). Name a test after the behaviour, put the failure path next to the success path, keep each test independent (its own data directory, no fixed ports); a protocol or adapter change adds the test in the layer it touches and, across layers, one in the host. Tests may use `unwrap`, `expect` and `panic!` (`clippy.toml`).
- **vos client and kit:** `bun test` next to the code (`*.test.ts`: the renderer core against a TypeScript fake host on an in-memory wire, the shell's rune modules through the preload in `client/bunfig.toml`, the kit's logic, `types/tests`).
- **vos stories and e2e:** every screen and state of the shell, the renderer and the kit is a story (`*.stories.svelte`; renderer and shell states are JSON-tree stories on a fake host); `bun run test:e2e` runs every story (render, axe WCAG 2.1 AA, snapshots light and dark, no overflow), the `desktop` project (every `apps/*/app.json` and built-in page opened in the real shell on the real host: sizes, layouts, light and dark, axe, snapshots) and the `journeys` projects (`JOURNEYS.md` and `client/journeys/*.md`, one spec per journey id, driven by test ids). The `verify-change` skill (sections 3 and 7) says how the e2e host works and how to drive the real shell by hand.
- **Luau apps:** `bun run lint:luau` runs `luau-lsp analyze` in strict mode over every app and fixture against `types/generated/vos.d.luau` (a positive fixture, `negative/` fixtures whose `-- expect-error` markers are counted in `scripts/lint-luau.ts`, a second pass for an app's own modules); `bun run lint:stylua` checks the format.

Locators in tests are `getByTestId` only; the test-id conventions are in `.claude/skills/coding-styleguide/references/typescript-svelte.md`.

## Rust style

- Edition 2024 for the neoc crates (the vos crates set edition 2021 themselves). The workspace lint table (`unsafe_code = forbid`, clippy pedantic, `unwrap`/`expect`/`panic` denied) is inherited by the vos crates only; the neoc crates do not inherit it yet (see their `[lints.cargo]` comment: the ~700 pedantic findings and six `unsafe` blocks it mentions were counted when the table was imported, against neoc's table of that time, which did not deny `unwrap`/`expect`/`panic`; this one is stricter, so the count would be higher). `neoc-vm` forbids `unsafe` outside tests (`lib.rs`); the `neoc` binary crate still has some, in its CLI test helper.
- `clippy::all` + `clippy::pedantic`; warnings are errors (`RUSTFLAGS="-D warnings"`). Use `#[allow(...)]` with a comment when you must.
- `thiserror` for library error enums; the CLI renders them with `miette`. No `unwrap()` / `expect()` in library code.
- `///` on public items is full, normal English. Everywhere else, write tight: cut filler, keep every fact. Never abbreviate code, API names, types, commands, paths or error text.
- Plain voice. No marketing language.
- `mlua` is built with `luau` (the engine's source is vendored by `luau0-src`; native code generation, `luau-jit`, is not enabled) and `rusqlite` with `bundled`. The binary supports Linux, macOS and Windows; gate platform code with `#[cfg(unix)]` / `#[cfg(not(unix))]`.


## Workflow

`main` is the only long-lived branch, and everything lands on it through a squash-merged PR; nothing is pushed to `main`
directly. Code and pull requests are on GitHub at `flying-dice/neoc`.

**Tracking.** This repository's work is in the Jira project **NEOC**
(<https://flying-dice.atlassian.net/jira/software/projects/NEOC>): create new work there, not as GitHub issues. The **AGOS**
Jira project (board <https://flying-dice.atlassian.net/jira/software/projects/AGOS/boards/101>) and the **AGOS Confluence
space** (<https://flying-dice.atlassian.net/wiki/spaces/AGOS/overview>) still hold the vos product epics and the long-form
planning pages. The current AGOS epic is **AGOS-55**, foundations. (Older comments cite AGOS-n keys; they stay valid.)

1. Branch from `main`: `issues/NEOC-<n>-<slug>` (e.g. `issues/NEOC-6-std-task`).
2. Work, then run the pre-PR checks locally (see "The gate"). GitHub Actions is blocked and is not a gate: the local run is the
   verification.
3. Open a PR into `main` (a draft is fine). One module or one well-scoped change per PR; say why, not only what. Title and
   commits are Conventional Commits; reference the Jira key in the body or footer (`Refs NEOC-<n>`).
4. Self-review with the `pr-self-review` skill and post its `Self-review` note on the PR for the head SHA.
5. Hand off to an independent `pr-review` pass: a fresh reviewer with no session context, who posts a verdict note for the head
   SHA (`READY TO MERGE`, `CHANGES NEEDED` or `HOLD`). Fix and repeat from step 2 until `READY TO MERGE`.
6. Squash-merge only on `READY TO MERGE` for the current head, then delete the branch and mark the Jira ticket Done. The one
   exception was the merge of vos into neoc (NEOC-113, PR #34), which landed with a merge commit to keep vos's history.
7. An app that isn't ready for the full gate and the UX research panel stays in its PR: keep pushing to the branch; don't merge
   a partial app behind a flag or with "tests later".

Releases are cut by pushing a `vX.Y.Z` tag on `main` (see CONTRIBUTING.md). Don't push release tags unless asked.

### Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`. Imperative, lowercase, no trailing period; one logical change per commit.

- **Type:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- **Scope** (optional): `cli`, a module (`std:fs`), or a subsystem (`vm`, `vos`, `client`, `install`, `workspace`, `ci`).
- **Footer:** `Refs NEOC-42`. Never `Closes` / `Fixes` / `Resolves`.
- **Breaking change:** `!` after the type/scope **and** a `BREAKING CHANGE:` footer. The Lua surface of a module, the CLI, and vos's
  app API, protocol and `app.json` are the user-visible contracts, so changes there almost always need `!`.

```
fix(std:fs): read_to_string reports the path in its error

Previously the error named only the OS cause. Now it prefixes the path
the caller passed.

Refs NEOC-42
```

## The gate

One command set, defined once in [CONTRIBUTING.md](CONTRIBUTING.md#pre-pr-checks) and run by the `pre-commit` skill:

```sh
bun run gate           # the neoc checks (scripts/check-neoc.sh), then the vos checks: lint, typecheck, test, test:e2e
bun run gate:neoc      # = scripts/check-neoc.sh: the scoped gate for a change confined to the neoc crates (no bun, no darklua; not for a change to the embedding API or sandbox, to an async host function's `Role` or to what a module registers with `Life`: `vos-lua`'s `turns.rs` and `chaos.rs` pin those, so they get the full gate)
```

`bun run gate` sets `VOS_GATE=1` and `NEOC_REQUIRE_LUAU_LSP=1`, so a missing `luau-lsp`, `stylua` or `darklua` fails instead of
skipping. Both variables keep their meaning: `VOS_GATE=1` (or `CI`) makes the vos lints (`lint:luau`, `lint:stylua`) and the
`vos-codegen` editor test fail on a missing tool; `NEOC_REQUIRE_LUAU_LSP=1` does the same for `neoc-vm`'s `luau_decls` test.
Without them a developer machine skips with a notice, and a skip is not a pass. Needed tools: the pinned Rust toolchain,
`cargo-nextest`, `cargo-deny`, `luau-lsp` 1.70.1 (the release binary on `PATH` or in `~/.local/bin`; there is no Homebrew
formula), and for vos Bun, `darklua` 0.18.0 (`cargo install darklua --version 0.18.0 --locked`; building the apps fails without
it), `stylua` and Playwright's Chromium. e2e needs a Mac for the screenshot baselines; `VOS_PORT_BASE=9100` runs it in a second
worktree (Storybook = base, e2e host = base + 10).

## Commands

```sh
bun run dev             # build:apps, then the vos host on :7400 (dev_reload) + Vite on :5173; pair with the code the host prints
bun run build:apps      # bundle every apps/*/app.json into one Luau file (darklua) + app refs + registry.json under dist/apps
bun run build           # build the client (client/dist)
bun run host            # cargo run -p vos-host: serves client/dist, /rpc, /agui, /mcp on :7400
bun run host pair       # print a fresh device pairing code (every client must pair first, even on loopback)
bun start               # the same host, built with the release-host profile (opt-level z, panic abort)
cargo run -p neoc -- run script.lua   # the neoc CLI
bun run lint            # conflict markers + atomic layers + test ids + boundaries + rust (fmt, clippy -D warnings, lint:deps, cargo deny) + luau (luau-lsp) + stylua + codegen:check + package eslint
bun run typecheck       # cargo check --workspace --all-targets --locked + tsc on types/ + svelte-check in each package
bun run test            # cargo nextest over the vos crates (neoc's crates are in check-neoc.sh) + bun test (types, scripts, client, kit)
bun run test:e2e        # all Storybooks + desktop + journeys, headless, against the real host
bun run test:e2e:update # rebaseline snapshots (then look at them)
bun run storybooks list # every Storybook (client, packages/ui); each package: bun run storybook
bun run format          # prettier + cargo fmt + StyLua
bun run codegen         # regenerate types/generated/* (vos.d.luau, vos.ts, ui.lua, prelude.lua) from the Rust schema and types/neoc.d.luau
bun run codegen:check   # fails when they are stale (part of bun run lint)
cargo run -p neoc-vm --example luau_decls   # regenerate types/neoc.d.luau (-- --check fails on drift)
bun scripts/design-audit.ts <page>  # static styleguide audit of a built-in shell page's styles
```

## Definition of done

A change is done when all of these hold:

1. The gate passes on the head commit (`bun run gate`, or `scripts/check-neoc.sh` for a change confined to the neoc crates).
2. New behaviour has new checks, in the layer it lives in (see "Tests"): for neoc, Rust tests and a `*.test.lua` suite; for vos,
   `cargo nextest` tests (domain, use cases against the port fakes, adapters, the host end to end), `bun test` for client and
   kit logic, a story per new screen or state, a journey spec per new or changed journey. Changed snapshots were looked at.
3. Every interactive vos element has a `data-testid` (kit components take a required `testid` prop; a Luau app's interactive
   nodes have a `key`); it works by keyboard, in light and dark, and at the app's minimum window.
4. UI changes carry a `verify-change` "Manual verification" record in the PR.
5. For a new app or a change people will notice in one: the `pr-review` section "vos checks" (V5, the readiness table) holds,
   including a `ux/<app>/round-<n>.md` that meets the `app-design` exit criteria.

## Adding things

- **A neoc module:** [CONTRIBUTING.md](CONTRIBUTING.md#adding-a-new-module).
- **A vos app:** `apps/README.md`, "Adding an app". From its first commit the app has an `app.json`, an `app.manifest` with a
  `window` min size (and a comment when the layout needs more than 320x240), a `JOURNEYS.md` plus `e2e/journeys.spec.ts`, and a
  Rust test of its logic and tools next to the others in `crates/vos-lua/tests`. The generic `desktop` project opens every
  `apps/*/app.json` in the real shell with no extra code; each screen and state beyond the landing view is reached by a
  journey or shown by a JSON-tree story in the client Storybook. The app is `lint:luau` (strict) and StyLua clean from the
  first commit. Then design it with `app-design` until the exit criteria hold; until then it stays in its PR.
- **A kit component or renderer adapter:** a component in the right atomic layer of `packages/ui` with a required `testid` prop
  and a story; an adapter under `client/src/view/<layer>/` that takes `AdapterProps`, reads props through `useView().view`
  inside `$derived`, renders children with `<Children>`, and is registered in `client/src/view/kit.ts`; the component's schema
  (props, events, slots, accessibility rules) is a Rust change in `vos-domain` first, then `bun run codegen`
  (`coding-styleguide`, `references/typescript-svelte.md`).

## Skills: when to use which

Each skill covers both products; where the checks differ it has a section per product. The skills are in `.claude/skills/`.

| When | Skill |
|---|---|
| Designing a new neoc module, or reviewing a runtime design | `runtime-design`; `requesting-module` for a request to add one (the `std-module`, `lib-module` and `vnd-module` agents implement them) |
| Writing or changing code (Rust, Luau, Svelte/TypeScript) | `coding-styleguide` |
| After implementing, before pushing | `change-review`, `clean-code-review`, then `pre-commit` (loops until clean; `refactor` clears markers) |
| Hardening tests, finding unhappy paths a suite misses | `chaos-monkey` |
| Before marking a PR ready, and again after fixes | `pr-self-review` |
| Reviewing a PR (independent; verdict READY TO MERGE, CHANGES NEEDED or HOLD; applies the vos checks when the diff touches vos) | `pr-review` |
| Any change to what a person sees or does | `verify-change` |
| Before building a new app (or re-scoping one) | `prebuild-synthetic-ux` (personas, riskiest assumptions, key journeys and stories, to `ux/<app>/discovery.md`) |
| Designing or redesigning an app | `app-design` (discovery, journeys, usability review, the `ux-research` workflow, conformance to `app-design/STYLEGUIDE.md`, iterate; reports in `ux/<app>/`) |
| Reviewing the usability of something built | `postbuild-synthetic-usability` (task walkthroughs on the real host, heuristics and WCAG, to `ux/<app>/review-<n>.md`) |
