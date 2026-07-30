# CLAUDE.md

Guidance for Claude (and any LLM agent) working in the **neoc** monorepo — one
Cargo workspace holding two halves:

- **Runtime** (`crates/neoc-vm`) — an embedded Luau engine backed by Rust. Lua
  scripts run in a sandboxed VM with a curated module surface (`std:*`, `lib:*`,
  `vnd:*`). The unsafe halves of the Lua standard library are stripped;
  everything scripts need is provided through explicit modules. The `neoc`
  binary is a thin shell over this library.
- **Compiler** (`neoc-*` crates) — the Rust compiler for the **neoc** language.
  neoc emits one typed superset: `luau`, selected with `--target luau`. Plain Lua
  is **not** a selectable target — `luau` downgrades to Lua 5.1 through the Luau
  toolchain, so neoc emits the typed superset and lets the toolchain produce the
  plain form. (The `codegen-lua` crate stays: `luau` reuses it behind the codegen
  seam.) A `.luau` module is self-contained — no sidecars, no runtime library.
  The library crate (`neoc-compiler`) is what the runtime embeds; the grammar
  lives in the `neoc-grammar` submodule.

One binary, `neoc`, drives both: `run` / `emit-headers` (runtime) and `build` /
`check` / `fmt` / `lsp` (compiler).

---

## Golden rule: no host-language leaks

**The host language (Rust, today) must never leak into neoc.** A user writing
`.neoc` should never see, hear, or need to know the compiler is written in
Rust — and the same source must compile identically through any future
re-implementation in another host language.

Concretely:

- **Diagnostics, hover, completion, error messages** speak neoc. Say
  `Result<T, E>`, `struct`, `ext fn`, `match arm`. Never say `enum variant`,
  `trait bound`, `Option<&str>`, `borrowed value`, `lifetime`, `Cow<'_, str>`,
  or quote a `thiserror` Display impl verbatim.
- **Type names in errors are neoc types.** `number`, `string`, `bool`,
  `Result<T, E>`, user-defined `struct`s. Not `i64`, `String`, `&str`, `usize`.
- **Spec prose is host-agnostic.** `lang/` describes the language. It may not
  say "the parser is implemented in Rust", "we use the `tree-sitter` crate", or
  "the type env is a `HashMap`". An author re-implementing neoc in OCaml or Zig
  must satisfy the spec without reading a line of our Rust.
- **`ext` / `extern "<host>"` is the only seam.** Host runtimes declare every
  API they expose to neoc using `ext` (`ext fn`, `ext struct`, …) in `.neoc`
  source. No magic globals, no compiler-baked-in functions, no implicit prelude
  that isn't itself `.neoc`. If it's callable from a `.neoc` file, it has an
  `ext` declaration somewhere — visible, type-checkable, greppable. A runtime
  that smuggles a function in without an `ext` is broken.
- **Only the *form* of `ext` is the language's business.** The *contents* —
  which bindings exist, their signatures — belong to whichever runtime embeds
  the compiler, and are not visible in the language spec or language docs.
- **Tooling output that crosses the boundary gets translated.** When a parser
  error originates in `tree-sitter` or a `thiserror` variant fires, the
  LSP/CLI maps it to a neoc-shaped message first. Raw host errors are a leak — a
  bug.

Rule of thumb for any user-visible string: **would this still make sense if the
next major version of neoc were written in OCaml?** If no, rewrite it.

Rustdoc, internal panics, log lines behind `RUST_LOG`, and crate-internal error
variants are exempt — those are for us, not for neoc users.

---

## Spec first, stages in sync

The language is defined in `lang/` and the spec wins over the impl:

| File | Layer |
| --- | --- |
| `lang/RATIONALE.md` | why neoc exists; the choices everything refers back to |
| `lang/NOTATION.md` | the meta-notations (EBNF, inference rules, lowering brackets) |
| `lang/LEXICAL.md` | tokens, keywords, literals, comments |
| `lang/SYNTAX.md` | EBNF grammar |
| `lang/STATIC.md` | name resolution, types, well-formedness (incl. `ext` forms) |
| `lang/DYNAMIC.md` | lowering table — neoc construct → emitted Lua 5.1 |
| `lang/CONFORMANCE/` | executable `.neoc` + expected `.luau` pairs |

Every compiler crate's tests cite a spec section. Behaviour not in the spec is
undefined and may not be relied on by the conformance suite. When impl and spec
disagree, **the spec wins** — fix the impl.

**Language changes move through every stage together.** A new construct touches
grammar → AST → parser → types → codegen → conformance, and a feature isn't
"done" until each stage is updated and asserts at its own level (CST tests, AST
tests, static-diagnostic tests, emit goldens, end-to-end run, conformance pair).
Don't land a half-threaded feature — a parser that produces a node no emitter
handles, or an emitter for a node the type-checker never sees, is a desync bug.

---

## Crate isolation

The workspace splits responsibilities across crates; boundaries are enforced by
`Cargo.toml`, not convention:

- Every compiler crate (everything except `neoc-vm` and the `neoc` bin) carries
  **no host-language assumptions** — no `std::fs`, `std::net`, `std::process`,
  `tokio`, `mlua`, `hyper`, `sqlx`. The runtime host deps live in `neoc-vm`; I/O
  lives there, in the `neoc` bin, and (over stdio) in `neoc-lsp`.
- The pure leaf crates (`neoc-ast`, `neoc-macros`, `neoc-codegen-lua`,
  `neoc-codegen-luau`) compile for `wasm32-unknown-unknown`; CI gates them with
  `cargo check --target wasm32-unknown-unknown -p neoc-ast -p neoc-macros -p neoc-codegen-lua -p neoc-codegen-luau`.
  The parser graph links the tree-sitter C runtime, which doesn't cross-compile
  to wasm32 yet — a tracked limitation, so the gate guards the leaf crates
  where host-dependency creep would start.
- `neoc-compiler/src/lib.rs` is the public API surface. Anything not re-exported
  there is implementation detail and may change without a version bump. The
  runtime embeds this crate.

Don't reach across crate boundaries to "shortcut" something. If a need crosses
cleanly, lift it into the right crate's API. If it doesn't cross cleanly, the
design is wrong — flag it.

---

## Build and test

```sh
cargo build                                          # debug build
cargo build --release                                # release build
cargo test --workspace                               # full suite (runtime + compiler)
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.luau'  # Luau integration tests
cargo run -p neoc -- emit-headers --out crates/neoc-vm/src/lua  # regenerate ext headers
cargo bench --bench lib                              # Criterion benchmarks (lib modules)
cargo bench --bench vnd                              # Criterion benchmarks (vnd modules)
cargo check --target wasm32-unknown-unknown -p neoc-ast -p neoc-macros -p neoc-codegen-lua -p neoc-codegen-luau  # wasm leaf gate
cargo clippy --workspace --all-targets               # lint
cargo fmt --check                                    # formatting check
```

---

## Project structure

```
crates/neoc/                 # the thin `neoc` binary (the only bin)
  src/
    main.rs                  # CLI entry (run/emit-headers + compiler subcommands)
    cli.rs                   # folded compiler subcommand tree (build/check/fmt/lsp)
  tests/                     # Luau integration tests (*.test.luau, *.spec.luau) + cli.rs

crates/neoc-vm/              # the Luau runtime library
  src/
    lib.rs                   # library root (pub mod dto, errors, lua)
    errors.rs                # runtime error types (NeocError)
    lua/
      sandbox.rs             # engine construction
      modules.rs             # module registration + headers()
      std/                   # std:* modules (collections, fs, io, net, thread, …)
      lib/                   # lib:* modules (base64, json, test, vcr, …)
      vnd/                   # vnd:* modules (hyper, serde_json, rusqlite, sqlx_*, …)
  benches/                   # Criterion suites (lib/, vnd/)

neoc-grammar                 # vendors tree-sitter-neoc (submodule); cc-builds parser.c
neoc-ast                     # typed AST — pure data, span-free, PartialEq
neoc-parser                  # CST → AST adapter; parse diagnostics
neoc-types                   # declaration collection, inference, well-formedness, elaborate
neoc-macros                  # built-in derives (Clone/Eq/Debug/Display)
neoc-codegen-lua             # AST → Lua 5.1; one emit module per construct family
neoc-codegen-luau            # AST → self-typed Luau (inlined runtime, inline types)
neoc-compiler                # public lib API: transpile to Luau; merges per-stage diagnostics
neoc-autosql                 # SQL schema + queries → typed .neoc data layer (neoc autosql)
neoc-fmt                     # formatter
neoc-lsp                     # language server (diagnostics over stdio)
neoc-wasm                    # browser entry point
```

The dependency graph is acyclic; the AST is the frozen contract every compiler
stage shares. The grammar is the second repo, `tree-sitter-neoc`, consumed here
as the `neoc-grammar` submodule.

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
5. Add `crates/neoc/tests/<ns>/<name>.test.luau`.

---

## Compiler: code style (Rust specifics)

- **Edition 2024.** `unsafe_code` is denied workspace-wide; only `neoc-grammar`
  opts in (one documented FFI binding). If you genuinely need `unsafe`
  elsewhere, lift it into its own clearly named crate and explain why.
- **Lints.** `clippy::all` + `clippy::pedantic`. CI fails on warnings
  (`RUSTFLAGS: -D warnings`). Use `#[allow(...)]` with a comment when you must.
- **Errors.** `thiserror` for library error enums; `anyhow` only inside the CLI
  bin. Never `unwrap()` / `expect()` in library code — return `Result`.
- **Naming.**
  - `walk_<construct>` — CST → AST, only in `neoc-parser`.
  - `emit_<construct>` — AST → Lua, only in the codegen crates. Pure function.
  - `lower_<form>` — rewrites one construct in terms of another (e.g.
    `lower_match` → IIFE).
  - `infer_*` / `check_*` — typing, only in `neoc-types`.
  - `derive` — Lua for a built-in trait, only in `neoc-macros`.
- **Tests.** Live next to the code as `#[cfg(test)] mod tests`. Integration
  tests in `tests/` only when they truly cross crate boundaries. Every feature
  has a corresponding entry in `lang/CONFORMANCE/`.
- **Pure functions over methods.** Emitters and lowerings take `&Node` and
  return `String` or a new tree. Don't mutate inputs.

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
| Commands | Exact: `neoc run`, `neoc build`, `neoc check`, `neoc lsp`, `cargo check --target wasm32-unknown-unknown`. (That `--target` is a Rust build target — *not* `neoc build --target`, which selects the output language: `luau`.) |
| File paths, env vars | Exact. |
| Rustdoc (`///`) on public items | Full, normal English — see below. |
| Legal, LICENSE, security notes | Complete and careful. |

Rule of thumb: **trim filler, never trim fact.**

### Docs enforcement

Two doc layers, different rules.

**1. Prose docs** (guides, README body, tutorials, spec prose) → terse. Short
sentences, lead with the point, no ceremony.

**2. Rustdoc on public items** (`///` on anything `pub`) → full, normal English.
`cargo doc` renders these for embedders and the docs site; they must be clear,
complete, and grammatical. Clipped phrasing in a `# Arguments` block is bad DX.

```rust
/// Compile a neoc source string to Lua 5.1.
///
/// # Arguments
///
/// * `src` — the `.neoc` source text. Caller owns the buffer; the compiler
///   does not retain a reference once this function returns.
/// * `env` — the type environment seeded with whatever `ext` declarations
///   should be in scope. Embedders typically build this once at startup.
///
/// # Errors
///
/// Returns `Error::Parse` for syntactic problems, `Error::Type` for static
/// errors, and `Error::Codegen` for emit-time failures (rare; usually a bug).
pub fn transpile(src: &str, env: &TypeEnv) -> Result<CompileOutput, Error> { /* ... */ }
```

Prose around a code sample stays terse. The doc comment itself stays complete.

---

## Voice

Direct. No marketing language. Friendly error messages that help the user, never
scold. No rabbit / spring / Bun / Easter motifs — that brand belonged to an
earlier life of the project when it targeted TypeScript on Bun. neoc now targets
scripting runtimes (Lua first); the voice is plain.

---

## Commits & PRs

Use [Conventional Commits](https://www.conventionalcommits.org). Terse, but
spec-compliant. release-plz reads these to compute the next version and generate
the changelog — wrong format means wrong (or missing) releases.

```
fix(codegen-lua): trait defaults emit a clean Lua function signature

Previously inlined the trait's source signature verbatim, producing
invalid Lua. Now goes through the same param-name extractor inherent
impl methods use.

Closes #42.
```

- **Type** — one of: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`,
  `test`, `build`, `ci`, `chore`, `revert`.
- **Scope** — optional. Compiler crates: use the name without the `neoc-`
  prefix (`parser`, `ast`, `types`, `codegen-lua`, `macros`, `compiler`, `fmt`,
  `lsp`, `wasm`, `grammar`, `cli`). Runtime: use the module (`std:fs`) or
  subsystem (`install`). Cross-cutting: `workspace`, `ci`, `spec`.
- **Description** — imperative, lowercase, no trailing period.
- **Body** — optional, terse prose. Bullet what changed. Use it when the *why*
  isn't obvious from the subject.
- **Footer** — `Closes #42`, `Refs #7`, etc.
- **Breaking changes** — append `!` after type/scope (`feat(compiler)!:`) **and**
  add a `BREAKING CHANGE:` footer with full, careful English. The library API in
  `neoc-compiler/src/lib.rs` is the most user-visible surface — changes there
  almost always warrant `!`.
- One logical change per commit. If you're tempted to write "and" in the
  subject, split it.

PR titles follow the same format — the squash-merge commit inherits them, so
they feed the changelog.

---

## Issue workflow — GitLab board

Issue lifecycle is managed through the [GitLab board](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/boards/3)
using scoped `workflow::*` labels. Do not use `draft` labels or `DRAFT:` title
prefixes.

**Columns:** Backlog → Refinement → Design → Implementation → Review → Done

Each column maps to a scoped label. Labels are mutually exclusive within the
`workflow::` scope — applying a new stage label replaces the old one.

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
| Refinement → Design | Flesh out the ticket. Wait for CODEOWNER signal to advance. |
| Design → Implementation | If a design MR is needed, open one on a `design/{id}-{slug}` branch — CODEOWNER approves. If the description is sufficient design, move straight to Implementation. |
| Implementation → Review | Open the implementation MR from an `issues/{id}-{slug}` branch. CODEOWNER reviews. Label stays `workflow::Review` until approved. |
| Review → Done | CODEOWNER approves, merge to `main`, close the issue. |

**Branching model:**

- Implementation work: `issues/{id}-{slug}` (e.g. `issues/6-std-task`).
- Design artefacts: `design/{id}-{slug}` (e.g. `design/23-std-env`).

**Hard rules:**
- The `workflow::*` label is the source of truth for issue state — not the
  issue's open/closed status alone.
- Never move an issue out of `workflow::Backlog` without explicit CODEOWNER
  instruction.
- CODEOWNER approval is required at the Review gate before merge (see CODEOWNERS).
- The GitLab project is hosted at `gitlab.beluga-sirius.ts.net/flying-dice/neoc`.

---

## Cross-platform

The binary supports Linux, macOS, and Windows. Signal handling uses
`#[cfg(unix)]` / `#[cfg(not(unix))]` conditionally. `mlua` is built `vendored`
(bundles Luau). `rusqlite` uses `bundled` (bundles SQLite).

---

*Few words. Full meaning.*
