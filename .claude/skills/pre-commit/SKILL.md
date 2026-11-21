---
name: pre-commit
description: Mandatory quality gate before every push and before opening or updating a PR, for neoc and vos changes alike. Reviews the outgoing change adversarially (change-review), runs clean-code-review, clears markers above 0.5, and runs the one gate (`bun run gate`, or `scripts/check-neoc.sh` for a change confined to the neoc crates). Loops until clean. No exceptions for small changes.
---

# Pre-commit gate

Run before every push. Loop until everything below is clean.

1. **Review the outgoing change** (`git diff origin/main...HEAD` plus uncommitted work) with `change-review`: correctness,
   unhappy paths, tests. Ask what breaks it: a script value, a client message, a JSON-RPC param, an agent or tool input, an
   app return value, a stored value from an older version, a window size or theme. Every new behaviour has a new check, in the
   layer it lives in: a Rust test (neoc-vm unit test; a `crates/neoc/tests/**/*.test.lua` suite for a module; `cargo nextest`
   for the vos layers: domain, use cases against the port fakes, adapters, the host end to end), a bun test for client and kit
   logic, a story or a journey for each new screen or state, a journey spec for each new or changed journey in
   `JOURNEYS.md`. A bug or a missing check blocks the push: fix it now. "Tests later" is not allowed.
2. **Run `clean-code-review`** on the same change.
3. **Markers**: `git grep -nE "TODO: clean-code - [0-9]" -- crates apps packages client scripts e2e types tooling`. Any above
   0.5 blocks the push: fix it, or run `refactor` until none remain.
4. **The gate**, after every fix. There is one command set, the same one `CONTRIBUTING.md` lists:

   ```sh
   bun run gate           # the neoc checks (scripts/check-neoc.sh), then the vos checks: lint, typecheck, test, test:e2e
   scripts/check-neoc.sh  # scoped variant for a change confined to the neoc crates: no bun, no darklua
   ```

   `bun run gate` sets `VOS_GATE=1` and `NEOC_REQUIRE_LUAU_LSP=1` for every step, so a missing `luau-lsp`, `stylua` or
   `darklua` fails instead of skipping; `scripts/check-neoc.sh` sets `NEOC_REQUIRE_LUAU_LSP=1` itself. A skip is not green
   either way: install the tool. Both variables keep their meaning: `VOS_GATE=1` (or `CI`) fails the vos lints
   (`lint:luau`, `lint:stylua`) and the `vos-codegen` editor test when their tool is missing; `NEOC_REQUIRE_LUAU_LSP=1` does
   the same for `crates/neoc-vm/tests/luau_decls.rs`.

   Use the **scoped variant** only when the diff touches no `crates/vos-*`, `apps/`, `client/`, `packages/`, `e2e/`,
   `scripts/` (other than `scripts/check-neoc.sh`, `scripts/check-no-vos-leak.sh`, the install scripts and the toolchain pin) and does not change
   `neoc-vm`'s embedding API or sandbox (`lifecycle`, `script_env`, `modules`, `sandbox`), which `vos-lua` embeds, nor the
   turn behaviour of a module: the `Role` of an async host function (`task.wait` in `std/task.rs`, `std:thread`, `vnd:hyper`,
   `lib:http`, `Mutex`, ...) or what a module registers with `Life`. vos's `turns.rs` and `chaos.rs` pin those choices, and
   only the full gate runs them. It
   covers `cargo fmt`, clippy with `-D warnings`, `cargo test -p neoc -p neoc-vm`, the Lua suites, the release
   `abort_regressions` run, the bench smoke, the toolchain pin, `cargo deny`, the Luau declarations (`luau_decls --check` and
   the luau-lsp fixtures), `vos-codegen --check` (a new neoc module regenerates `types/generated/vos.d.luau`: run
   `cargo run -p vos-codegen`), a check that no neoc crate depends on a `vos-*` crate, and `cargo check --workspace
   --all-targets --locked`. Everything else gets the full `bun run gate`, and it is always the full gate before asking
   for a merge of a change that touches vos. If you changed the Rust schema or the app API, `bun run codegen` first and commit
   the regenerated files.

   Snapshot changes are deliberate: `bun run test:e2e:update`, then open the changed PNGs (contact sheet:
   `node e2e/tools/contact-sheet.mjs`). An update nobody looked at is a failure. e2e needs a Mac for the baselines.
5. **Hygiene**: no `.only`, no new `test.skip` / `test.fixme` / `it.skip` / `#[ignore]` without a linked Jira key, no debug
   `console.log` / `println!` / `dbg!`, no secrets or tokens, no `--no-verify`, no lowered thresholds (axe rules,
   `maxDiffPixelRatio`, exit criteria) to get green, no new `unwrap` / `expect` / `unsafe` / clippy `allow` without a stated
   reason, no hand edits to generated files (`types/neoc.d.luau`, `types/generated/*`), no leftover conflict markers.
6. **UI changes**: the `verify-change` skill has run and its "Manual verification" section is ready for the PR.
7. **Repeat from step 1** until the review is clean, no marker above 0.5 remains and every gate passes.

Only then push the `issues/NEOC-<n>-<slug>` branch and open (or update) the PR into `main`; nothing is pushed to `main`
directly. Next come the `pr-self-review` note and an independent `pr-review` pass.
