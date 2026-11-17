---
name: pre-commit
description: Mandatory AgentOS quality gate before every push and before opening or updating a PR. Reviews the outgoing change adversarially, runs clean-code-review, clears markers above 0.5, and runs the real gates (lint with atomic, test-id and boundary checks, typecheck, bun test, and test:e2e for anything rendered). Loops until clean. No exceptions for small changes.
---

# Pre-commit gate

Run before every push. Loop until everything below is clean.

1. **Review the outgoing change** (`git diff main...HEAD` plus uncommitted work) for correctness and unhappy paths:
   what renderer message, agent or tool input, stored value, window size or theme breaks it? Every new behaviour
   has a new check (`docs/TESTING.md`): a bun test for `src/shared` logic, a story for each new screen or state,
   a journey spec for each new or changed journey in `JOURNEYS.md`. A bug or a missing check blocks the push:
   fix it now. "Tests later" is not allowed.
2. **Run `clean-code-review`** on the same change.
3. **Markers**: `grep -rn "TODO: clean-code -" apps packages scripts e2e`. Any above 0.5 blocks the push: fix
   it, or run `refactor` until none remain.
4. **Gates**, after every fix:

   ```sh
   bun run lint         # atomic layers, test ids, boundaries, eslint
   bun run typecheck    # svelte-check: includes required testid props
   bun run test         # bun unit tests
   bun run test:e2e     # required if the change touches anything rendered (packages/ui, any renderer, stories,
                        # manifests, e2e/), and always before asking for a merge
   ```

   Snapshot changes are deliberate: `bun run test:e2e:update`, then open the changed PNGs (contact sheet:
   `node e2e/tools/contact-sheet.mjs`). An update nobody looked at is a failure.
5. **Hygiene**: no `.only`, no new `test.skip` / `test.fixme` / `it.skip` without a linked issue, no debug
   `console.log`, no secrets or tokens, no `--no-verify`, no lowered thresholds (axe rules, `maxDiffPixelRatio`,
   exit criteria) to get green.
6. **UI changes**: the `verify-change` skill has run and its "Manual verification" section is ready for the PR.
7. **Repeat from step 1** until the review is clean, no marker above 0.5 remains and every gate passes.

Only then push. Never push straight to `main`: push the branch and open (or update) the PR.
