# CLAUDE.md

Guidance for Claude (and any LLM agent) working in the **neoc** monorepo — one
Cargo workspace holding a Lua runtime:

- **Runtime** (`crates/neoc-vm`) — an embedded LuaJIT engine backed by Rust. Lua
  scripts run in a VM with a curated module surface (`std:*`, `lib:*`,
  `vnd:*`). The unsafe halves of the Lua standard library are stripped;
  everything scripts need is provided through explicit modules. This is not a
  security boundary.
- **Binary** (`crates/neoc`) — the thin `neoc` CLI over the runtime: `run` only.

neoc used to be a language that compiled to Lua. That is gone; the last state is
preserved under the `neoc-lang-final` tag. Restore a path from it with
`git checkout neoc-lang-final -- <path>`.

---

## Crate isolation

Two crates; the boundary is enforced by `Cargo.toml`, not convention:

- `neoc-vm` is the library. It owns the `mlua`/`tokio` stack, the sandbox and
  every module. It depends on no other workspace crate.
- `neoc` is the binary. It owns the CLI tree, glob expansion, the tokio runtime
  the engine drives on, and process exit policy. It depends on `neoc-vm`.

Don't reach across the boundary to "shortcut" something. If a need crosses
cleanly, lift it into `neoc-vm`'s API. If it doesn't, the design is wrong —
flag it.

---

## Build and test

```sh
cargo build                                          # debug build
cargo build --release                                # release build
cargo test --workspace                               # Rust tests + CLI integration tests + CLI Gherkin spec
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.lua'  # Lua integration tests (Docker suites need a daemon)
cargo bench -p neoc-vm --features bench --bench lib  # Criterion benchmarks (lib modules)
cargo bench -p neoc-vm --features bench --bench vnd  # Criterion benchmarks (vnd modules)
cargo clippy --workspace --all-targets               # lint
cargo fmt --check                                    # formatting check
```

---

## Project structure

```
crates/neoc/                 # the thin `neoc` binary (the only bin)
  src/
    main.rs                  # CLI entry: `run`, embedded-payload executables
  tests/                     # Lua integration tests (*.test.lua, *.spec.lua),
                             # cli.rs, cucumber.rs

crates/neoc-vm/              # the LuaJIT runtime library
  src/
    lib.rs                   # library root (pub mod errors, lua)
    errors.rs                # runtime error types (NeocError)
    lua/
      sandbox.rs             # engine construction
      facade.rs              # Module builder: registration, spans, (value, error), recorded signatures
      modules.rs             # module registration + `require`
      std/                   # std:* modules (collections, fs, io, net, thread, …)
      lib/                   # lib:* modules (base64, json, test, vcr, …)
      vnd/                   # vnd:* modules (hyper, serde_json, rusqlite, sqlx_*, …)
  benches/                   # Criterion suites (lib/, vnd/)

tooling/cli/                 # Gherkin spec for the CLI, driven against the built binary
docs/                        # documentation site (VitePress): modules, guides, ADRs, RFCs
```

---

## Runtime: module namespaces

Three namespaces, chosen at registration in `crates/neoc-vm/src/lua/modules.rs`.
No ambient globals — `require("namespace:name")` is the only way in:

- `std:*` — mirrors Rust std (`std:collections`, `std:fs`, `std:net`).
- `lib:*` — blessed gap-fillers and custom modules (`lib:base64`, `lib:test`).
- `vnd:*` — vendored crates; name matches the upstream crate (`vnd:hyper`,
  `vnd:serde_json`).

### Design principles (non-negotiable)

Priority order, 1 > 2 > 3 > 4:

1. **No surprises** — behave like every other major runtime. If Rust, Go,
   Python, and Node agree, follow them.
2. **No footguns** — never ship an API that is unintuitive or causes silent
   mistakes. If a design needs a warning paragraph, redesign it.
3. **Mirror Rust first** — primary API is `UserData` types mirroring `std::*`
   1:1 (same methods, same semantics).
4. **Supplement with scripting ergonomics** — flat functions (Python/Node-style)
   layered on top. They delegate to the UserData layer and never introduce their
   own design decisions.

### Key conventions

- Every module exports a single `pub fn module(lua: &Lua) -> mlua::Result<Table>`.
- Infallible Lua functions return the value directly. Fallible ones return
  `(Value, Value)` using `ok`/`from_err` helpers.
- Error prefixes use `"module.function"` format (no namespace prefix).
- Resource handles use `UserData` with methods. Multi-handle ops are free
  functions.
- Dependencies: `std:*` uses std + blessed.rs. `lib:*` adds focused vendored
  crates. `vnd:*` wraps exactly one crate.
- No speculative abstractions — add helpers only when 3+ call sites exist.

### Adding a module

1. Create `crates/neoc-vm/src/lua/<ns>/<name>.rs` with a `module` function.
2. Register in `crates/neoc-vm/src/lua/<ns>/mod.rs` (alphabetical).
3. Add `#[cfg(test)] mod tests` in the file.
4. Add Criterion benches (gated behind the `bench` feature).
5. Add `crates/neoc/tests/<ns>/<name>.test.lua`.

---

## Rust code style

- **Edition 2024.** `unsafe_code` is denied workspace-wide. If you genuinely need
  `unsafe`, lift it into its own clearly named crate and explain why.
- **Lints.** `clippy::all` + `clippy::pedantic`. CI fails on warnings
  (`RUSTFLAGS: -D warnings`). Use `#[allow(...)]` with a comment when you must.
- **Errors.** `thiserror` for library error enums; the CLI bin renders them with
  `miette`. Never `unwrap()` / `expect()` in library code — return `Result`.
- **Tests.** Live next to the code as `#[cfg(test)] mod tests`. Integration
  tests in `tests/` only when they truly cross crate boundaries. Every module has
  a Lua integration test in `crates/neoc/tests/<ns>/`.

---

## House Style: Terse

Write tight. Cut every word that doesn't carry meaning. This repo favours short,
direct prose — docs, comments, commits, PR bodies, chat — stripped of filler,
hedging, and ceremony. Keep the substance, drop the padding.

**Cut these:**
- Pleasantries — "Sure, I'd be happy to help" → just answer.
- Hedging — "it might be worth considering" → say it plainly.
- Filler — "the reason this happens is because" → "because".
- Throat-clearing — get to the point in the first sentence.

**Before:**
> The reason the `match` is being rejected as non-exhaustive is likely because
> the scrutinee is an `enum` and one of its variants has no corresponding arm.

**After:**
> The `match` is non-exhaustive because the `Color` enum has a variant — `Blue`
> — with no arm. Add `Color::Blue => …` or a `_ =>` catch-all.

### Terse, not cryptic

Tight writing stays clear and grammatical. We remove waste, not articles. Some
things stay exact and complete — **never** abbreviate or stylise these:

| Thing | Rule |
| --- | --- |
| Code blocks | Real code, real syntax. |
| API names | Exact. `#[derive(Clone)]` stays `#[derive(Clone)]`. |
| Type signatures | Exact. Never trim a type. |
| Error / log messages | Quote verbatim. |
| Commands | Exact: `neoc run`, `cargo test --workspace`. |
| File paths, env vars | Exact. |
| Rustdoc (`///`) on public items | Full, normal English — see below. |
| Legal, LICENSE, security notes | Complete and careful. |

Rule of thumb: **trim filler, never trim fact.**

### Docs enforcement

Two doc layers, different rules.

**1. Prose docs** (guides, README body, tutorials) → terse. Short
sentences, lead with the point, no ceremony.

**2. Rustdoc on public items** (`///` on anything `pub`) → full, normal English.
`cargo doc` renders these for embedders and the docs site; they must be clear,
complete, and grammatical. Clipped phrasing in a `# Arguments` block is bad DX.

```rust
/// Encode `input` as base64 using the standard alphabet.
///
/// # Arguments
///
/// * `input` — the bytes to encode. Caller owns the buffer; the function does
///   not retain a reference once it returns.
/// * `padded` — whether to append `=` padding. Pass `false` for the unpadded
///   form used in URLs and JWTs.
///
/// # Errors
///
/// Returns `Error::InputTooLarge` when the encoded length would overflow
/// `usize`; this cannot happen for in-memory inputs on a 64-bit target.
pub fn encode(input: &[u8], padded: bool) -> Result<String, Error> { /* ... */ }
```

Prose around a code sample stays terse. The doc comment itself stays complete.

---

## Voice

Direct. No marketing language. Friendly error messages that help the user, never
scold. No rabbit / spring / Bun / Easter motifs — that brand belonged to an
earlier life of the project when it targeted TypeScript on Bun. neoc is now a
Lua runtime; the voice is plain.

---

## Commits & PRs

Use [Conventional Commits](https://www.conventionalcommits.org). Terse, but
spec-compliant. Releases are cut by pushing a `vX.Y.Z` tag (see CONTRIBUTING.md);
release notes are generated from merged PRs and commits.

```
fix(std:fs): read_to_string reports the path in its error

Previously the error named only the OS cause. Now it prefixes the path
the caller passed.

Refs NEOC-42.
```

- **Type** — one of: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`,
  `test`, `build`, `ci`, `chore`, `revert`.
- **Scope** — optional. The binary: `cli`. Runtime: use the module (`std:fs`) or
  subsystem (`vm`, `install`). Cross-cutting: `workspace`, `ci`.
- **Description** — imperative, lowercase, no trailing period.
- **Body** — optional, terse prose. Bullet what changed. Use it when the *why*
  isn't obvious from the subject.
- **Footer** — `Refs NEOC-42`, etc.
- **Breaking changes** — append `!` after type/scope (`feat(cli)!:`) **and**
  add a `BREAKING CHANGE:` footer with full, careful English. The Lua surface of
  a module and the CLI are the user-visible contracts — changes there almost
  always warrant `!`.
- One logical change per commit. If you're tempted to write "and" in the
  subject, split it.

PR titles follow the same format — the squash-merge commit inherits them, so
they feed the changelog.

---

## Issue workflow — Jira and GitHub

Work is tracked in [Jira](https://flying-dice.atlassian.net/jira/software/projects/NEOC)
(project `NEOC`). Source and pull requests live on GitHub at
`flying-dice/neoc`. Jira is the source of truth for ticket state.

- **Branches.** Branch from `develop`: `issues/{key}-{slug}` for implementation
  (e.g. `issues/NEOC-6-std-task`), `design/{key}-{slug}` for RFC/ADR work.
- **Pull requests.** Open against `develop`. Title follows Conventional
  Commits. Reference the Jira key in the body or commit footer (`Refs NEOC-6`).
- **Review.** CODEOWNERS approval is required before merge. Reviewers use the
  `pr-review` skill; authors run `pr-self-review` first.
- **CI.** Minutes are limited: CI runs on push to `main` and `develop` (docs-only pushes excluded), on
  manual dispatch (`gh workflow run ci.yml --ref <branch>`), and weekly. It does
  not run per PR. Verify locally before opening a PR (see Build and test).
- **Gates.** Never move a ticket out of Backlog without explicit CODEOWNER
  instruction. A design PR needs CODEOWNER approval before implementation starts.
- **Done.** Merge to `develop`, then mark the Jira ticket Done.

---

## Cross-platform

The binary supports Linux, macOS, and Windows. Signal handling uses
`#[cfg(unix)]` / `#[cfg(not(unix))]` conditionally. `mlua` is built `vendored`
(bundles LuaJIT). `rusqlite` uses `bundled` (bundles SQLite).

---

*Few words. Full meaning.*
