---
name: pr-self-review
description: Use before marking a pull request ready, handing it to an independent review, or re-handing it after fixes — author-side quality gate (the same 11-gate rubric as pr-review; G11 covers the vos checks when the diff touches vos paths)
---

Author gate. Run before every handoff to review and every re-handoff. Reviewer runs the same rubric — anything skipped here, they find, and each find costs a full round.

## Standard

You are your own first reviewer. Default verdict FAIL until each gate carries evidence. Evidence = command + measured output at the head SHA. "Tested locally", "should work", "covered" = no evidence = FAIL. Judge by the blocking rule in the rubric: a real bug, data loss or a security hole, a broken acceptance criterion, a failing gate or a no-vos-leak or boundary violation is a FAIL; anything else you find is a follow-up ticket in Jira NEOC, listed in the note, and does not hold the hand-off (speed is a requirement, CLAUDE.md, "Non-functional requirements").

## Process

1. **Freeze head.** Push. Record `git rev-parse HEAD`. Run the pre-PR checks at that SHA (see G1; the gate is fast, and e2e only when CLAUDE.md, "The gate", requires it).
2. **Run the rubric** in author mode. Fan-out per the rubric, `model: "sonnet"`. When the diff touches vos paths, G11 means reading the section "vos checks" of `.claude/skills/pr-review/SKILL.md` and running it.
3. **Fix loop.** Any FAIL → fix, push, then re-run only what the fix touches (a targeted test, the check of the touched path) and the gate once on the final head. No mutation, reverts or looped runs to prove a fix works: read the test and the code. A note is valid only for the SHA it names.
4. **Post** the Self-review note (template below) on the PR.
5. **Hand off to an independent `pr-review` pass** only when every gate is PASS or `N/A — <reason>`.

<!-- rubric:begin -->
## Rubric

11 gates. Cost-ordered. Each gate: PASS / FAIL / `N/A — <reason>`. No blanks. Evidence = command + measured output at head SHA. Assertion without evidence = FAIL.

Blocking: a gate is FAIL, and a merge is held, only for a real bug; data loss or a security hole; a broken acceptance criterion; a failing gate (a pre-PR check that does not pass); a no-vos-leak or boundary violation. Every other defect a gate turns up (doc wording, comment accuracy, extra test coverage, cosmetic issues, naming, clean-code nits) is recorded as a follow-up ticket in Jira NEOC and does not hold the merge: that gate is PASS, with the ticket key beside it. Iteration speed is a requirement (CLAUDE.md, "Non-functional requirements"), and it comes from fast checks and small PRs, never from fewer review rounds or a shortcut that lets a bug through.

Stop rules:
- **Author mode** (`pr-self-review`): G1–G4 fail-fast — first FAIL → fix, push, restart at G1. G5–G11 run in full; fix every FAIL, push, restart at G1.
- **Reviewer mode** (`pr-review`): G1 FAIL → hand back, stop — no probing a red or stale head. G2–G11 run in full; post every blocking finding in one pass and list the follow-ups beside them. A partial review buys another round.

Local runs: run the pre-PR checks once at the head SHA (G1), then only what the rubric adds — unhappy-path probes, greps, link checks. A gate needing a check the pre-PR checks lack → run it by command and record the gap. Never mutate code, revert a fix, or loop a test N times to see it fail or to rule out a flake: judge a test by reading it and the code it covers, and run it once, targeted. A new test is fast (it fits the gate) or is `#[ignore = "slow: ..."]` under `bun run slow`.

### Fan-out
`git diff --stat origin/<target>...HEAD` → a large or risky diff only (> 400 lines changed, > 10 files, or a change to the VM's embedding API or sandbox, the protocol, the host's HTTP surface or a crate boundary): one message, 4 parallel `Agent` calls (5 when the diff touches vos paths, see G11), `subagent_type: "Explore"`, model as the skill pins. Read-only — never edit, push, or post. Otherwise single-reader: run every gate yourself.

| Agent | Gates |
|---|---|
| claims-vs-code | G2, G9 |
| test-validity | G5 |
| unhappy-paths | G6, G7 |
| docs-drift | G8 |
| vos-checks (vos paths only) | G11 |

G1, G3, G4, G10: run yourself.

- Prompt each with: PR number, head SHA, worktree path, its gate text verbatim, return format `file:line — failure mode — evidence command + output`.
- Re-measure every sub-agent finding before acting on it. Not reproducible → record as such. Never drop silently.
- `Agent` tool absent or denied → fan-out NOT RUN. Run single-reader, record `Mode: degraded — <reason>`. Never present a single-reader pass as fan-out.

### G1 — Head & checks
GitHub Actions is not a gate. Run the pre-PR checks from CONTRIBUTING.md locally at the head SHA and record command + output: `bun run gate` (the neoc checks, then the vos checks: lint, typecheck, test, and e2e), `bun run gate --no-e2e` when e2e is not required, or `bun run gate:neoc` for a change confined to the neoc crates (see "The gate" in CLAUDE.md for which one and when e2e is required). The slow suites are not part of the gate (`bun run slow`). A lint that printed `SKIPPED`, or a required e2e you could not run (it needs a Mac), is unverified, not passed.
- Head SHA recorded: local `git rev-parse HEAD` == PR head `sha`.
- Every pre-PR check passes at that exact SHA. Any failure = FAIL. `gate:neoc` on a diff outside its scope (CLAUDE.md, "The gate"), or `--no-e2e` on a diff that requires e2e = FAIL.
- The checks cover the change: the Lua suite or Rust tests for the touched area ran (not filtered out).
- Re-request: ≥ 1 new commit since the last reviewed head. 0 = FAIL.
- `main` exists as the target. No merge conflicts.

### G2 — Claims ↔ evidence
Scope: PR description, commit messages, every comment and doc line added or edited in the diff.
- Each factual claim — behaviour, rationale, figure, "only", "every", "never", "N×", "orders of magnitude" — has a command + output proving it, or is deleted.
- Comment states what code does → read the code, confirm. Comment says the opposite = FAIL.
- Rationale for ignores / allowlists / suppressions → verified per entry, not by a header sentence.
- Description ↔ diff: every file in the diff accounted for; nothing described that the diff lacks.

### G3 — Acceptance criteria
- Table: AC (verbatim from the ticket) → test name or `file:line` → PASS / FAIL.
- AC not met as written → amend on the Jira ticket first (comment + ticket owner acknowledgement), then cite it. PR-only amendment = FAIL.

### G4 — Diff hygiene
- Unrelated change present → split into its own PR.
- Silent revert: `git diff <last-reviewed-head>..HEAD` and `git log -p origin/<target>..HEAD` — no line or value from an earlier fix commit undone. Intended reverts named in the description with reason.
- Rename / removal: `grep -rn '<old term>'` → 0 hits outside changelog / history. Includes test names, comments, docs.
- Formatter + lint for touched files pass — run by command (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`; `bun run lint` for vos paths). Prose wrap matches the surrounding paragraph.
- `neoc-vm` and `neoc` take no vos concept (apps, `readOnly` tools, views, `app.*`, a `vos:` module): `cargo tree -p neoc -p neoc-vm -e all --prefix none | grep '^vos-'` prints nothing, and a new name or type in those crates that only vos needs = FAIL (it belongs in `crates/vos-lua`).
- Reference the Jira key (`refs NEOC-<n>`) only. `Closes` / `Fixes` / `Resolves` = FAIL.
- Breaking change (public API, language semantics, config key, CLI flag, wire format) → conventional `!` or `BREAKING CHANGE:` footer.
- No debug output, commented-out code, or TODO without a Jira ref.

### G5 — Test validity
- Every new public fn / branch / error arm → ≥ 1 test exercising it. Report count: new items vs tested items.
- Each regression test asserts the reported behaviour, read from the test body. An assertion that would still hold with the defect present (attribute-only, tautological, mocked-out subject) = FAIL. Judge tests by reading the test and the code — never by mutating the code, reverting the fix, re-running against altered code, or looping the test to prove it bites or is not flaky. A targeted run of the test at head is fine.
- Test reaches the call site the defect lived at — not a helper beside it, not a fixture that skips the branch.
- Deny / allow lists: assert an unlisted entry is rejected, not only that listed ones are.
- BDD: runner fails on undefined / skipped steps (`fail_on_skipped` or equivalent); summary shows skipped = 0. Count scenarios with `grep -cE '^\s*(Scenario|Scenario Outline):'`, compare names not totals.
- Fault injection: hook can clear mid-test; each guard driven on its own.
- Harness can express the failure:
  - Keyboard / focus / scroll / layout → real browser. jsdom: Space on a button is inert; `scrollHeight` / `clientHeight` = 0.
  - Playwright `click()` auto-scrolls → native `el.click()` for scroll assertions.
  - Signal races → in-process same-tick `process.kill`; external `kill(2)` never enters the window.
- Removed or weakened test → justified in the description. Removed to satisfy a done-check = FAIL.
- New test is fast: it runs in the gate's budget (CLAUDE.md, "The gate"), or it is `#[ignore = "slow: ..."]` and listed under `bun run slow`. A slow test in the gate is a follow-up to move it, unless it adds more than ~10 s to the gate: then FAIL.

### G6 — Unhappy paths
Given / when / then table. Each category run or `N/A — <reason>`:

| Category | Probe |
|---|---|
| Edge input | empty, max, unicode, NULL, nesting depth, duplicates, ordering |
| Error path | each `Err` / `throw` / non-zero exit reached and asserted |
| Concurrency / lifecycle | shutdown mid-op, retry timer vs teardown, double start, claim/lock scope |
| Security / sandbox | arg / env injection, eval or bytecode escape, secrets in job scope or logs, unverified binaries |
| Rollback | PR reverts cleanly; migration has a down path |
| Interface migration | every caller / consumer updated or shimmed; semver honoured |
| Complexity | per-pass recomputation, exponential recursion, unbounded buffers |
| CI config | triggers fire on the intended events; steps fail closed |

### G7 — Narrowing & deletion audit
- New or narrowed predicate / match arm / filter → list the shapes it now excludes, from the code, and find the test that holds each one (or run one targeted probe at head). No pre-fix build, no reverted code. Valid case silenced = FAIL.
- Deleted read / query / guard → list every clause it carried (WHERE, LIMIT, permission check); probe the state each one excluded.
- Same function fixed twice → review its contract, not the symptom.

### G8 — Docs drift
- CONTRIBUTING, README, `apps/README.md`, CLAUDE.md, rustdoc, CLI help, and the skills or workflows that name the behaviour → updated in the same PR.
- Every added or changed link resolves (`test -e` for paths, HTTP 200 for URLs).
- Docs describe the state after this PR, not a future one. Dependency on another PR → declared in the description and enforced: draft until the dependency merges, merge order at the top of the description. Declared but unenforced = FAIL.
- Grep the doc set for the changed term — every mention agrees. Self-contradiction = FAIL.

### G9 — Citations
- Every `file:line` posted comes from `grep -n` / `sed -n` at the head SHA in the same pass. No `cat -n` window offsets. No recall.

### G10 — Fix-round discipline (re-request only)
Rounds continue until the independent reviewer returns READY TO MERGE at the exact head; every fix round is re-reviewed, and nobody merges over an open blocking finding. A re-review reads the delta (the lines since the last reviewed head and their tests) and treats it as new code under the full blocking rules: it checks the fix, and checks for regressions the fix itself introduced. Re-run only the checks the delta touches (G1), but read all of the delta.
- Per prior thread: fix commit SHA + test name + the finding's given/when/then run at the new head (a targeted run of that test) → fixed behaviour observed.
- Diff since the last reviewed head touches only finding sites + their tests. Prose rewrites and new behaviour → separate PR.
- G1–G9 re-run against the fix diff only: re-run just the checks and tests the delta touches (`git diff <last-reviewed-head>..HEAD`), not the whole gate again unless the delta reaches outside what the last green run covered. Comment / doc lines the fix introduced get G2.

### G11 — vos checks
`N/A — no vos paths` unless the diff touches `crates/vos-*`, `apps/`, `client/`, `packages/`, `e2e/`, `types/generated` or the vos scripts. Otherwise run the section "vos checks" of `.claude/skills/pr-review/SKILL.md` (V1 tests and coverage, V2 accessibility, V3 boundaries and security, V4 UI structure and design, V5 the app readiness table when an app or what a person sees or does in one changed) and record each row PASS, FAIL or `N/A — <reason>` with command + output or `file:line`.
- The gate for vos paths is `bun run gate`, with e2e where the e2e rule requires it (CLAUDE.md, "The gate"); a required e2e not run = unverified = FAIL.
- A UI change carries a `verify-change` "Manual verification" section in the PR.

### Target shape — one-round PRs
- Small PRs, merged soon, over a long-lived branch.
- One defect / one concern per PR.
- Minimal diff. Description matches the diff exactly.
- Test drives the original defect's given/when/then and asserts the fixed behaviour.
- Pre-PR checks green at the reviewed SHA.
- Fix round: every finding fixed with its own test. Nothing else touched.

### Rubric maintenance
Same finding class raised on ≥ 2 PRs and no gate covers it → Jira ticket proposing the gate. Edit both copies of this rubric in one PR.
<!-- rubric:end -->

## Self-review note

Post on the PR. One note per head SHA.

```
## Self-review — <sha8>

Mode: fan-out (4× sonnet, 5× when vos paths) | single-reader | degraded — <reason>
Checks: pre-PR checks pass @ <sha8>
Diff: <files> files, +<add> / -<del>

| Gate | Result | Evidence |
|---|---|---|
| G1 Head & checks | PASS | `bun run gate` (or `scripts/check-neoc.sh`) → ok @ <sha8> |
| G2 Claims ↔ evidence | PASS | <claim> → `<cmd>` → <output> |
| ... | ... | ... |
| G10 Fix-round discipline | N/A — first request | |
| G11 vos checks | N/A — no vos paths | |

### Acceptance criteria
| AC | Proof | Result |
|---|---|---|

### Follow-ups (not blocking)
<NEOC-n — one line each> | none

### Unhappy paths
| Given | When | Then | Run | Result |
|---|---|---|---|---|

### Prior threads (re-request only)
| Thread | Fix SHA | Test | Given / when / then @ new head |
|---|---|---|---|
| <comment link> | <sha8> | <name> | <scenario> → fixed (exit 0) |
```

## Don't

- Mark a gate PASS without a command.
- Hand off before the pre-PR checks pass at the head SHA.
- Push after posting the note without re-running — note goes stale.
- Bundle prose rewrites or new behaviour into a fix round.
- `@mention` attributively — cite the comment by link. A mention spawns a pass.
