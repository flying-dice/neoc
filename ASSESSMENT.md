# Debt Assessment — `refactor/debt-sweep`

Living backlog. Source: four parallel surveys (architecture, clean code, docs, tests), each finding verified against the tree before landing. Worked top-down, commit per item.

Status keys: `[ ]` open · `[x]` landed · `[~]` partial · `[-]` rejected (false positive) · `[>]` deferred (risky to do autonomously).

---

## Verified corrections to the raw survey

The architecture survey flagged `tsconfig.json` / `package.json` / `bun.lock` as "dead TypeScript/Bun era — delete." **False.** The Bun/TS toolchain is live:

- `.gitlab-ci.yml` `pages` job: `cd docs && bun install --frozen-lockfile && bun run docs:build` — VitePress docs site, GitLab Pages.
- `packages/autots` = `@neoc-sh/autots`, a maintained `ts-morph` tool generating `.neoc` ext headers from TS. Recent commit `b32ac79 feat(autots)`.
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

- [ ] **T1** `crates/neoc-codegen-lua/src/validate.rs` (640 LOC, 0 inline tests) — the runtime type-check emitter. Add inline `mod tests`: primitives, struct composite, enum dispatch, Result/Option, nested `List<Struct>`, cross-module degrade. (CRITICAL, medium)
- [x] **T2** `neoc-parser/src/expr.rs` `decode_escape` — inline tests for every escape, byte/unicode pass-through, unknown, non-escape. `fce16f9`
- [ ] **T3** `crates/neoc-codegen-lua` `expr.rs` error branches (≈11 `error()` calls) — assert diagnostics fire. (high, medium)
- [ ] **T4** `crates/neoc-codegen-js/tests/emit.rs` — parallel the lua emit suite (patterns, expr lowering, each derive). (high, medium-high)
- [ ] **T5** `lang/CONFORMANCE` — add lexical/static error-case pairs. (medium)
- [ ] **T6** `crates/neoc-wasm` — 0 tests; needs a wasm harness. (deferred — harness cost) `[>]`

## Theme 5 — Structural (larger, sequence late, keep tests green)

- [ ] **C1** `lib/compile.rs` `detect_embedded_payload_from` — split open/seek/read/validate/extract (SRP TODO at :94). (medium)
- [ ] **C2** `lib/csv.rs` (1776 LOC) — split reader/writer submodules. (medium)
- [ ] **C3** `vnd/hyper.rs` (1054 LOC) — split client/server/types. (medium)
- [ ] **C4** `codegen-lua/src/pat.rs` ≈ `codegen-js/src/pat.rs` — extract shared `Compiled` test-collection, keep target formatting split. (medium) `[>]` evaluate against crate-isolation rule first.

## Low-confidence / flag only

- [ ] **F1** Root `tsconfig.json` (`jsx: react-jsx`, no JSX consumer; `autots` and `docs` carry their own). Likely `bun init` leftover. Confirm nothing resolves it before removal. `[>]`
- [ ] **F2** Naming collision: Rust `neoc-autosql` (SQL→neoc) vs TS `autots` (TS→neoc) vs CLI `neoc autosql`. Confusing trio. Doc the distinction. `[>]`
