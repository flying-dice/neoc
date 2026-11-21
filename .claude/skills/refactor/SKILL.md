---
name: refactor
description: Clear one clean-code TODO marker per pass (left by clean-code-review), in neoc or vos code. Picks the highest-scored marker, fixes it without changing behaviour or a public contract (a module's Lua surface, the CLI, a port, the protocol, the app API), runs the gates, reports, and stops. Run it repeatedly to work the list down, or when pre-commit is blocked by a marker above 0.5.
---

# Refactor: one marker per pass

1. List markers: `git grep -nE "TODO: clean-code - [0-9]" -- crates apps packages client scripts e2e types tooling` (Rust,
   Luau, TypeScript and Svelte all use the same `TODO: clean-code - <score> - <PRINCIPLE>` text).
2. Pick the highest score (ties: the one in the current diff first).
3. Fix it and remove the marker. Behaviour stays the same, so the existing tests are the safety net: if the code has no
   test covering it, add one first (a `cargo test` or `cargo nextest` test in the layer, a Lua `*.test.lua` for a module, a
   bun test, a story `play` function, or a journey assertion) and see it pass before and after.
4. If the fix would change a public contract, stop. That is a design change for a PR of its own, not a refactor: report it
   and leave the marker. Contracts are:
   - **neoc:** a module's Lua surface (names, argument and return shapes, error prefixes), the `neoc` CLI, the generated
     `types/neoc.d.luau`, the public API of `neoc-vm` (`lifecycle`, `script_env`, `modules`, `sandbox`);
   - **vos:** a port trait in `vos-application`, a domain or schema type that feeds codegen (`types/generated`), a JSON-RPC
     method, view message or AG-UI event, anything in the `app` API (`types/generated/vos.d.luau`), a tool's name or input,
     an app's `app.json`, a kit component's props or test ids.
5. Run the gate for what you touched: `scripts/check-neoc.sh` for a change confined to the neoc crates (the scoped variant in
   `pre-commit`), otherwise `bun run gate`. Include the e2e half when anything rendered changed.
6. Report the file, what changed and the score cleared. Stop.

No markers, or none above 0.5: report "clean" and stop.
