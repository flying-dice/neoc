---
name: pre-commit
description: Mandatory AgentOS quality gate before every push and before opening or updating a PR. Reviews the outgoing change adversarially, runs clean-code-review, clears markers above 0.5, and runs the real gates (lint with clippy, crate direction, cargo deny, luau-lsp, StyLua, atomic and test-id checks; typecheck; cargo nextest and bun test; and test:e2e for anything rendered). Loops until clean. No exceptions for small changes.
---

# Pre-commit gate

Run before every push. Loop until everything below is clean.

1. **Review the outgoing change** (`git diff main...HEAD` plus uncommitted work) for correctness and unhappy paths:
   what client message, JSON-RPC param, agent or tool input, app return value, stored value, window size or theme
   breaks it? Every new behaviour has a new check (`docs/TESTING.md`): a `cargo nextest` test in the layer the logic
   lives in, a bun test for client and kit logic, a story or a journey for each new screen or state, a journey spec for
   each new or changed journey in `JOURNEYS.md`. A bug or a missing check blocks the push: fix it now. "Tests later" is
   not allowed.
2. **Run `clean-code-review`** on the same change.
3. **Markers**: `grep -rnE "TODO: clean-code -" crates apps packages client scripts e2e`. Any above 0.5 blocks the push:
   fix it, or run `refactor` until none remain.
4. **Gates**, after every fix:

   ```sh
   bun run lint         # atomic layers, test ids, conflict markers, cargo fmt + clippy -D warnings, crate direction
                        # (check-deps), cargo deny, luau-lsp, StyLua, codegen:check, eslint
   bun run typecheck    # cargo check --workspace --all-targets, tsc on types/, svelte-check (required testid props)
   bun run test         # cargo nextest run --workspace, then bun test (types, client, kit)
   bun run test:e2e     # required if the change touches anything rendered (packages/ui, the client, apps, stories,
                        # e2e/), and always before asking for a merge; needs a Mac for the baselines
   ```

   `lint:luau` and `lint:stylua` say `SKIPPED` when `luau-lsp` or `stylua` is missing: a skip is not green, so install
   them (and `cargo-nextest`, `cargo-deny`) before calling the gate clean. If you changed the Rust schema or the app API,
   run `bun run codegen` first and commit the regenerated files.

   Snapshot changes are deliberate: `bun run test:e2e:update`, then open the changed PNGs (contact sheet:
   `node e2e/tools/contact-sheet.mjs`). An update nobody looked at is a failure.
5. **Hygiene**: no `.only`, no new `test.skip` / `test.fixme` / `it.skip` / `#[ignore]` without a linked issue, no debug
   `console.log` / `println!` / `dbg!`, no secrets or tokens, no `--no-verify`, no lowered thresholds (axe rules,
   `maxDiffPixelRatio`, exit criteria) to get green, no new `unwrap` / `expect` / `unsafe` / clippy `allow` without a
   stated reason, no hand edits to generated files, no leftover conflict markers.
6. **UI changes**: the `verify-change` skill has run and its "Manual verification" section is ready for the PR.
7. **Repeat from step 1** until the review is clean, no marker above 0.5 remains and every gate passes.

Only then push. Never push straight to `main`: push the branch and open (or update) the PR.
