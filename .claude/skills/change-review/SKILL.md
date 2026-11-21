---
name: change-review
description: Review a set of changes (the staged diff, or a branch's diff) for correctness — before committing, pushing, or as a standalone pass
---

Review the change on its own merits. No session context assumed — the diff has to stand by itself.

## Process

1. **See exactly what changed** — the staged diff (`git diff --staged`), or a branch diff (`git diff <base>...HEAD`), plus `git status` for new files the change depends on. Review only what's in the change set.
2. Read the diff in context — what changed, why. Open the surrounding code the hunks touch; a hunk that reads fine alone can still break its caller.
3. **Run the checks the change touches** — build, lint, typecheck, and the relevant tests for the affected area. Scope to what the diff affects; don't rebuild the world (the full gate is `pre-commit`'s job, once, on the final head; on a re-review after fixes, re-run only what the delta touches). Run them locally: the local run is the evidence.
4. Devise unhappy-path scenarios (given/when/then) and run the ones the tests don't cover, as targeted tests or probes at the head. Judge whether a test would catch a defect by reading it and the code; never by mutating the code, reverting a fix, or looping a test. A failed scenario → a full given/when/then in the finding so the author can turn it into a test.
5. **Test gap** — behaviour added with no test is a finding (`issue`), not a pass.
6. Report findings (format below). `bug`/`issue` **block**; `nit`/`question` don't.

## Checklist

Pass, fail, or `N/A — <reason>`. No blanks.

- [ ] Builds + affected tests pass (run locally, not assumed)
- [ ] New behaviour has tests
- [ ] Destructive/edge-case paths considered
- [ ] No silent reverts
- [ ] No unrelated changes, no debug/WIP leftovers (`dbg!`, stray `println!`, `.only`, commented-out code)
- [ ] Change does what its message/intent says

## Comment format

```
**[type]** summary

detail
```

- `bug` — breaks at runtime. Blocks.
- `issue` — wrong but won't crash (missing test, edge case, leak). Blocks.
- `nit` — non-blocking.
- `question` — answer before proceeding.

## Focus

Weight unhappy paths. The happy path probably works — the author just wrote it. Find where it falls apart: regressions, edge cases, error paths, concurrency, rollback safety, interface changes without migration, silent reverts, debug/WIP code sneaking in.

## Ignore

Style a formatter/linter already enforces. "I'd do it differently." Re-running a check you already ran at this exact revision. Mutation testing (editing code to watch a test fail, looping a test to rule out a flake). Anything outside the change under review.
