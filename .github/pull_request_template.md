## What and why

<!-- One paragraph. Link the issue or round report it answers. -->

## Gate

- [ ] CI green on the head commit: lint (atomic, test ids, boundaries, eslint) · typecheck · test · e2e
- [ ] New behaviour has new checks: bun tests for logic, a story per new screen or state, a spec per new or changed journey
- [ ] Changed snapshots were opened and looked at (or none changed)
- [ ] `pre-commit` run: no `TODO: clean-code` marker above 0.5, no skipped tests, no lowered thresholds
- [ ] SDK or protocol change (or none): zod schema, desktop host, memory host and tests updated together

## App readiness

<!-- New app, or a change people will see or feel in an app. Otherwise write "n/a". -->

- [ ] `manifest.window` with a justified `minWidth` / `minHeight`
- [ ] A story for every screen and state
- [ ] `JOURNEYS.md` with a passing spec for every journey
- [ ] `verify-change` record below
- [ ] UX round meets the exit criteria: `docs/ux/<app>/round-<n>.md` (paste the scorecard)

## Manual verification

<!-- The verify-change record: what was checked, in which states, with screenshots. "n/a" only if nothing visible changed. -->

## Review verdict

<!-- From the agentos-code-review skill: READY TO MERGE, or STAYS IN MR with what must change. -->
