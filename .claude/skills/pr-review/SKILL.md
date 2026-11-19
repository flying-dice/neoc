---
name: pr-review
description: Use when you are the reviewer on a pull request — before approving, requesting changes, or posting review comments
---

Assigned as reviewer. No session context. Code speaks for itself. Author ran this rubric via `pr-self-review` — your job: prove their evidence, then find what they missed.

## Standard

Highest clean-code bar. No tradeoff, no mercy, no benefit of the doubt. Every defect — correctness or clean-code — named and blocked at the line. Brutal honesty: wrong is wrong, cite file/line/failure-mode, don't soften it. Resolve in-review or it does not merge. "Good enough" is a finding. The author's table is a claim, not evidence.

## Process

Cost-ordered. Phase 0–1 block → pass ends — no deep probing on a build a cheaper phase already failed. Phase 2–3 findings never end the pass — collect every finding, post once.

### Phase 0 — Dedupe
- Fetch PR head SHA and unresolved thread count. Compare with your last review note.
- Both identical and no new author replies → stop. No note. Not a pickup — no acknowledgement.
- Otherwise → acknowledgement comment, continue.

### Phase 1 — Prerequisites
1. Read PR description + linked Jira ticket (`gh pr view <n>`).
2. **Checks** at head SHA. GitHub Actions is not a gate and a PR has no checks: run the G1 local checks yourself and record them. Any check fails → one `[issue]` thread `Checks failing @ <sha8>`, hand back, stop.
3. **Self-review note** for the head SHA present. Absent or names an older SHA → one `[issue]` thread `No self-review @ <sha8>`, hand back, stop.

### Phase 2 — Evidence audit
- Re-derive ≥ 3 rows of the author's evidence yourself at head: one G5 test row, one G2 claim, one AC row. Re-run the command; compare output.
- Any mismatch → finding, then audit every row. Accept a finding, never someone else's evidence for it. Continue to Phase 3.
- Re-request: verify each prior thread against the new head → `RESOLVED` (resolve the thread via `gh api graphql` `resolveReviewThread`) or `STANDS` (reply with the measurement, keep open).

### Phase 3 — Adversarial rubric
1. Code locally only to read it or run scenarios the pre-PR checks don't — **detached worktree at the PR head SHA**, fetched from the PR ref (`<pr-head-sha>` = PR head `sha` recorded in Phase 0):
   ```sh
   git -C ~/Projects/<repo> fetch origin +refs/pull/<n>/head:refs/pr-review/<n>
   test "$(git -C ~/Projects/<repo> rev-parse --verify -q refs/pr-review/<n>)" = "<pr-head-sha>"
   git -C ~/Projects/<repo> worktree add --detach ~/Projects/.worktrees/<repo>/pr-<n> <pr-head-sha>
   ```
   `refs/pr-review/` sits outside `refs/remotes/origin/*` — a `fetch --prune origin` from any agent cannot delete it, no remote branch can clobber it. SHA mismatch → head moved; back to Phase 0. Never `origin/<pr-source-branch>` — absent when the source branch is deleted or lives in a fork. Never check the PR branch out by name — the author's agent may own it.
2. Run the rubric in reviewer mode, independently — do not start from the author's table. Fan-out per the rubric, `model: "opus"`.
3. Failed scenario → full given/when/then in an inline comment for the author to automate.
4. **Check gap** — a check you'd expect among the pre-PR checks is missing and you had to run it by hand to trust the change → raise a Jira ticket to add it. Don't let a manual run quietly paper over the gap.

### Phase 4 — Post
1. **Re-fetch head SHA.** Moved since Phase 0 → re-verify every finding against the new head. Mark each `STANDS` / `WITHDRAWN`; post only `STANDS`.
2. Every `file:line` from `grep -n` / `sed -n` at head, run in this pass.
3. Inline review comments. One thread per finding, anchored at the head SHA (`commit_id` = head). Read the stored comment back after posting.
4. Multi-line bodies as JSON payloads (`gh api`). GET the review thread back; confirm it landed.
5. Post the review comment.
6. Count unresolved review threads via `gh api graphql` (`reviewThreads { isResolved }`). Never trust the review decision alone.
7. Changes needed → threads stay open; hand back with a PR comment that **@mentions the PR assignee once**. Reference other comments by link, not `@name`.
8. Approved → **Approve the PR** — `gh pr review <n> --approve`. A comment is not approval.

<!-- rubric:begin -->
## Rubric

10 gates. Cost-ordered. Each gate: PASS / FAIL / `N/A — <reason>`. No blanks. Evidence = command + measured output at head SHA. Assertion without evidence = FAIL.

Stop rules:
- **Author mode** (`pr-self-review`): G1–G4 fail-fast — first FAIL → fix, push, restart at G1. G5–G10 run in full; fix every FAIL, push, restart at G1.
- **Reviewer mode** (`pr-review`): G1 FAIL → hand back, stop — no probing a red or stale head. G2–G10 run in full; post every finding in one pass. A partial review buys another round.

Local runs: run the pre-PR checks once at the head SHA (G1), then only what the rubric adds — unhappy-path probes, greps, link checks. A gate needing a check the pre-PR checks lack → run it by command and record the gap.

### Fan-out
`git diff --stat origin/<target>...HEAD` → > 50 lines changed OR > 3 files: one message, 4 parallel `Agent` calls, `subagent_type: "Explore"`, model as the skill pins. Read-only — never edit, push, or post. Otherwise single-reader: run every gate yourself.

| Agent | Gates |
|---|---|
| claims-vs-code | G2, G9 |
| test-validity | G5 |
| unhappy-paths | G6, G7 |
| docs-drift | G8 |

G1, G3, G4, G10: run yourself.

- Prompt each with: PR number, head SHA, worktree path, its gate text verbatim, return format `file:line — failure mode — evidence command + output`.
- Re-measure every sub-agent finding before acting on it. Not reproducible → record as such. Never drop silently.
- `Agent` tool absent or denied → fan-out NOT RUN. Run single-reader, record `Mode: degraded — <reason>`. Never present a single-reader pass as fan-out.

### G1 — Head & checks
GitHub Actions is not a gate. Run the pre-PR checks from CONTRIBUTING.md locally at the head SHA (`cargo fmt --check`, `RUSTFLAGS="-D warnings" cargo clippy --all-targets`, `cargo test --workspace`, the Lua suites, the bench smoke) and record command + output.
- Head SHA recorded: local `git rev-parse HEAD` == PR head `sha`.
- Every pre-PR check passes at that exact SHA. Any failure = FAIL.
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
- Formatter + lint for touched files pass — run by command (`cargo fmt --check`, `RUSTFLAGS="-D warnings" cargo clippy --all-targets`). Prose wrap matches the surrounding paragraph.
- Reference the Jira key (`refs NEOC-<n>`) only. `Closes` / `Fixes` / `Resolves` = FAIL.
- Breaking change (public API, language semantics, config key, CLI flag, wire format) → conventional `!` or `BREAKING CHANGE:` footer.
- No debug output, commented-out code, or TODO without a Jira ref.

### G5 — Test validity
- Every new public fn / branch / error arm → ≥ 1 test exercising it. Report count: new items vs tested items.
- Each regression test asserts the reported behaviour, read from the test body. An assertion that would still hold with the defect present (attribute-only, tautological, mocked-out subject) = FAIL. Judge tests by reading them — never by re-running them against altered code.
- Test reaches the call site the defect lived at — not a helper beside it, not a fixture that skips the branch.
- Deny / allow lists: assert an unlisted entry is rejected, not only that listed ones are.
- BDD: runner fails on undefined / skipped steps (`fail_on_skipped` or equivalent); summary shows skipped = 0. Count scenarios with `grep -cE '^\s*(Scenario|Scenario Outline):'`, compare names not totals.
- Fault injection: hook can clear mid-test; each guard driven on its own.
- Harness can express the failure:
  - Keyboard / focus / scroll / layout → real browser. jsdom: Space on a button is inert; `scrollHeight` / `clientHeight` = 0.
  - Playwright `click()` auto-scrolls → native `el.click()` for scroll assertions.
  - Signal races → in-process same-tick `process.kill`; external `kill(2)` never enters the window.
- Removed or weakened test → justified in the description. Removed to satisfy a done-check = FAIL.

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
- New or narrowed predicate / match arm / filter → list the shapes it now excludes; run each against the pre-fix and post-fix build; diff outputs. Valid case silenced = FAIL.
- Deleted read / query / guard → list every clause it carried (WHERE, LIMIT, permission check); probe the state each one excluded.
- Same function fixed twice → review its contract, not the symptom.

### G8 — Docs drift
- CONTRIBUTING, README, CLAUDE.md, rustdoc, CLI help affected by the behaviour → updated in the same PR.
- Every added or changed link resolves (`test -e` for paths, HTTP 200 for URLs).
- Docs describe the state after this PR, not a future one. Dependency on another PR → declared in the description and enforced: draft until the dependency merges, merge order at the top of the description. Declared but unenforced = FAIL.
- Grep the doc set for the changed term — every mention agrees. Self-contradiction = FAIL.

### G9 — Citations
- Every `file:line` posted comes from `grep -n` / `sed -n` at the head SHA in the same pass. No `cat -n` window offsets. No recall.

### G10 — Fix-round discipline (re-request only)
- Per prior thread: fix commit SHA + test name + the finding's given/when/then reproduced at the new head → fixed behaviour observed.
- Diff since the last reviewed head touches only finding sites + their tests. Prose rewrites and new behaviour → separate PR.
- G1–G9 re-run against the fix diff. Comment / doc lines the fix introduced get G2.

### Target shape — one-round PRs
- One defect / one concern per PR.
- Minimal diff. Description matches the diff exactly.
- Test drives the original defect's given/when/then and asserts the fixed behaviour.
- Pre-PR checks green at the reviewed SHA.
- Fix round: every finding fixed with its own test. Nothing else touched.

### Rubric maintenance
Same finding class raised on ≥ 2 PRs and no gate covers it → Jira ticket proposing the gate. Edit both copies of this rubric in one PR.
<!-- rubric:end -->

## Review note

Post on the PR. Every gate PASS, FAIL, or `N/A — <reason>`. No blanks.

```
## Review — <sha8> — APPROVED | CHANGES REQUESTED | HOLD

Mode: fan-out (4× opus) | single-reader | degraded — <reason>
Evidence audit: <n> rows re-derived, <m> mismatched
Prior threads: <r> resolved, <s> stand
New findings: <k>

- [ ] G1 Head & checks
- [ ] G2 Claims ↔ evidence
- [ ] G3 Acceptance criteria
- [ ] G4 Diff hygiene
- [ ] G5 Test validity
- [ ] G6 Unhappy paths
- [ ] G7 Narrowing & deletion audit
- [ ] G8 Docs drift
- [ ] G9 Citations
- [ ] G10 Fix-round discipline
```

## Comment format

```
**[type]** summary

detail
```

- `bug` — breaks at runtime.
- `issue` — wrong but won't crash (incl. any clean-code defect: SRP/DRY/naming/coupling/dead/KISS, and any failed gate).
- `question` — need answer before approving.

Every finding blocks. No severity scale. No non-blocking tier. No "nit". No defer-to-follow-up. One unresolved thread per finding. Approval forbidden while any thread is open. Name the gate in the summary: `**[issue]** G5 — regression test asserts the attribute, not the behaviour`.

## Focus

Weight unhappy paths. Happy path probably works — author tested it. Find where it falls apart.

Regressions. Edge cases. Error paths. Concurrency. Rollback safety. Interface changes without migration. Silent reverts. On re-requests: the fix diff itself — fixes introduce the next round's findings.

## Out of scope

Only what the linter already enforces — don't duplicate the tool. Targeted runs the rubric demands (unhappy-path probes) are in scope. Everything else with a clean-code or correctness basis is a finding and blocks, regardless of size.
