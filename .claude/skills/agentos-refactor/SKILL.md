---
name: agentos-refactor
description: Clear one AgentOS clean-code TODO marker per pass (left by agentos-clean-code-review). Picks the highest-scored marker, fixes it without changing behaviour or a public contract (a port, the protocol, the app API), runs the gates, reports, and stops. Run it repeatedly to work the list down, or when agentos-pre-commit is blocked by a marker above 0.5.
---

# Refactor: one marker per pass

1. List markers: `grep -rnE "TODO: clean-code -" crates apps packages client scripts e2e` (Rust, Lua, TypeScript and Svelte
   all use the same `TODO: clean-code - <score> - <PRINCIPLE>` text).
2. Pick the highest score (ties: the one in the current diff first).
3. Fix it and remove the marker. Behaviour stays the same, so the existing tests are the safety net: if the code
   has no test covering it, add one first (a `cargo nextest` test in the layer, a bun test, a story `play` function,
   or a journey assertion) and see it pass before and after.
4. If the fix would change a public contract, stop. That is a design change for an MR of its own, not a refactor:
   report it and leave the marker. Contracts are: a port trait in `agentos-application`, a domain or schema type that
   feeds codegen (`types/generated`), a JSON-RPC method, view message or AG-UI event, anything in the `app` API
   (`types/generated/luals/`), a tool's name or input, an app's `app.json`, a kit component's props or test ids.
5. Run `bun run lint && bun run typecheck && bun run test`, plus `bun run test:e2e` if anything rendered changed.
6. Report the file, what changed and the score cleared. Stop.

No markers, or none above 0.5: report "clean" and stop.
