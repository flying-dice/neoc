## What and why

<!-- One paragraph. Link the issue or round report it answers. -->

## Gate

- [ ] Gate green locally on the head commit: lint (atomic, test ids, clippy, crate direction, luau-lsp, StyLua, codegen, eslint) · typecheck · test · e2e
- [ ] New behaviour has new checks: `cargo nextest` tests for Rust and Luau logic, bun tests for client and kit logic, a story or journey per new screen or state, a spec per new or changed journey
- [ ] Changed snapshots were opened and looked at (or none changed)
- [ ] `pre-commit` run: no `TODO: clean-code` marker above 0.5, no skipped tests, no lowered thresholds
- [ ] Host API or protocol change (or none): domain schema, application port and use case, adapter, `bun run codegen` output and tests updated together; no side channel

## App readiness

<!-- New app, or a change people will see or feel in an app. Otherwise write "n/a". -->

- [ ] `app.manifest` `window` with a justified `minW` / `minH`; `app.json` with the smallest `permissions`; `lint:luau` and `lint:stylua` pass
- [ ] Every screen and state reached by a journey or a story; logic and tools tested from Rust
- [ ] `JOURNEYS.md` with a passing spec for every journey
- [ ] `verify-change` record below
- [ ] UX round meets the exit criteria: `docs/ux/<app>/round-<n>.md` (paste the scorecard)

## Manual verification

<!-- The verify-change record: what was checked, in which states, with screenshots. "n/a" only if nothing visible changed. -->

## Review verdict

<!-- From the agentos-code-review skill: READY TO MERGE, or STAYS IN MR with what must change. -->
