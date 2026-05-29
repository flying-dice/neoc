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

- [ ] **D1** `neoc-autosql` missing from every crate table. Owner = `ARCHITECTURE.md` crate table; `README.md`, `CLAUDE.md` mirror. Add the row + structure line. (trivial)
- [ ] **D2** Build/test commands restated in `CONTRIBUTING.md`, `CLAUDE.md`, `checklist.md`. SSOT = `CONTRIBUTING.md` Pre-MR checks; others link. (small)
- [ ] **D3** Rust code-style duplicated between `STYLEGUIDE.md` and `CLAUDE.md §Compiler: code style`. SSOT = `STYLEGUIDE.md`; CLAUDE links. (small)
- [ ] **D4** Module namespaces (`std:/lib:/vnd:`) stated in README, STYLEGUIDE, CLAUDE. SSOT = `STYLEGUIDE.md`; README/CLAUDE give one line + link. (small)
- [ ] **D5** `docs/styleguide` vs root `STYLEGUIDE.md` — confirm overlap, collapse to one. (small)
- [ ] **D6** `plan.md` is shipped bootstrap history; `checklist.md` is a live template. Label `plan.md` archive (or move `docs/archive/`); keep `checklist.md`. (small)
- [ ] **D7** Host-leak golden rule: SSOT = `CLAUDE.md`; `checklist.md`/`ARCHITECTURE.md` link not restate. (trivial)

## Theme 2 — Library safety: no panics in library code (verified each)

CLAUDE.md forbids `unwrap()`/`expect()`/`panic!` outside the `neoc` bin and `#[cfg(test)]`.

- [ ] **S1** `crates/neoc-vm/src/lua/lib/compile.rs:83-84` — `try_into().unwrap()` on trailer slices. Use fixed-size array reads / return `Result`. (CRITICAL, trivial)
- [ ] **S2** `crates/neoc-vm/src/lua/std/task.rs:127` — `.expect("Notify fired…")` in public async `await`. Return `Err`. (high, small)
- [ ] **S3** `crates/neoc-vm/src/lua/lib/args.rs:212` — `unwrap()` after a count==1 guard. Restructure. (high, trivial)
- [ ] **S4** `crates/neoc-vm/src/lua/lib/args.rs:433` — `.expect("clap matched subcommand absent from spec")`. Return `Err`. (medium, small)
- [ ] **S5** Sweep remaining `unwrap()/expect()/panic!/todo!/unimplemented!` in `neoc-vm` non-test library code; fix or justify with comment. (medium)

## Theme 3 — Error-message convention (`module.function:` prefix)

- [ ] **E1** `std/task.rs:227-246` — `"{kind}: …"` → `"task.spawn: …"`. (small)
- [ ] **E2** `std/time.rs:107,110` — add `time.<op>:` prefix. (small)
- [ ] **E3** `vnd/hyper.rs:488` — double-wrapped message. (trivial)
- [ ] **E4** `lib/json.rs:507-510` — `"object:"` → `"json.object:"`. (trivial)

## Theme 4 — Test backfill (highest-value gaps)

- [ ] **T1** `crates/neoc-codegen-lua/src/validate.rs` (640 LOC, 0 inline tests) — the runtime type-check emitter. Add inline `mod tests`: primitives, struct composite, enum dispatch, Result/Option, nested `List<Struct>`, cross-module degrade. (CRITICAL, medium)
- [ ] **T2** `crates/neoc-parser/src/expr.rs` `decode_escape` — add inline tests for every escape + unknown. (medium value, low effort — quick win)
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
