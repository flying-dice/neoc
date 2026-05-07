<!--
The agent-backlog Kanban workflow is defined in
`.claude/skills/agent-backlog-kanban/SKILL.md` in the repo root.
The skill auto-loads in any Claude Code session opened from a clone of
this repo and is the source of truth for column rules and gates.
-->

## Summary

<!-- What changes and why. One paragraph. -->

## Linked issue

<!-- e.g. "Closes #123" — refinement issue this PR resolves. -->

## Acceptance

<!-- The Definition of Done agreed on the linked issue at the Refinement gate.
     Cross-reference each criterion to the test(s) that exercise it. -->

## Verification

- [ ] `cargo test --lib` (Rust unit tests)
- [ ] `cargo run -- 'tests/**/*.test.luau'` (Luau integration tests)
- [ ] `cargo clippy --all-targets` (lint)
- [ ] `cargo fmt --check` (formatting)
- [ ] Docs build clean (`bun run docs:build` from `docs/`)
- [ ] Testing report attached or linked (Review gate, per the kanban skill)
