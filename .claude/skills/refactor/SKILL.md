---
name: refactor
description: Clear one AgentOS clean-code TODO marker per pass (left by clean-code-review). Picks the highest-scored marker, fixes it without changing behaviour or the SDK contract, runs the gates, reports, and stops. Run it repeatedly to work the list down, or when pre-commit is blocked by a marker above 0.5.
---

# Refactor: one marker per pass

1. List markers: `grep -rn "TODO: clean-code -" apps packages scripts e2e --include=*.ts --include=*.svelte`.
2. Pick the highest score (ties: the one in the current diff first).
3. Fix it and remove the marker. Behaviour stays the same, so the existing tests are the safety net: if the code
   has no test covering it, add one first (a bun test in `src/shared`, a story `play` function, or a journey
   assertion), and see it pass before and after.
4. If the fix would change a public contract (anything exported from `@agentos/sdk`, a `host/protocol` channel, a
   tool's name or input, a kit component's props or test ids), stop. That is a design change for an MR of its
   own, not a refactor: report it and leave the marker.
5. Run `bun run lint && bun run typecheck && bun run test`, plus `bun run test:e2e` if anything rendered changed.
6. Report the file, what changed and the score cleared. Stop.

No markers, or none above 0.5: report "clean" and stop.
