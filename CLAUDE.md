# CLAUDE.md

This file is for Claude Code — it describes how to work in this repo.

## What this is

`neoc` is an embedded Luau runtime backed by Rust. Lua scripts run in a sandboxed engine with a curated module surface (`std:*`, `lib:*`, `vnd:*`). The unsafe halves of the Lua standard library are stripped; everything scripts need is provided through explicit modules.

## Build and test

```sh
cargo build                                          # debug build
cargo build --release                                # release build
cargo test --workspace --lib                         # in-module Rust tests
cargo test -p neoc --bin neoc                         # end-to-end subprocess tests
cargo run -p neoc -- 'crates/neoc/tests/**/*.test.luau'  # all Luau integration tests
cargo bench --bench lib                              # Criterion benchmarks (lib modules)
cargo bench --bench vnd                              # Criterion benchmarks (vnd modules)
cargo clippy --all-targets                           # lint
cargo fmt --check                                    # formatting check
```

## Project structure

```
crates/neoc/
  src/
    main.rs            # CLI entry point (script runner + serve mode)
    lib.rs             # Library root (pub mod dto, lua)
    lua/
      sandbox.rs       # Engine construction
      modules.rs       # Module registration
      std/             # std:* modules (collections, fs, io, net, thread, workers)
      lib/             # lib:* modules (base64, json, test, vcr)
      vnd/             # vnd:* modules (hyper, serde_json, rusqlite, sqlx_*, etc.)
  tests/               # Luau integration tests (*.test.luau, *.spec.luau)
  benches/             # Criterion benchmark suites (lib/, vnd/)
```

## Module namespaces

Three namespaces, chosen at registration time in `crates/neoc/src/lua/modules.rs`:

- `std:*` — mirrors Rust std (e.g. `std:collections`, `std:fs`, `std:net`)
- `lib:*` — blessed gap-fillers and custom modules (e.g. `lib:base64`, `lib:test`)
- `vnd:*` — vendored crates, name must match the upstream crate (e.g. `vnd:hyper`, `vnd:serde_json`)

## Design principles (non-negotiable)

Every API decision must satisfy these four rules, in priority order:

1. **No surprises** — behave like every other major runtime. If Rust, Go, Python, and Node agree, follow them.
2. **No footguns** — never ship an API that is unintuitive or causes silent mistakes.
3. **Mirror Rust first** — primary API is `UserData` types mirroring `std::*` 1:1 (same methods, same semantics). Think of it as Rust's best bits wrapped in a GC for lifetimes.
4. **Supplement with scripting ergonomics** — flat functions (Python/Node-style) layered on top for convenience. These delegate to the UserData layer and never introduce their own design decisions.

Precedence: 1 > 2 > 3 > 4.

## Key conventions

- Every module exports a single `pub fn module(lua: &Lua) -> mlua::Result<Table>`.
- Infallible Lua functions return the value directly. Fallible ones return `(Value, Value)` using `ok`/`from_err` helpers.
- Error prefixes use `"module.function"` format (no namespace prefix).
- Resource handles use `UserData` with methods. Multi-handle ops are free functions.
- Dependencies: `std:*` uses std + blessed.rs. `lib:*` adds focused vendored crates. `vnd:*` wraps exactly one crate.
- No speculative abstractions — add helpers only when 3+ call sites exist.

## Adding a module

1. Create `crates/neoc/src/lua/<ns>/<name>.rs` with a `module` function
2. Register in `crates/neoc/src/lua/<ns>/mod.rs` (alphabetical)
3. Add `#[cfg(test)] mod tests` in the file
4. Add Criterion benches (gated behind `bench` feature)
5. Add `crates/neoc/tests/<ns>/<name>.test.luau`

## Commit style

All commits use [Conventional Commits](https://www.conventionalcommits.org/). release-plz reads these to determine the next version and generate the changelog — wrong format means wrong (or missing) releases.

```
<type>(<scope>): <subject>
```

**Types:**

| Type | When to use |
|---|---|
| `feat` | New capability visible to users or scripts |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `refactor` | Code change with no behaviour change |
| `test` | Tests only |
| `chore` | Maintenance (deps, CI tweaks, etc.) that doesn't affect the binary |
| `perf` | Performance improvement |

**Scope** is optional but recommended — use the module name (`std:fs`), subsystem (`ci`, `install`), or omit for repo-wide changes.

**Breaking changes:** append `!` after the type/scope (`feat!:`) and add a `BREAKING CHANGE:` footer.

**Rules:**
- Subject is lowercase, no trailing period.
- Body and footers are optional; use them when the *why* isn't obvious from the subject.
- One logical change per commit. If you're tempted to write "and" in the subject, split it.

## Issue workflow — GitLab board

Issue lifecycle is managed through the [GitLab board](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/boards/3) using scoped `workflow::*` labels. Do not use `draft` labels or `DRAFT:` title prefixes.

**Columns:** Backlog → Refinement → Design → Implementation → Review → Done

Each column maps to a scoped label. Labels are mutually exclusive within the `workflow::` scope — applying a new stage label automatically replaces the old one.

| Label | Meaning |
|---|---|
| `workflow::Backlog` | Not yet refined — awaiting CODEOWNER go-ahead |
| `workflow::Refinement` | Being fleshed out: acceptance criteria, open questions, approach options |
| `workflow::Design` | RFC or ADR in progress; awaiting CODEOWNER approval before implementation |
| `workflow::Implementation` | Implementation merge request open; work in progress |
| `workflow::Review` | Merge request under CODEOWNER review; awaiting approval |
| *(closed)* | Done — issue closed on merge |

**Gates:**

| Transition | Gate |
|---|---|
| Backlog → Refinement | CODEOWNER gives explicit go-ahead. No bot self-promotes from Backlog. |
| Refinement → Design | Flesh out the ticket (acceptance criteria, open questions, approach options). Wait for CODEOWNER signal to advance. |
| Design → Implementation | If a design MR is needed, open one on a `design/{id}-{slug}` branch — CODEOWNER approves before moving to Implementation. If the ticket description is sufficient design, move straight to Implementation once settled. |
| Implementation → Review | Open the implementation MR from an `issues/{id}-{slug}` branch. CODEOWNER reviews. Label stays `workflow::Review` until approved. |
| Review → Done | CODEOWNER approves the MR, merge to `main`, close the issue. |

**Branching model:**

- Implementation work: `issues/{id}-{slug}` (e.g. `issues/6-std-task`)
- Design artefacts: `design/{id}-{slug}` (e.g. `design/23-std-env`)

**Additional labels:**

| Label | Purpose |
|---|---|
| `module` | New or updated runtime module (std/lib/vnd) |
| `infrastructure` | CI, release, scripts, tooling |
| `priority:high` | Must ship in the current milestone |
| `priority:medium` | Should ship in the current milestone |
| `priority:low` | Nice to have; deferred if scope is tight |
| `design-rfc` | Design RFC required before implementation begins |
| `design-adr` | Architecture Decision Record produced by this issue |
| `spike` | Time-boxed investigation; produces a recommendation, not production code |

**Milestones:**

| Milestone | Scope |
|---|---|
| M1: std — Stable Standard Library | All `std:*` modules stable, tested, documented |
| M2: lib — Blessed Library Extensions | All `lib:*` modules stable, tested, documented |
| M3: vnd — Vendor Libraries | All `vnd:*` wrapper modules stable, tested, documented |

**Hard rules:**
- The `workflow::*` label is the source of truth for issue state — not the issue's open/closed status alone.
- Never move an issue out of `workflow::Backlog` without explicit CODEOWNER instruction.
- CODEOWNER approval is still required at the Review gate before merge (see CODEOWNERS file).
- The GitLab project is hosted at `gitlab.beluga-sirius.ts.net/flying-dice/neoc`.

## Cross-platform

The binary supports Linux, macOS, and Windows. Signal handling uses `#[cfg(unix)]` / `#[cfg(not(unix))]` conditionally. `mlua` is built with `vendored` (bundles Luau). `rusqlite` uses `bundled` (bundles SQLite).
