# Conformance

The executable contract every neoc implementation must satisfy. Where `LEXICAL.md`, `SYNTAX.md`, `STATIC.md`, and `DYNAMIC.md` describe the language in prose and notation, this directory describes it in runnable cases. An implementation that passes every case here matches the spec; one that doesn't, doesn't.

## Layout

```
CONFORMANCE/
├── lexical/    ← tokenisation cases    (LEXICAL.md)
├── syntax/     ← parse / reject cases  (SYNTAX.md)
├── static/     ← typing / well-formedness cases  (STATIC.md)
└── dynamic/    ← lowering + execution cases  (DYNAMIC.md)
```

One sub-directory per spec layer. A test in `lexical/` exercises the lexer in isolation; one in `dynamic/` runs all four layers end-to-end.

## File conventions

| Layer | Inputs | Expected output |
| --- | --- | --- |
| `lexical/` | `name.neoc` | `name.tokens` — one token per line, format `KIND@line:col "lexeme"`. **Deferred** — see `lexical/README.md`; neoc has no flat tokeniser to assert against. |
| `syntax/`  | `name.neoc` (accept) | the source must parse against the tree-sitter grammar with no error node. A second form, `name.reject`, is a source the parser must reject (the tree contains an error or missing node); `name.reject.expected` names the error category in prose. |
| `static/`  | `name.neoc` | `name.diagnostics` — the set of static errors the checker must report, one per line (empty file = well-typed). The match is order-independent. |
| `dynamic/` | `name.neoc` | `name.lua` (the byte-for-byte lowering — readable, committed), `name.stdout` (the output the lowering must produce when run), and `name.rs` (the closest working Rust translation — see below). A rejection case ships `name.fails` instead of `name.rs`; its `name.stdout` holds what the run prints before the rejection. |

Pairs live side-by-side. The test runner glob is `*.neoc` and the expected files are derived by extension.

### Multi-module cases

A `dynamic/` case that needs more than one file is a **directory** named like a single-file case. Its entry is `main.neoc`; the other modules sit beside it at the paths their `use` declarations name (`use geo::shape::Circle` is `geo/shape.neoc`). The case compiles as a project — `use` and `extern` `mod` resolve exactly as in a real build — and every module has its own readable `.lua` golden. The entry exports `pub fn main`; the runner serves the non-entry modules to `require` and then calls it. The case's `main.stdout` and `main.rs` anchor live in the directory (the anchor is one file, with the other modules as inline `mod`s). A directory case cannot carry `#[test]` fns: each module's test table is local to its chunk, out of the runner's reach, so the runner rejects such a case — put tests in a single-file case.

```
dynamic/3-07-use-cross-module/
├── main.neoc  main.lua  main.rs  main.stdout
├── util.neoc  util.lua
└── geo/shape.neoc  geo/shape.lua
```

### Sad-path cases

A case that the runtime must **reject** carries `name.fails` (`main.fails` in a directory case) in place of `name.rs`. The file holds a non-blank fragment of the error the run must raise. The program must compile clean; the runner then asserts the run raises, the raised message (never the program text) contains the fragment, and the output printed before the raise equals `name.stdout` — so a rejection at the wrong point fails the case. The `.lua` golden still pins the emit. A rejection has no Rust analogue, so the anchor is neither required nor run.

## Naming

Each case file starts with the spec section it exercises, then a short descriptive slug:

```
dynamic/4-05-match-literal-arms.neoc   ← the source
dynamic/4-05-match-literal-arms.rs     ← closest Rust (design anchor)
dynamic/4-05-match-literal-arms.lua    ← emitted Lua 5.1 (LuaJIT)
dynamic/4-05-match-literal-arms.stdout ← the output both the Lua and the Rust anchor must produce
```

`4-05` means `DYNAMIC.md` §4.5. This keeps the conformance directory navigable in alphabetical order and lets a reviewer trace from a case back to the rule it tests.

## Readable emitted output

The `.lua` files are the **committed, readable lowering** of each case — exactly what `neoc build` (target `lua`) emits. Read them to see the transformation; regenerate them after a codegen change with:

```
NEOC_BLESS=1 cargo test -p neoc-compiler --test conformance
```

## Dynamic Rust anchors

Each dynamic case also ships a `name.rs`: the closest working Rust translation. neoc is "Rust without lifetimes," so the anchor mirrors the case — same structs, enums, impls, `match` arms — and prints exactly `name.stdout`.

The single data-driven runner `crates/neoc-compiler/tests/conformance.rs` pins the whole contract, one visible test per case × target:

- `<case>::<target>::source` — the live emit equals the committed `.lua` (the transformation is pinned; no toolchain needed).
- `<case>::<target>` — transpile (or compile the `.rs`) and run in the runtime's in-process LuaJIT sandbox / under `rustc`, asserting `name.stdout`.

So Lua == Rust output == golden `name.stdout`. The `.rs` is the design anchor: start a new dynamic case from a working `.rs`, then derive the neoc, the lowering, and the stdout from it.

## What a passing implementation looks like

A neoc implementation passes conformance when, for every `*.neoc` file under `CONFORMANCE/`:

1. The implementation produces the expected tokenisation, CST, diagnostics, lowering, or runtime output (as appropriate for the layer).
2. No additional warnings or diagnostics are produced beyond those listed in the expected output.
3. The exit status is zero (success) for cases that should succeed, non-zero for cases that should fail.

The neoc workspace ships one runner per active layer under `crates/neoc-compiler/tests/`: `conformance.rs` (dynamic), `conformance_static.rs` (static), and `conformance_syntax.rs` (syntax). Each uses `libtest-mimic` to emit one visible test per case. A different implementation in another host language is expected to bring its own runner; the conformance files themselves are language- and implementation-agnostic.

## Adding cases

When a new case is added:

- Cite the spec section it exercises in the filename prefix.
- The case must be minimal — exercise one rule, not five.
- The expected outputs must be hand-written (or hand-reviewed if generated), never copied from whatever the current implementation happens to produce. The spec leads; the impl follows.

## What conformance does not test

- Performance characteristics. The spec defines correctness, not throughput.
- Diagnostic wording. Two implementations may format the same error differently; conformance pins the error *kind* via `name.diagnostics`, not the exact prose.
- Source maps. Useful, but implementation-dependent — pinned in implementation tests, not here.
