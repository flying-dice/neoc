---
status: approved
created: 2026-05-17
implemented-in:
---

# 0008 — Errors guide: helpful errors that educate, not punish

> [!NOTE]
> This RFC is `approved`. All seekers reviewed and approved. Awaiting CODEOWNER merge.

## Summary

Establish neoc's error diagnostic strategy: enriched error messages with "did you mean" suggestions, available-option enumeration, and structured CLI rendering. Extends the existing `(nil, string)` / `error()` convention (ADR-0005) without breaking it. Introduces `strsim` for edit-distance matching and `miette` + `thiserror` for CLI-facing diagnostics.

## Motivation

neoc errors are currently bare format strings: `"require: unknown module \"lib:jsn\""`. No suggestions, no error codes, no source context. Every mature runtime — Rust, Node, Elm, Kotlin/Roslyn, Clang — converges on the same pattern: machine-readable identity + human-readable narrative + actionable suggestions. neoc has none of these.

Concrete scenarios that are broken today:

1. **Module typo** — `require("lib:jsn")` produces a generic message. The runtime knows the answer (`lib:json`) but doesn't say it.
2. **Type mismatch** — `"expected SQL string, got table"` doesn't list what types are accepted.
3. **CLI errors** — `eprintln!("{e}")` with no source context, no error codes, no help text.
4. **LLM consumers** — error messages are not machine-parseable. No `candidates` field for automated correction.

The module registry is 25 entries. Levenshtein against that set is sub-microsecond. There is no performance excuse for not suggesting.

## Detailed design

### Layered approach

Three layers, shipped independently. Each layer is independently valuable. Layer 3 is deferred to a separate RFC.

#### Layer 1 — inline suggestions (no breaking change)

Enrich existing error strings with suggestions computed on the error path. Zero happy-path cost.

**Dependency:** `strsim = "0.11"` (~12KB)

**Algorithm:** Levenshtein with threshold `max(1, name.len() / 3)` — Rust's proven threshold. Avoids garbage suggestions on short names and over-strictness on long names. Tie-breaking: when multiple candidates share the lowest edit distance, return the lexicographically first. Deterministic output, no user confusion.

**Gating rules:**

- Suggest only when edit distance is within threshold.
- Never suggest when the suggestion equals the user's input.
- Enumerate available options when candidate set ≤ 10 items.

**Insertion points:**

| Error path | File | Current message | Enriched message |
|---|---|---|---|
| `require()` module not found | `modules.rs:79-84` | `require: unknown module "lib:jsn" — expected std:<name>, lib:<name>, or vnd:<name>` | `require: unknown module "lib:jsn" — did you mean "lib:json"? available in lib: base64, compile, http, ini, json, mime, test, vcr` |
| Userdata type mismatch | various (`serde_json.rs`, `rusqlite.rs`, `hyper.rs`, `sqlx_*.rs`) | `"expected SQL string, got table"` | `"expected SQL string, got table — accepted types: string, Statement"` |

**`require()` implementation:**

The `require` closure in `modules.rs:79-84` has access to the full `HashMap<String, Table>` keys. On miss:

1. Check for `:` in the requested name. If present, split into `(namespace, local_name)`. If absent, `namespace = None`.
2. If namespace present — filter candidates to that namespace, Levenshtein match the module name portion, enumerate if ≤ 10.
3. If no namespace — search across all namespaces, Levenshtein match against all registered module names, enumerate if ≤ 10.

```rust
// Pseudocode — in the require closure's error path
let requested = &name;
let keys: Vec<&str> = modules.keys().map(|k| k.as_str()).collect();
let suggestion = find_closest(requested, &keys, max(1, requested.len() / 3));

let (ns, _local_name) = if let Some(pos) = requested.find(':') {
    (Some(&requested[..pos]), &requested[pos + 1..])
} else {
    (None, requested.as_str())
};

let namespace_modules: Vec<&str> = if let Some(ns) = ns {
    keys.iter()
        .filter(|k| k.starts_with(&format!("{ns}:")))
        .copied()
        .collect()
} else {
    // No namespace prefix — search across all namespaces
    keys.to_vec()
};

let mut msg = format!("require: unknown module {requested:?}");
if let Some(s) = suggestion {
    msg.push_str(&format!(" — did you mean {s:?}?"));
}
if !namespace_modules.is_empty() && namespace_modules.len() <= 10 {
    msg.push_str(&format!(" available: {}", namespace_modules.join(", ")));
}
```

**Type mismatch enrichment pattern:**

Where a function checks `Value::type_name()`, also list accepted types in the error message. Example in `rusqlite.rs`:

```rust
// Before
format!("expected SQL string, got {}", other.type_name())

// After
format!("expected SQL string, got {} — accepted: string", other.type_name())
```

No Levenshtein needed — these are fixed lists per function.

#### Layer 2 — miette for CLI diagnostics

Structured error rendering for errors that surface through the CLI (not through Lua `(nil, string)` tuples).

**Dependencies:**

```toml
thiserror = "2"
miette = { version = "7", features = ["fancy"] }
```

**Error types:**

Define a `NeocError` enum covering all CLI-facing error paths:

```rust
use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum NeocError {
    #[error("unknown module {name:?}")]
    #[diagnostic(
        code(neoc::E0001),
        url("https://neoc.dev/errors/E0001")
    )]
    ModuleNotFound {
        name: String,
        #[help]
        suggestion: Option<String>,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("module required here")]
        span: miette::SourceSpan,
    },

    #[error("embedded payload corrupt")]
    #[diagnostic(code(neoc::E0002))]
    CorruptPayload {
        #[help]
        detail: String,
    },

    #[error("engine initialisation failed")]
    #[diagnostic(code(neoc::E0003))]
    EngineInit(#[from] mlua::Error),
}
```

**Rendering:** Replace `eprintln!("{e}")` in `main.rs` with `miette::set_hook` + `eprintln!("{:?}", miette::Report::new(e))`. Produces:

```
  × unknown module "lib:jsn"
  ╰─▶ did you mean "lib:json"?

  main.lua:3:15
  │
3 │ local json = require("lib:jsn")
  │              ─────────────────── module required here
  │
  help: available in lib: base64, compile, http, ini, json, mime, test, vcr

  neoc::E0001 — https://neoc.dev/errors/E0001
```

**Error code registry:**

| Code | Error | Layer |
|---|---|---|
| `neoc::E0001` | Module not found | 1+2 |
| `neoc::E0002` | Corrupt embedded payload | 2 |
| `neoc::E0003` | Engine init failure | 2 |

Flat namespace `neoc::ENNNN` — sequential. Stable across versions. Each code links to a docs page.

**Source retention and span extraction:**

neoc already reads the full script source into a `String` in `main.rs` before passing it to `lua.load()`. Layer 2 retains this source in an `Arc<String>` stored in Lua app data (`lua.set_app_data(source.clone())`). When a `require()` error occurs:

1. **Source text** — retrieved from `lua.app_data_ref::<Arc<String>>()`. Wrapped in `miette::NamedSource::new(script_path, source)`.
2. **Span extraction** — mlua's `Error::CallbackError` includes a traceback string with `"chunk_name:line:"` prefixes. Parse the line number from the traceback, then compute the byte offset into the retained source by iterating lines: `source.lines().take(line - 1).map(|l| l.len() + 1).sum()`. The span length is the length of the `require("...")` call on that line, found by searching for the `require` token at the byte offset.
3. **Multi-file limitation** — neoc currently executes a single entry script (glob-matched). `Arc<String>` holds only the entry source. `dofile()`/`loadfile()` calls within the script do not have their source retained. Layer 2 annotated output degrades gracefully to error-code + help text without source context for those paths. Full multi-file source retention is a future enhancement if needed.

This approach has zero happy-path cost — `Arc::clone` is a refcount bump, and span extraction only runs on the error path.

**Scope boundary:** miette renders errors that reach `main.rs`. It does NOT change the Lua-facing `(nil, string)` convention. The two paths are independent:

```
Lua function error → (nil, "module.fn: message")     ← Layer 1 enriches this string
mlua::Error raised → eprintln → process::exit(1)     ← Layer 2 renders this with miette
```

#### Layer 3 — structured error tables (deferred)

Change `err` from string to table: `{ code = "E0001", message = "...", hint = "...", candidates = {...} }`.

**NOT proposed in this RFC.** Requires ADR-0005 amendment. Deferred until:

- Error code taxonomy stabilises from Layer 1+2 usage.
- LLM consumer use cases are proven (currently theoretical).
- Migration path is clear (`if err then` works with tables, but string matching breaks).

Separate RFC will be filed when Layer 1+2 ship and the need is concrete.

### Error message style guide

Codified as an extension to the existing error-conventions guide (`docs/src/guides/error-conventions.md`):

**Structure:**

```
module.function: what went wrong[ — did you mean "X"?][ available: a, b, c]
```

**Rules:**

1. **Prefix** — `"module.function:"` per ADR-0005. Stable, matchable.
2. **Message** — what went wrong. Lowercase, no trailing period.
3. **Suggestion** (optional) — `" — did you mean \"X\"?"`. Present only when Levenshtein distance ≤ `max(1, name.len() / 3)`. Never echoes the user's input.
4. **Enumeration** (optional) — `" available: a, b, c"`. Present only when candidate set ≤ 10 items.
5. **Type hints** (optional) — `" — accepted: string, Statement"`. For type mismatch errors, list accepted types.

**`help` vs `note` distinction (Layer 2 only):**

- `help` — actionable change the user can make. "did you mean X?", "use std.conv.parse_int(value)".
- `note` — explanatory context. "searched paths: std, lib, vnd".

### Performance contract

All suggestion computation lives in the error path. Zero cost on the happy path.

- Levenshtein on 25 modules × avg 10 chars = O(250 × L²) ≈ sub-microsecond.
- Module key collection: `HashMap::keys()` — no allocation beyond the iterator.
- Candidate enumeration: string formatting — error path only.

No concern until the module set exceeds ~1000. If that happens, switch to BK-tree.

### Docs pages

Each error code gets a page at `docs/src/errors/ENNNN.md`:

```markdown
# E0001 — Module not found

## What it means

The `require()` call references a module name that doesn't exist in neoc's registry.

## Common causes

- Typo in module name (`lib:jsn` instead of `lib:json`)
- Wrong namespace (`std:json` instead of `lib:json`)
- Module not available in this neoc version

## How to fix

Check the module name against the [module reference](/reference/modules).
```

Auto-generation from `#[diagnostic]` attributes is a future optimisation. Hand-write initially — error pages need examples and context that derives can't provide.

### Acceptance criteria

- [ ] `strsim` added to `Cargo.toml`
- [ ] `require()` errors include Levenshtein suggestion + namespace enumeration
- [ ] Type mismatch errors in ≥ 3 modules list accepted types
- [ ] `thiserror` + `miette` added to `Cargo.toml`
- [ ] `NeocError` enum defined with `#[diagnostic]` attributes
- [ ] `main.rs` error output uses miette rendering
- [ ] Error-conventions guide updated with suggestion style rules
- [ ] ≥ 1 error code page in `docs/src/errors/`
- [ ] No performance regression on happy path (verified by existing benchmarks)
- [ ] All existing tests pass unchanged

## Drawbacks

- **Two new dependencies** — `strsim` (~12KB), `miette` with `fancy` feature (~80KB) + transitive deps (`owo-colors`, `unicode-width`, `supports-color`, `textwrap`) (~70-100KB) + `thiserror` (minimal). Total binary impact ~150-200KB.
- **Error message changes break snapshot tests** — any test asserting exact error strings needs updating. Existing tests use prefix matching (`"env.var: "` prefix), so impact is limited.
- **miette only covers CLI path** — Lua-facing errors remain strings. Two rendering paths to maintain.
- **Error code commitment** — once assigned, codes must remain stable. Wrong taxonomy early is expensive to fix.

## Alternatives

### Do nothing

Current errors work. But "works" and "helps" are different. Every runtime in the survey enriches errors. Doing nothing is a permanent DX tax on every neoc user.

### miette only (skip strsim)

miette renders errors beautifully but doesn't compute suggestions. Without Levenshtein, "did you mean" requires hand-coded suggestion maps. `strsim` is 12KB and solves this generically.

### ariadne instead of miette

ariadne produces slightly prettier output. But miette has `#[derive(Diagnostic)]` — near-zero boilerplate. ariadne requires manual builder calls. miette is the ecosystem standard (cargo, turbopack). Follow the ecosystem.

### Structured error tables now (Layer 3 immediately)

Tempting but premature. The error code taxonomy doesn't exist yet. Breaking ADR-0005 before proving the value with Layer 1+2 is unnecessary risk. Ship the non-breaking layers first.

### Feature-gated miette

`features = ["diagnostics"]` would keep miette optional. Unnecessary — every neoc user benefits from better errors. No use case for stripping diagnostics. The 50KB is justified.

## Open questions

1. ~~**Error code taxonomy**~~ — **Resolved: flat `neoc::E0001`**. Matches rustc convention, simpler, sequential. Subsystem is derivable from the error type — namespacing the code adds no information.
2. **Docs hosting for error pages** — inline in VitePress site (proposed) vs auto-generated from code. Hand-written initially; automation later if error count grows past ~20.
3. **Layer 3 timing** — when do structured error tables become necessary? Current proposal defers until LLM consumer demand is concrete.
4. **`miette` in test output** — should `lib:test` assertion failures also use miette rendering? Currently out of scope but natural extension.

## Implementation notes

- **Layer 1 files touched:** `Cargo.toml` (add `strsim`), `src/lua/modules.rs` (require error enrichment), individual module files for type hint enrichment.
- **Layer 2 files touched:** `Cargo.toml` (add `thiserror`, `miette`), new `src/errors.rs` (NeocError enum), `src/main.rs` (miette rendering).
- **Docs:** `docs/src/guides/error-conventions.md` (extend with suggestion rules), new `docs/src/errors/` directory.
- **Sequencing:** Layer 1 ships first (single commit, `fix` type). Layer 2 follows (separate MR, `feat` type). Independent of any other in-flight work.
- **Testing:** Add `tests/require-errors.test.luau` covering typo suggestions. Existing tests unaffected — they test happy paths.
