# Plan — Populate `neocc` with the language assets

Scope: bring this repo from bootstrap to "ready for crate build-out" by importing every language-defining asset, writing a proper reference manual, and standing up a stub workspace skeleton CI passes against. The Rust impl is a separate plan that starts after this one finishes.

**Posture toward `../bun-oas/`.** `bun-oas` is one input among several, not the spec. Take what's correct or canonical; fix or skip what isn't. The references for *how* to define a language are [The Rust Reference](https://doc.rust-lang.org/reference/) and the [Lua 5.4 Reference Manual](https://www.lua.org/manual/5.4/) — those define the bar. If a `bun-oas` artefact already meets that bar (e.g. `grammar.js` is a perfectly serviceable syntactic definition), use it as-is rather than re-deriving it. If an artefact is non-canonical (per-feature `.md` files that don't fit a layered reference manual, examples that violate the Rust-shaped design, grammar quirks), redo it the proper way.

## State today

Bootstrap is complete:

- `Cargo.toml` virtual workspace, `.gitlab-ci.yml`, `CLAUDE.md` (golden rule + `ext`-only-seam corollary), `README.md`, `rust-toolchain.toml`, `.gitignore`, `.claude/skills/language-review/`.

What's missing: grammar, examples, reference manual, the workspace skeleton crates.

## Available inputs

| Source | Useful for | Trust level |
| --- | --- | --- |
| `bun-oas/zed/tree-sitter-neoc/grammar.js` (+ generated artefacts) | starting point for `crates/neocc-grammar/` and the syntactic spec | high — but revise productions that read non-canonically |
| `bun-oas/zed/tree-sitter-neoc/test/corpus/*.txt` | grammar regression tests, conformance seeds | high |
| `bun-oas/examples/*` | example fixtures, conformance seeds | medium — drop anything that doesn't pass the language-review skill |
| `bun-oas/specs/<feature>.md`, `ide-<feature>.md` | source material for the reference manual | low — wrong organisation (per-feature, not per-layer). Read for facts, rewrite for shape. |
| `bun-oas/src/neoc/types/`, `codegen/lua/` | source material for `lang/STATIC.md` and `lang/DYNAMIC.md` | medium — reverse-engineer the rules, don't transcribe TS |
| `bun-oas/BRAND.md` | identity | high |
| The Rust Reference, Lua 5.4 Reference Manual | shape of a real reference manual | canonical |

What does **not** come across at all: TS source (`src/`), `package.json`, `bun.lock`, `biome.json`, `tsconfig.json`, `node_modules/`, TS test files, `ARCHITECTURE.md` (TS-impl specific).

---

## Port order

Four PRs, each independently reviewable and CI-green. Editor extensions (Zed, IntelliJ) live outside this repo and aren't ported.

### PR 1 — Grammar

Copy `grammar.js` + generated artefacts into `crates/neocc-grammar/`. No Cargo crate yet — `build.rs` and Rust bindings come later in the impl plan. This PR is pure file movement so the grammar lives in its final home.

**Adds:**

```
crates/neocc-grammar/
├── grammar.js
├── src/{parser.c, grammar.json, node-types.json, tree_sitter/}
├── queries/highlights.scm
├── test/corpus/*.txt
└── README.md         ← "generated artefacts; regen with tree-sitter generate"
```

**Posture:** copy as-is. If a production reads non-canonically when reviewed later (during PR 4 spec work or once the parser crate is built), revise it then with a follow-up PR — don't preemptively rewrite the grammar here.

**Acceptance:** `tree-sitter generate` against the copied `grammar.js` regenerates `parser.c` byte-identical to the committed file. CI unchanged.

### PR 2 — Examples

Copy `.neoc` / `.lua` pairs into `examples/`. Drop everything else.

**Adds:**

```
examples/
├── showcase.{neoc,lua}
├── features/{01-struct..20-loops}.{neoc,lua}
└── custom-macros/{app.neoc, app.lua, README.md}
```

**Drops:** `*.test.ts`, `run-tests.lua`, `__snapshots__/`, `runtime.test.ts`, `custom-macros/macros.ts` (user-macro story is open per the master plan — default v1 is built-ins only).

**Posture:** each example must pass the language-review skill. If an example uses an idiom that the skill would flag (Lua-isms, TS-isms, unjustified Rust divergences), either rewrite the `.neoc` to be Rust-shaped (regenerating its `.lua`) or drop the example. Don't preserve examples just because they exist.

**Acceptance:** every `.neoc` has a matching `.lua`. Every `.neoc` would pass language-review with no red flags.

### PR 3 — Reference manual

Fill in `lang/`, modelled on the Rust Reference and the Lua Reference Manual. The directory skeleton already exists (`lang/{RATIONALE,NOTATION,LEXICAL,SYNTAX,STATIC,DYNAMIC}.md`, `lang/CONFORMANCE/`); this PR authors the content. Use `bun-oas/specs/`, `grammar.js`, `bun-oas/src/neoc/types/`, and `bun-oas/src/neoc/codegen/lua/` as **inputs**; don't transcribe them.

**Layout (already committed as skeleton):**

```
lang/
├── RATIONALE.md       ← done
├── NOTATION.md        ← done (EBNF dialect, inference rules, lowering brackets)
├── LEXICAL.md         ← tokens, keywords, literals, comments
├── SYNTAX.md          ← canonical EBNF grammar
├── STATIC.md          ← name resolution, type system, well-formedness
├── DYNAMIC.md         ← lowering table — neoc construct → emitted Lua 5.1
└── CONFORMANCE/       ← .neoc + expected output, organised by layer
    ├── lexical/
    ├── syntax/
    ├── static/
    └── dynamic/
```

**Posture:**
- **If something in `bun-oas` already covers a spec layer adequately, use it.** Example: if `grammar.js` is already a sufficient syntactic definition, `SYNTAX.md` can lift its EBNF rather than re-derive a parallel grammar that drifts. Apply the same logic anywhere a bun-oas artefact already meets the bar.
- **Per-feature spec files (`bun-oas/specs/<feature>.md`) are not the right organisation** for a reference manual. Their content gets folded into the appropriate layer file. The per-feature files themselves aren't copied across.
- **IDE-only behaviour (`bun-oas/specs/ide-<feature>.md`) is not language spec.** Hover, completion, signature help are LSP concerns; they belong in the eventual `neocc-lsp` crate docs, not here. Skip these files entirely.
- **Per the CLAUDE.md golden rule:** no mention of Rust, TypeScript, tree-sitter, or the crate layout in the spec. The spec defines the language; the impl is implementation detail.
- **Per the language-review skill:** every spec section must hold up against the Rust-shape test. If `bun-oas` semantics drift from Rust without a "no lifetimes / no GC" justification, fix the spec, not preserve the drift.

Sub-divide into per-layer PRs (`LEXICAL`, then `SYNTAX`, then `STATIC`, then `DYNAMIC`, then `CONFORMANCE`) — the single PR is almost certainly too large to review otherwise.

**Acceptance:**
- Every `> *To be authored.*` marker in `lang/` is replaced with real content.
- Every construct in `examples/` has a corresponding entry in `lang/CONFORMANCE/dynamic/`.
- A run of the language-review skill returns a clean synchronisation matrix for rungs 11–15.

### PR 4 — Workspace skeleton + ARCHITECTURE.md + BRAND.md

Stand up stub crates so the dependency graph is committed and `cargo build` / `cargo test` / wasm-check / clippy / fmt all pass. Write `ARCHITECTURE.md` for the Rust crate layout. Copy `BRAND.md` verbatim.

**Adds:**

```
crates/
├── neocc/src/main.rs
├── neocc-wasm/src/lib.rs
├── neocc-grammar/src/lib.rs        ← pub fn language() -> tree_sitter::Language { unimplemented!() }
├── neocc-ast/src/lib.rs
├── neocc-parser/src/lib.rs
├── neocc-types/src/lib.rs
├── neocc-macros/src/lib.rs
├── neocc-codegen-lua/src/lib.rs
├── neocc-compiler/src/lib.rs       ← public API surface declared with todo!() bodies
├── neocc-fmt/src/lib.rs
└── neocc-lsp/src/lib.rs

ARCHITECTURE.md
BRAND.md
```

Workspace `Cargo.toml`: `members = ["crates/*"]`. Per-crate `Cargo.toml` inherits `[workspace.package]` and declares deps per the master plan's graph.

**Acceptance:** `cargo build --workspace`, `cargo test --workspace`, `cargo check --target wasm32-unknown-unknown --workspace --exclude neocc`, `cargo clippy --workspace --all-targets`, `cargo fmt --check` all green. `ARCHITECTURE.md`'s dep graph matches `cargo tree`.

---

## Out of scope

Everything below belongs to the master plan (`../bun-oas/plan.md`), not here:

- Implementing any stub crate (parser adapter, type inferencer, codegen, LSP, …).
- Anything in editor-extension repos (Zed, IntelliJ) — those live in their own repos and consume `neocc-grammar` / `neocc lsp` once those exist.
- Publishing `neocc` to crates.io or `neocc-wasm` to npm.
- The companion PR in `../neoc/` that adds the `neocc` dependency.
- Archiving `bun-oas`.

---

## Build status (pipeline)

The full **TreeSitter → AST → Type-check → Elaborate → Emit** pipeline is live
and tested end to end. Per-stage assertions exist at every boundary so an
emitter bug cannot hide behind a green end-to-end run. **93 tests green**;
clippy + fmt clean.

**Done — every crate**
- `neocc-grammar` — tree-sitter grammar for the whole surface; `cc` build; CST tests.
- `neocc-ast` — full typed AST (span-free, `PartialEq`).
- `neocc-parser` — modular CST→AST adapter (every construct); AST-stage tests.
- `neocc-types` — declaration collection + inference + checks (unknown names,
  type mismatches, struct-literal fields, `match` exhaustiveness, `?`
  well-formedness, trait-impl completeness) + `elaborate` (string `+` → `Concat`).
- `neocc-macros` — `Clone`/`Eq`/`Debug`/`Display` derives, structural over the AST.
- `neocc-codegen-lua` — emitter for the whole language: functions, structs
  (metatable dispatch), enums (tagged tables), `impl`, control flow, operators
  (incl. string concat via `..`), calls, fields, indexing, struct/enum
  construction, lists, closures, `match` + patterns, `?`, derive splicing.
- `neocc-compiler` — `transpile(src) -> CompileOutput` running parse → check →
  elaborate → emit, diagnostics tagged by stage.

**Test layers** (the "assert at every stage" requirement)
- CST: grammar parse tests. AST: `parse` → assert typed AST. Emit: hand-built-AST
  → Lua goldens (parser-independent) + source → Lua goldens. Static: `check`
  diagnostics on ill-typed programs. Elaborate: AST-rewrite tests. End-to-end:
  transpile → run under `lua` → assert stdout (fib, loops, enum match, derived
  clone/eq, string concat).

### Not implemented — intentional (deferred advanced Rust)

These are out of scope for now; the language is honest about not having them.
They are *not* bugs — they're features we chose not to build yet.

- **Generic trait-bound enforcement.** `<T: Trait>` parses and type-params are
  tracked, but a bound violation is not caught (type params are permissive).
- **Deep structural `Eq`.** Derived `Eq` compares fields with Lua `==`, so a
  nested derived type compares by reference, not recursively. Full structural
  equality across nested derives is deferred.
- **`async`/`await`, trait objects (`dyn`), const generics, lifetimes,
  ownership/borrowing.** Per the rationale, neoc deliberately omits these.

### Limitations to finish (not-done / broken, being completed)

- **`continue`, `let` destructuring, `..base` struct update, enum methods**
  (no metatable dispatch on enum values) — *in progress* (task #14).
- **Visibility / multi-file modules.** `pub` only affects local-vs-global Lua
  emission; cross-module `use`/visibility is not enforced because the checker is
  single-module. Needs the module system (task #15).
- **Attribute well-formedness** (`STATIC.md` §9) is partial.

### Tooling not yet built (needed for a usable product)

- **CLI** — the `neocc` binary is a stub; no `build`/`check`/`fmt`/`lsp`/`run`.
- **LSP** — `neocc-lsp` is a stub; no diagnostics/hover/completion.
- **Formatter** — `neocc-fmt` is a stub.
- **Browser/WASM build** — blocked: the tree-sitter C runtime doesn't
  cross-compile to wasm32 (needs cc-to-wasm or a pure-Rust parser). The wasm CI
  gate checks only the host-free leaf crates.

### Known limitations to revisit

- AST is span-free; diagnostics carry CST spans. Threading spans into the AST is
  deferred until the LSP needs precise ranges.
