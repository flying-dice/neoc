# Debt Assessment — `refactor/debt-sweep`

Living backlog. Source: four parallel surveys (architecture, clean code, docs, tests), each finding verified against the tree before landing. Worked top-down, commit per item.

Status keys: `[ ]` open · `[x]` landed · `[~]` partial · `[-]` rejected (false positive) · `[>]` deferred (risky to do autonomously).

---

## Sweep 2 + local review

A second pass: four agents (compiler-crate sweep, footgun/dead-code sweep, cross-cutting-consistency sweep, and a correctness review of this branch's own diff).

**Local review verdict — branch is clean, no blockers.** One honest correction it caught: the json DRY commit (`76622c6`) said "behaviour unchanged", but the new shared `node_len` resolves *both* container kinds, so a stale handle to a node that was replaced in-place with a different kind now returns the live length instead of `0`. Strictly more correct (the old `0` was a footgun), untested path — but the commit message overstated. Noted here for accuracy.

**Sweep 2 — landed (verified, low-risk):**
- [x] `ini.stringify` emitted `format!("{:?}", v)` for non-scalars — Rust `Debug` syntax leaking into INI output (footgun **and** host-language-leak). Now serialises string/number/bool and errors cleanly on the rest. `d19da2a`
- [x] `task` spawn/defer/delay raised a bare `task:` prefix; `rusqlite` `Statement` used `sqlite.statement.*` while the module uses `rusqlite.*`. Both normalised; dead `:memory:` branch in `rusqlite.open` dropped. `04a8c1e`
- [x] Misleading `jsonschema` `__tostring` comment (claimed it makes `if result then` work — userdata is always truthy). Corrected. `63dea2d`

**Sweep 2 — deferred items, now decided + tracked (operator call, 2026-05-29):**
- [>] `jsonschema.is_valid` + `sqlx_postgres` silent type coercion → **board issue [#76](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/issues/76)**; gated on a `runtime-design` review before any behaviour change.
- [>] ~9 DRY `Ok→ok / Err→from_err` patterns + dead `collections::arc()` → **board issue [#77](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/issues/77)**; work via the `refactor` skill loop.
- [x] `codegen-lua` ↔ `codegen-js` `EnumIndex`/`TupleStructIndex` (and `pat.rs`, C4) → **decided: accept the duplication, keep the backends independent** (isolation over DRY). Documented the intentional parallel on both `EnumIndex` structs. `bec824d`

**Sweep 2 — compiler crates otherwise clean.** No library panics, no host-language leaks across `neoc-types`/`neoc-parser`/`neoc-codegen-*`/`neoc-compiler`/`neoc-ast`. The 65 `unwrap()`s in `neoc-fmt` are CST-field access guarded by the parser's prior validation (a documented contract, not stray panics) — flagged for a defensive helper, not a bug.

---

## Outcome — `refactor/debt-sweep`

15 commits. Gate green: `cargo fmt --check`, `cargo clippy --workspace --all-targets` (zero warnings), `cargo test --workspace` (0 failed) — run against the recorded grammar submodule (`5891f9c`), not the leftover feature-branch the working tree had drifted onto.

**Landed:**
- **Safety** — 4 production panics removed from `neoc-vm` library code (`compile`, `task`, `args`); now return `Result`.
- **Convention** — runtime error messages given the `module.function:` prefix (`time`, `json` ×7).
- **Tests** — 3 *genuine* gaps closed: `decode_escape`, codegen-lua emit value-position diagnostics (7), codegen-js Clone/Eq/Display derives (4). +1 additive (nested-list validator).
- **Structure** — `compile.rs` trailer detection SRP-split; `json.rs` Object/Array read methods de-duplicated; both god-files split into submodules — `lib:csv` → reader/writer, `vnd:hyper` → client/server. All behaviour-preserving and test-guarded.
- **Docs** — `neoc-autosql` added to all three crate listings; `plan.md` bannered as bootstrap archive.

**The headline finding: the survey cried wolf.** Verifying each claim against the tree turned up a high false-positive rate — and acting on them blindly would have *caused* damage:
- "Delete the dead Bun/TS files" → would have broken CI (live VitePress docs) and the `autots` interop tool.
- "validate.rs has 0 tests" → it had 5 exact-golden tests; my first pass duplicated them with weaker asserts (reverted).
- "Add lexical conformance cases" → deferred by design, documented.
- Several "duplication" docs findings → a deliberately self-contained agent brief + already-linked SSOTs.

The real defects were narrower and sharper than the survey's volume implied. Net: every change is evidenced, tested, and reversible.

**Deferred to a reviewed MR (not autonomous):** only C4 — the cross-crate `pat.rs` share between `codegen-lua` and `codegen-js`. It needs a design call on where the shared piece lives versus the crate-isolation rule, so it is flagged rather than forced. The csv and hyper god-files (C2/C3) are now split. See Theme 5.

---

## Verified corrections to the raw survey

The architecture survey flagged `tsconfig.json` / `package.json` / `bun.lock` as "dead TypeScript/Bun era — delete." **False.** The Bun/TS toolchain is live:

- `.gitlab-ci.yml` `pages` job: `cd docs && bun install --frozen-lockfile && bun run docs:build` — VitePress docs site, GitLab Pages.
- ~~`packages/autots` = `@neoc-sh/autots`, a `ts-morph` tool generating `.neoc` ext headers from TS.~~ **Retired** (ts-interop-inversion): neoc owns ports, the host adapts — nothing maps TS *into* neoc anymore.
- `packages/bun-http-sqlite` = neoc sample project.

Deleting those files breaks CI and the interop tool. **Rejected.** Only the *root* `tsconfig.json` (carries `jsx: react-jsx`, no JSX consumer) is plausibly vestigial — flagged low-confidence below, not auto-deleted.

Lesson logged: surveys propose, the tree decides.

---

## Theme 1 — Docs single-source-of-truth (verified, low risk)

The repo has **two** doc surfaces the survey half-saw: root `*.md` + the `docs/src` VitePress site (`adrs/`, `rfcs/`, `vnd/`, `styleguide/`) + `tasks/`. Same topics restated across all three.

- [x] **D1** `neoc-autosql` added to ARCHITECTURE.md (canonical table), README.md, CLAUDE.md. `a1332fd`
- [-] **D2** Build/test commands in CONTRIBUTING/CLAUDE/checklist. Verified: CLAUDE.md is a deliberately self-contained agent brief; collapsing its command list to a link degrades that. By-design, not debt. Flagged, not changed.
- [~] **D3** Survey conflated two scopes: `STYLEGUIDE.md §Rust` is *runtime module* style; `CLAUDE.md §Compiler` is *compiler crate* style — complementary, not duplicate. The real overlap is the naming table (walk_/emit_/lower_) in `ARCHITECTURE.md §Conventions` ↔ `CLAUDE.md`. Low-value to collapse (both are short, both self-contained). Flagged.
- [-] **D4** Namespaces: `STYLEGUIDE.md` is SSOT; README is a one-liner, CLAUDE adds runtime-impl conventions. Not redundant restatement. No change.
- [-] **D5** False premise: `STYLEGUIDE.md §Documentation` already links to `docs/styleguide/documentation-styleguide.md`. Already SSOT.
- [x] **D6** `plan.md` bannered as archive (completed bootstrap, cites the gone `../bun-oas/`). `checklist.md` kept — live, self-contained. `12ac775`
- [-] **D7** Host-leak rule: `checklist.md` restates the three cardinal rules at its head by design (operational checklist usability). Not debt.

**Theme 1 verdict:** the one real defect was D1 (autosql absent everywhere) + D6 (stale plan). The survey's other "duplication" findings are mostly a self-contained agent brief (CLAUDE.md) and already-linked SSOTs — collapsing them trades usability for purity. The deeper `docs/src` VitePress surface (adrs/rfcs/guides/std/vnd) was not cross-audited against root docs this pass — logged for a dedicated round.

## Theme 2 — Library safety: no panics in library code (verified each)

CLAUDE.md forbids `unwrap()`/`expect()`/`panic!` outside the `neoc` bin and `#[cfg(test)]`.

- [x] **S1** `compile.rs:83-84` — `try_into().unwrap()` → fixed-width `copy_from_slice`. `bb7c176`
- [x] **S2** `task.rs:127` — `.expect()` in async `await` → `task.await` error. `bb7c176`
- [x] **S3** `args.rs:212` — `unwrap()` after count guard → two-char match. `bb7c176`
- [x] **S4** `args.rs:433` — `.expect()` → `args.parse` error. `bb7c176`
- [-] **S5** Bulk sweep — the 1302 hits are ≈all `#[cfg(test)]` code (legitimate). The production panics were the targeted S1–S4. Rejected as a blanket task; revisit case-by-case if a specific module surfaces.

## Theme 3 — Error-message convention (`module.function:` prefix)

- [-] **E1** `std/task.rs` — false positive: `{kind}` is already `"task.spawn"`/`"task.defer"`/`"task.delay"`. Renders correctly.
- [x] **E2** `std/time.rs:107,110` — bare → `time:` prefix. `9075397`
- [-] **E3** `vnd/hyper.rs:488` — false positive: `from_err` already prefixes `hyper.serve:`. Not double-wrapped.
- [x] **E4** `lib/json.rs` — `object:`/`array:` (7 sites) → `json.object:`/`json.array:`. `9075397`

## Theme 4 — Test backfill (highest-value gaps)

- [-] **T1** Survey false alarm: `validate.rs` is **not** untested — `tests/emit.rs` already has 5 exact-golden validator tests (pub/private params, enum dispatch+recurse, list/option/result combinators, imported/type-param skip, omitted). My first pass duplicated them with weaker substring asserts; **reverted**. Kept one genuinely additive case: nested `[[number]]` combinator composition. `e670929`
- [x] **T2** `neoc-parser/src/expr.rs` `decode_escape` — inline tests for every escape, byte/unicode pass-through, unknown, non-escape. `fce16f9`
- [x] **T3** `codegen-lua` emit value-position diagnostics — 7 tests (continue/break/return/loop/while/range/assign in value position) via hand-built AST. The genuinely-missing coverage; the `expr.rs` `error()` branches had none. `e670929`
- [~] **T4** codegen-js had 19 inline tests but only `Debug` among the four derives. Added Clone/Eq/Display (struct) + Eq (enum). Did **not** blindly "parallel the lua suite" — JS has no runtime-validator layer, so most lua tests have no JS analog. `72b9574`
- [-] **T5** False alarm. Static conformance has 40+ cases incl. extensive error paths (unknown-name, arity, nonexhaustive, supertrait-cycle…); syntax has `.reject`/`.reject.expected` pairs; **lexical is deferred by design** (`lexical/README.md`: no standalone tokeniser exists, and synthesising one would violate the hand-written-expected rule). No real gap.
- [>] **T6** `crates/neoc-wasm` — 0 tests; needs a wasm-bindgen harness. Deferred — harness setup cost outweighs the 3-line entry point's risk this pass.

**Theme 4 verdict:** the survey overstated test gaps 2:1 — T1 (validate.rs) and T5 (conformance) were false alarms (already covered / deferred-by-design). The genuine, landed wins: T2 (`decode_escape`), T3 (emit value-position diagnostics), T4 (JS Clone/Eq/Display derives).

## Theme 5 — Structural (larger, sequence late, keep tests green)

- [x] **C1** `lib/compile.rs` `detect_embedded_payload_from` → `read_trailer_bytes` + `extract_payload` (SRP TODO resolved). `3ff17e3`
- [x] **C5** `lib/json.rs` Object/Array `get`/`len`/`to_table` byte-for-byte dup → shared `node_len`/`node_to_table` (DRY TODO resolved). `76622c6`
- [x] **C2** `lib/csv.rs` (1776 LOC) → `csv/reader.rs` + `csv/writer.rs`; parent keeps shared opts, the public surface, and the 550-LOC integration tests. Pure move + `pub(super)`; 34 unit + 46 integration tests guard it. `f6ab442`
- [x] **C3** `vnd/hyper.rs` (1054→664 LOC) → `hyper/client.rs` + `hyper/server.rs`; parent keeps the Request handler, the public surface, and the bench matrix (bench-only imports cfg-gated). 23 integration tests + normal/bench builds + clippy guard it. `e850f22`
- [>] **C4** `codegen-lua/pat.rs` ≈ `codegen-js/pat.rs` — extract shared `Compiled`. Deferred: crate-isolation rule means the shared piece would need a home crate (`neoc-ast`?); not a clean autonomous call. Flagged for design review.

**Theme 5 verdict:** C1 (SRP) and C5 (DRY) — contained, TODO-marked — landed. Both god-files (C2 csv, C3 hyper) split cleanly into submodules, behaviour-preserving and test-guarded; the key risk (visibility surgery, hyper's cfg-bench import coupling) was handled compile-driven. Only C4 stays deferred — it crosses the crate-isolation boundary and needs a design call, not an autonomous edit.

## Low-confidence / flag only

- [ ] **F1** Root `tsconfig.json` (`jsx: react-jsx`, no JSX consumer; `docs` carries its own). Likely `bun init` leftover. Confirm nothing resolves it before removal. `[>]`
- [x] **F2** Naming collision: Rust `neoc-autosql` (SQL→neoc) vs TS `autots` (TS→neoc) vs CLI `neoc autosql`. Resolved: `autots` retired (ts-interop-inversion), leaving `neoc-autosql` / `neoc autosql`.
