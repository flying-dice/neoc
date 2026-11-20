# CLAUDE.md

Guidance for Claude (and any LLM agent) working in **neoc**: one Cargo workspace holding a Lua runtime.

- **Runtime** (`crates/neoc-vm`): an embedded LuaJIT engine backed by Rust. Scripts run in a VM with a curated module surface (`std:*`, `lib:*`, `vnd:*`). The unsafe halves of the Lua standard library are stripped; everything else comes through explicit modules. Not a security boundary.
- **Binary** (`crates/neoc`): the thin `neoc` CLI over the runtime. `run` only.

neoc used to be a language that compiled to Lua. That is gone; the last state is under the `neoc-lang-final` tag. Restore a path with `git checkout neoc-lang-final -- <path>`.

Read [README.md](README.md) for what neoc is and [CONTRIBUTING.md](CONTRIBUTING.md) for the build, the pre-PR checks, how to add a module and how to release.

## Layout

```
crates/neoc/                 # the `neoc` binary (the only bin)
  src/main.rs                # CLI entry: `run`, embedded-payload executables
  tests/                     # Lua suites (*.test.lua, *.spec.lua), cli.rs, cucumber.rs
crates/neoc-vm/              # the runtime library
  tests/script_parse.rs      # compiles (never runs) every Lua script, Docker or not
  src/errors.rs              # runtime error types (NeocError)
  src/lua/sandbox.rs         # engine construction, stripped globals
  src/lua/facade.rs          # Module builder: registration, spans, (value, error), the LuaCATS emitter
  src/lua/modules.rs         # module registration + `require`
  src/lua/{std,lib,vnd}/     # one file per module
  examples/luals.rs          # writes (or `-- --check`s) the LuaCATS definitions in types/luals/
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

The full pre-PR gate, including the Lua suite set that needs no Docker, is in [CONTRIBUTING.md](CONTRIBUTING.md#pre-pr-checks).

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
- **Types.** Every module's `declare` records its surface (functions, parameter and return types, docs, resource types with their methods, fields and constructors) and that record is the source of the editor types in `types/luals/`, never edited by hand: `cargo run -p neoc-vm --example luals` regenerates, `-- --check` fails on drift. Keep the record true to the runtime; the drift tests in `modules.rs` check that every live table key is recorded, that every recorded method, field and static exists, and that every named type has a class; they do not check parameter or return types against the runtime, and the types they cannot build (`NOT_BUILT`: Docker, a server, a worker) are not held to a live value. The vocabulary is `string`, `number`, `bool`, `()` (nothing), `LuaValue` (opaque, `any`), a trailing `?` for optional, `Result<T, string>` for `(value, err)`, `(A, B)` for several returns, a trailing `("...", "LuaValue")` for variadic arguments, and a type's own name for a resource type.

Adding a module: see [CONTRIBUTING.md](CONTRIBUTING.md#adding-a-new-module).

## Tests

- **Rust:** `#[cfg(test)] mod tests` next to the code. Integration tests in `tests/` only where they cross crate boundaries, plus `crates/neoc-vm/tests/luals.rs` (the generated definitions and the lua-language-server fixtures), `crates/neoc-vm/tests/hostile_inputs.rs` (every module called with hostile values, fails on any panic) and `forged_len.rs`.
- **Lua:** a new module gets `crates/neoc/tests/<ns>/<name>.test.lua`, run with `neoc run`. (Not every existing module has one yet: `std:net`, `std:workers` and `lib:test` have none, and `std:task` uses a `.spec.lua`.) Per exported function: one happy path, one test per distinct error condition, and an empty or zero value where it applies. The outer `describe` is the bare module name.
- **CLI spec:** `tooling/cli/*.feature`, driven by `crates/neoc/tests/cucumber.rs`.
- **Benches:** three tiers per module (`library`, `into_out_lua`, `through_lua`), behind the `bench` feature.

## Rust style

- Edition 2024. `unsafe_code` is denied in the workspace lint table, which the crates do not inherit yet (see their `[lints.cargo]` comment). `neoc-vm` forbids `unsafe` outside tests (`lib.rs`); the `neoc` binary crate still has some, in its CLI test helper.
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
