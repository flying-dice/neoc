# Contributing

Build, test, add a module, release. The module design rules are in [CLAUDE.md](CLAUDE.md#modules); what neoc is, is in [README.md](README.md).

## Before you start

- `rustup` and `cargo` on `PATH`. The toolchain is pinned in [rust-toolchain.toml](rust-toolchain.toml); rustup installs it on first use. Don't use `+stable`.
- Bumping the toolchain is its own PR: edit `rust-toolchain.toml`, the `Dockerfile` builder image and any `.github` workflow reference together, fix new clippy/rustc fallout in that PR, and run `scripts/check-toolchain-pin.sh`.

## Build and test

```sh
cargo build
cargo test -p neoc -p neoc-vm
```

`cargo test -p neoc -p neoc-vm` runs the `neoc-vm` unit tests, the `neoc` binary tests, the CLI integration tests and the Gherkin spec in `tooling/cli/`.

The workspace also holds the vos crates (`crates/vos-*`, governed by [AGENTS.md](AGENTS.md)). neoc's own checks do not need what those crates need: the test line names `-p neoc -p neoc-vm`, so neoc contributors need neither `bun` nor `darklua`. The vos crates have their own gate (`bun run lint`, `typecheck`, `test`, `test:e2e`, in AGENTS.md); their tests build the bundled apps, which needs `bun` and `darklua` 0.18.0 on `PATH`, and `cargo test --workspace` would run them. `cargo build` and `cargo clippy --all-targets` cover the vos crates too, and need nothing extra. NEOC-115 unifies the two gates.

Lua suites run with the binary itself:

```sh
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.lua'
```

## Pre-PR checks

Run all of these locally before opening a PR. They must pass exactly as written.

```sh
cargo fmt --check
RUSTFLAGS="-D warnings" cargo clippy --all-targets
cargo test -p neoc -p neoc-vm
cargo run -p neoc -- run \
  "crates/neoc/tests/lib/*.test.lua" \
  "crates/neoc/tests/std/*.test.lua" \
  "crates/neoc/tests/std/task.spec.lua" \
  "crates/neoc/tests/app.test.lua" \
  "crates/neoc/tests/vnd/jsonschema.test.lua" \
  "crates/neoc/tests/vnd/quick_xml.test.lua" \
  "crates/neoc/tests/vnd/rusqlite.test.lua" \
  "crates/neoc/tests/vnd/serde_json.test.lua" \
  "crates/neoc/tests/vnd/serde_yaml.test.lua" \
  "crates/neoc/tests/vnd/toml.test.lua"
cargo test --release -p neoc-vm --test abort_regressions
cargo bench -p neoc-vm --features bench --bench lib --bench vnd -- --test
scripts/check-toolchain-pin.sh
cargo deny check
cargo run -p neoc-vm --example luau_decls -- --check
NEOC_REQUIRE_LUAU_LSP=1 cargo test -p neoc-vm --test luau_decls
```

`abort_regressions` holds every case to `neoc_vm::VM_THREAD_STACK` (8 MiB) of stack in a debug build, which is what `neoc run` and an embedder that follows `lifecycle.rs` give a thread that polls the VM, and to tokio's default 2 MiB in a release build; the release run is a separate line above because it is the one that proves a default-size thread survives. Building Luau needs a C++ compiler (`mlua-sys` compiles it).

The Lua list leaves out the suites that need a Docker daemon (`sqlx_mysql`, `sqlx_postgres`, `testcontainers_modules`). With Docker running, run every suite instead: `cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.lua' 'crates/neoc/tests/**/*.spec.lua'`.

The last two lines are the editor types. `luau_decls -- --check` fails when `types/neoc.d.luau` (the generated Luau declarations of every module, one definitions file) no longer matches what the modules record; fix it with `cargo run -p neoc-vm --example luau_decls` and commit the result. The `luau_decls` test runs the fixtures in `types/fixtures/` through `luau-lsp analyze` in strict mode: the files in `types/fixtures/` outside `negative/` must check clean, and a line of `types/fixtures/negative/` marked with `-- expect-error: <text>` must carry exactly one finding per marker (a line with two findings carries two markers), each marker matching a different finding's message up to its `caused by:` lines, and no unmarked line may carry a finding. `NEGATIVE_MARKERS` in the test pins the number of markers per file, so deleting a wrong line with its marker is an edit of that table. luau-lsp must be on `PATH` (`luau-lsp.exe` on Windows) at the pinned version (`LUAU_LSP_VERSION` in `crates/neoc-vm/tests/luau_decls.rs`; take the release binary of that version from the luau-lsp GitHub releases, there is no Homebrew formula; a binary that is not on `PATH` counts as missing); without `NEOC_REQUIRE_LUAU_LSP=1` it skips with a notice when the tool is missing or not that version, so `cargo test -p neoc -p neoc-vm` still passes without it. The same file also runs the language server the way the editor starts it (typed `require`, a wrong member, hover), under the same variable. The other tests in that file (the committed file is current, the example's `--check` and argument exit codes, the fixture matcher, no file name has a colon, the positive fixture requires every module) always run.

The bench line compiles every bench and runs each body once, with no timing. It catches stale bench code the other checks never build. To time a module, drop `-- --test` and name it: `cargo bench -p neoc-vm --features bench --bench lib <name>`.

GitHub Actions (`.github/workflows/ci.yml`) runs similar checks on pushes to `main`, plus `cargo deny check advisories`, the release-binary cross builds and the Docker image build. It is not a gate and is not relied on: the local run is the verification.

## Branching and PRs

`main` is the only long-lived branch.

1. Branch from `main`: `issues/{key}-{slug}` (e.g. `issues/NEOC-6-std-task`).
2. Run the [pre-PR checks](#pre-pr-checks) locally.
3. Open the PR into `main` (a draft is fine). One module or one well-scoped change per PR. Say why in the description, not only what. Reference the Jira key (`Refs NEOC-<n>`).
4. Self-review with the `pr-self-review` skill and post the `Self-review` note on the PR for the head SHA.
5. Hand off to an independent `pr-review` pass: a fresh reviewer with no session context, who posts a verdict note for the head SHA (`READY TO MERGE` or `CHANGES NEEDED`). Fix and re-run from step 2 until `READY TO MERGE`.
6. Squash-merge only on `READY TO MERGE` for the current head, then delete the branch. The one exception is the merge of vos into neoc (NEOC-113, PR #34), which lands with a merge commit so that vos's history stays in `main`: never squash it.

### Commits and PR titles

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`. The squash commit message comes from the PR title (a PR with several commits) or from the commit itself (a single-commit PR), so keep both in this format.

| Type | When |
|---|---|
| `feat` | New capability |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `refactor` | No behaviour change |
| `test` | Tests only |
| `perf` | Performance |
| `build`, `ci`, `chore` | Tooling and maintenance |

Scope is optional (a module or a subsystem). Subject is imperative, lowercase, no trailing period. One logical change per commit. A breaking change takes `!` after the type/scope and a `BREAKING CHANGE:` footer. The Lua surface of a module and the CLI are the user-visible contracts. A new crate dependency needs the owner's sign-off; call out any benchmark regression in the PR.

## Adding a new module

Example: a `lib:foo` module. Pick the namespace first (see [CLAUDE.md](CLAUDE.md#modules)). Copy the shape of a small existing module such as `crates/neoc-vm/src/lua/lib/hex.rs`; don't write one from scratch.

1. **Module file.** `crates/neoc-vm/src/lua/lib/foo.rs` exports `pub fn module(lua: &Lua) -> mlua::Result<Table>` and a `declare` that builds the surface through the `Module` facade (`crates/neoc-vm/src/lua/facade.rs`). Each function carries its name, parameter types and a one-line description. Fallible functions return `(value, err)`.
2. **Register.** Add `pub mod foo;` to `crates/neoc-vm/src/lua/lib/mod.rs`, a `("lib:foo", lib::foo::module)` row to the `REGISTRY` table in `crates/neoc-vm/src/lua/modules.rs`, and `lib::foo::declare(lua)?` to `declared()` in the same file, each alphabetical in its namespace (`declared_modules_match_registered_keys` fails when the two lists disagree). Add `foo` to the `runtime_tables` list in that file's tests and update the module count the tests assert (`declared_modules_match_registered_keys`, `every_module_yields_valid_declarations`, and `embedding.rs::the_registry_is_an_api_an_embedder_can_build_its_own_require_from`). `hostile_inputs.rs` reads the names from `modules::names()`, so the new module is swept without a change there.

   **An async function** (one whose body awaits) is made with `lifecycle::async_function` / `async_method` and a `Role`, never with `Lua::create_async_function` or `add_async_method` (clippy refuses them, see `clippy.toml`). Choose the role by what the body does, not by what it calls: **`Role::Wait`** when it waits for something that does not use the VM (the network, a socket, a worker, a file, a lock a script cannot be holding): the facade's `async_func` / `try_async_func` are `Role::Wait`, which is right for nearly every I/O module; **`Role::Timer`** for a wait that is only a timer (`task.wait`, a `std:time` sleep); **`Role::Vm`** when the body runs script code or uses state the embedder protects with its turn (`lib:test`'s `describe` runs a Lua callback); **`Role::Mixed`** when it does both in turns, and then the body starts without the turn and calls `lifecycle::idle` for each wait and `lifecycle::hold` before it touches what needs the turn (`Mutex:with_lock`, `server:wait()`). With no embedder none of this changes what the function does; it decides when an embedder lends its turn. Anything a script starts that outlives the call (a task, a server, a listener) registers with the engine's `Life` (`Life::of(lua)`), so shutting the script down stops it: see "Embedding the VM" in [CLAUDE.md](CLAUDE.md).
3. **Rust tests.** A `#[cfg(test)] mod tests` in the module file, driving the full engine from `crate::lua::sandbox::make_engine()`. Run: `cargo test -p neoc-vm foo`.
   The drift tests in `modules.rs` (`definitions_cover_runtime_table_keys`, `recorded_members_exist_on_live_values`, `every_named_type_is_declared`) hold what `declare` records to the live module (through freshly generated text; `committed_declarations_match_the_modules` in `tests/luau_decls.rs` holds the committed `types/neoc.d.luau` to it): record every function with its real return shape (`Result<T, string>` for `(value, err)`, `()` for nothing, `(A, B)` for several values of its own, a `...` among them (or alone) for every value a Lua function it calls returned), end a function that takes trailing arguments with `("...", "LuaValue")`, and record each resource type's methods, fields (`field`) and the constructors on its sub-table (`static_fn`). A type a unit test can build goes in `INSTANCES`, one it cannot (Docker, a running server) in `NOT_BUILT` with the reason.
4. **Benches.** A `#[cfg(feature = "bench")] pub fn benches(c: &mut Criterion)` with three tiers: `library` (pure Rust), `into_out_lua` (marshalling cost) and `through_lua` (the full user path). Add it to the `criterion_group!` in `crates/neoc-vm/benches/lib/lib.rs`. Run: `cargo bench -p neoc-vm --features bench --bench lib foo`.
5. **Lua test.** `crates/neoc/tests/lib/foo.test.lua`, using `lib:test`:

   ```lua
   local _t       = require("lib:test")
   local describe = _t.describe
   local test     = _t.test
   local expect   = _t.expect

   local foo = require("lib:foo")

   describe("foo", function()
       describe("greet", function()
           test("basic case", function()
               expect(foo.greet("world")).toBe("hello, world")
           end)
       end)
   end)
   ```

   Run: `cargo run -p neoc -- run crates/neoc/tests/lib/foo.test.lua`. Per exported function: a happy path, one test per distinct error, and an empty or zero value where it applies.
6. **Types.** Run `cargo run -p neoc-vm --example luau_decls` to add the module's section to `types/neoc.d.luau` (its `export type lib_foo`, one `lib_foo_<Type>` per resource type, a `lib_foo_<Type>Statics` for each type with constructors or constants, and the `require` overload), commit it, and add a `local foo = require("lib:foo")` line (the check wants `= require("lib:foo")` on a line that is not a comment) and a few correct calls to `types/fixtures/usage.luau`; a wrong call goes in `types/fixtures/negative/wrong_use.luau` with `-- expect-error: <text>` at the end of the line, where `<text>` is part of the luau-lsp message before its `caused by:` lines (`Key 'nope' not found in table 'lib_foo'`, `expects 1 argument, but none are specified`, ...); a line that gives two findings takes two markers (a wrong `require("lib:fooo")` also gives `Other overloads are also not viable`), and the new markers go in `NEGATIVE_MARKERS`.
7. **README.** Add the module to the list in the README modules table, alphabetical within its namespace.

Extending an existing module follows the same steps: add the function and register it on the table, add the Rust and Lua cases, extend the bench group, and re-run the `luau_decls` example (the `luau_decls` test fails on a stale `types/neoc.d.luau`) with a call in `types/fixtures/usage.luau`.

## How to release

Releases are cut by pushing a tag on `main`. There is no release bot and no release PR.

```sh
git checkout main && git pull
git tag vX.Y.Z
git push origin vX.Y.Z
```

The tag must match `vMAJOR.MINOR.PATCH`, optionally with a `-<pre>` suffix (`v0.2.0-rc.1`), and must point at a commit on `main`. The `Release` workflow (`.github/workflows/release.yml`):

1. Stamps the tag's version into the workspace `Cargo.toml` in the CI checkout only (`scripts/stamp-version.sh`), so `neoc --version` reports it. Nothing is committed back.
2. Builds `neoc-x86_64-unknown-linux-gnu` and `neoc-x86_64-pc-windows-gnu.exe`.
3. Creates the GitHub Release with generated notes, both binaries and `SHA256SUMS`. A `-<pre>` tag is marked as a pre-release.

A build failure, or a tag not on `main`, fails the workflow without publishing. It needs only the default `GITHUB_TOKEN`. `scripts/install.sh` and `scripts/install.ps1` install from the latest non-pre-release; set `NEOC_VERSION=vX.Y.Z-rc.1` to install a pre-release.
