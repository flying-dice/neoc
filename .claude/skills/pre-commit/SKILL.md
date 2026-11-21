---
name: pre-commit
description: Mandatory quality gate before every push and before opening or updating a PR, for neoc and vos changes alike. Reviews the outgoing change adversarially (change-review), runs clean-code-review, clears markers above 0.5, and runs the one fast gate (`bun run gate`, `bun run gate --no-e2e` or `bun run gate:neoc`, per CLAUDE.md, "The gate"). Loops until clean. No exceptions for small changes. No mutation testing.
---

# Pre-commit gate

Run before every push. Loop until everything below is clean. Scale the work to the change: a small diff gets a single-reader
`change-review` and `clean-code-review` pass and the fast gate; sub-agent fan-outs are for large or risky diffs only.

1. **Review the outgoing change** (`git diff origin/main...HEAD` plus uncommitted work) with `change-review`: correctness,
   unhappy paths, tests. Ask, by reading, what breaks it: a script value, a client message, a JSON-RPC param, an agent or tool input, an
   app return value, a stored value from an older version, a window size or theme. Every new behaviour has a new check, in the
   layer it lives in: a Rust test (neoc-vm unit test; a `crates/neoc/tests/**/*.test.lua` suite for a module; `cargo nextest`
   for the vos layers: domain, use cases against the port fakes, adapters, the host end to end), a bun test for client and kit
   logic, a story or a journey for each new screen or state, a journey spec for each new or changed journey in
   `JOURNEYS.md`. A bug or a missing check blocks the push: fix it now. "Tests later" is not allowed.
2. **Run `clean-code-review`** on the same change.
3. **Markers**: `git grep -nE "TODO: clean-code - [0-9]" -- crates apps packages client scripts e2e types tooling`. Any above
   0.5 blocks the push: fix it, or run `refactor` until none remain.
4. **The gate**, on the final head and after a fix that touches more than a test or a doc line. There is one command set, the
   one `CONTRIBUTING.md` lists, and the rule for which one and when e2e is required is stated once, in `CLAUDE.md` ("The
   gate"). Check every path the diff touches (`git diff --name-only main...HEAD`) against it: a code change in a vos crate
   takes e2e even when no screen changes, and so do a vos test fixture e2e copies, a lockfile bump and a `neoc-vm` change
   that already takes the full gate.

   ```sh
   bun run gate            # the neoc checks (scripts/check-neoc.sh), then the vos checks: lint, typecheck, test, e2e
   bun run gate --no-e2e   # the same without e2e, only where the e2e rule in CLAUDE.md allows it
   bun run gate:neoc       # scripts/check-neoc.sh alone: a neoc-only change CLAUDE.md does not send to the full gate (no bun, no darklua)
   ```

   Between fixes run only what the fix touches (one test, one crate, one lint); the gate runs once more on the final head. It
   is fast on purpose (a few minutes warm). The slow suites (the `hostile_inputs` sweep, the release `abort_regressions` run,
   the full generated-tree case counts) are not in it: `bun run slow` runs them, and a change to what they guard (a module's argument
   handling, stack use or recursion, the VM's call scheduling) runs it once before the PR. A new test is fast, or it is
   `#[ignore = "slow: ..."]`, which `scripts/check-slow.sh` finds by that reason (nothing to add there).

   `bun run gate` sets `VOS_GATE=1` and `NEOC_REQUIRE_LUAU_LSP=1` for every step, so a missing `luau-lsp`, `stylua` or
   `darklua` fails instead of skipping; `scripts/check-neoc.sh` sets `NEOC_REQUIRE_LUAU_LSP=1` itself. A skip is not green
   either way: install the tool. Both variables keep their meaning: `VOS_GATE=1` (or `CI`) fails the vos lints
   (`lint:luau`, `lint:stylua`) and the `vos-codegen` editor test when their tool is missing; `NEOC_REQUIRE_LUAU_LSP=1` does
   the same for `crates/neoc-vm/tests/luau_decls.rs`.

   `gate:neoc` covers `cargo fmt`, clippy with `-D warnings`, the `neoc` and `neoc-vm` tests, the Lua suites, `vos-lua`'s
   module tests (`--test modules`, less the one darklua test), the bench smoke, the toolchain pin, `cargo deny`, the Luau declarations (`luau_decls --check` and the luau-lsp fixtures),
   `vos-codegen --check` (a new neoc module regenerates `types/generated/vos.d.luau`: run `cargo run -p vos-codegen`), a check
   that no neoc crate depends on a `vos-*` crate, and `cargo check --workspace --all-targets --locked`. If you changed the
   Rust schema or the app API, `bun run codegen` first and commit the regenerated files.

   Snapshot changes are deliberate: `bun run test:e2e:update`, then open the changed PNGs (contact sheet:
   `node e2e/tools/contact-sheet.mjs`). An update nobody looked at is a failure. e2e needs a Mac for the baselines.
   **No mutation testing**: do not edit code to watch a test fail, revert a fix to see a test go red, or loop a test to prove
   it is not flaky. Judge a test by reading it and the code.
5. **Hygiene**: no `.only`, no new `test.skip` / `test.fixme` / `it.skip` / `#[ignore]` without a linked Jira key, no debug
   `console.log` / `println!` / `dbg!`, no secrets or tokens, no `--no-verify`, no lowered thresholds (axe rules,
   `maxDiffPixelRatio`, exit criteria) to get green, no new `unwrap` / `expect` / `unsafe` / clippy `allow` without a stated
   reason, no hand edits to generated files (`types/neoc.d.luau`, `types/generated/*`), no leftover conflict markers.
6. **UI changes**: the `verify-change` skill has run and its "Manual verification" section is ready for the PR.
7. **Repeat from step 1** until the review is clean, no marker above 0.5 remains and every gate passes.

Only then push the `issues/NEOC-<n>-<slug>` branch and open (or update) the PR into `main`; nothing is pushed to `main`
directly. Next come the `pr-self-review` note and an independent `pr-review` pass.
