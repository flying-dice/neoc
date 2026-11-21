---
name: pr-review
description: Use when you are the reviewer on a pull request, neoc or vos — before giving a verdict (READY TO MERGE, CHANGES NEEDED or HOLD), requesting changes, or posting review comments. Runs the 11-gate rubric; G11 applies the vos checks (tests, stories and journeys, accessibility, boundaries, design, app readiness) when the diff touches vos paths.
---

Assigned as reviewer: a fresh reviewer with no session context, independent of the author. Code speaks for itself. Author ran this rubric via `pr-self-review` — your job: prove their evidence, then find what they missed.

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
8. Clean → post the review note with verdict **READY TO MERGE** at the head SHA. The author cannot approve their own PR and nothing reads GitHub approvals: the squash-merge requires this note, for the exact head SHA.

<!-- rubric:begin -->
## Rubric

11 gates. Cost-ordered. Each gate: PASS / FAIL / `N/A — <reason>`. No blanks. Evidence = command + measured output at head SHA. Assertion without evidence = FAIL.

Stop rules:
- **Author mode** (`pr-self-review`): G1–G4 fail-fast — first FAIL → fix, push, restart at G1. G5–G11 run in full; fix every FAIL, push, restart at G1.
- **Reviewer mode** (`pr-review`): G1 FAIL → hand back, stop — no probing a red or stale head. G2–G11 run in full; post every finding in one pass. A partial review buys another round.

Local runs: run the pre-PR checks once at the head SHA (G1), then only what the rubric adds — unhappy-path probes, greps, link checks. A gate needing a check the pre-PR checks lack → run it by command and record the gap.

### Fan-out
`git diff --stat origin/<target>...HEAD` → > 50 lines changed OR > 3 files: one message, 4 parallel `Agent` calls (5 when the diff touches vos paths, see G11), `subagent_type: "Explore"`, model as the skill pins. Read-only — never edit, push, or post. Otherwise single-reader: run every gate yourself.

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
GitHub Actions is not a gate. Run the pre-PR checks from CONTRIBUTING.md locally at the head SHA and record command + output: `bun run gate` (the neoc checks, then the vos checks: lint, typecheck, test, test:e2e), or `scripts/check-neoc.sh` for a change confined to the neoc crates (the scoped variant, see `pre-commit`). A lint that printed `SKIPPED`, or an e2e you could not run (it needs a Mac), is unverified, not passed.
- Head SHA recorded: local `git rev-parse HEAD` == PR head `sha`.
- Every pre-PR check passes at that exact SHA. Any failure = FAIL. The scoped variant on a diff that touches vos paths = FAIL.
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
- CONTRIBUTING, README, `apps/README.md`, CLAUDE.md, rustdoc, CLI help, and the skills or workflows that name the behaviour → updated in the same PR.
- Every added or changed link resolves (`test -e` for paths, HTTP 200 for URLs).
- Docs describe the state after this PR, not a future one. Dependency on another PR → declared in the description and enforced: draft until the dependency merges, merge order at the top of the description. Declared but unenforced = FAIL.
- Grep the doc set for the changed term — every mention agrees. Self-contradiction = FAIL.

### G9 — Citations
- Every `file:line` posted comes from `grep -n` / `sed -n` at the head SHA in the same pass. No `cat -n` window offsets. No recall.

### G10 — Fix-round discipline (re-request only)
- Per prior thread: fix commit SHA + test name + the finding's given/when/then reproduced at the new head → fixed behaviour observed.
- Diff since the last reviewed head touches only finding sites + their tests. Prose rewrites and new behaviour → separate PR.
- G1–G9 re-run against the fix diff. Comment / doc lines the fix introduced get G2.

### G11 — vos checks
`N/A — no vos paths` unless the diff touches `crates/vos-*`, `apps/`, `client/`, `packages/`, `e2e/`, `types/generated` or the vos scripts. Otherwise run the section "vos checks" of `.claude/skills/pr-review/SKILL.md` (V1 tests and coverage, V2 accessibility, V3 boundaries and security, V4 UI structure and design, V5 the app readiness table when an app or what a person sees or does in one changed) and record each row PASS, FAIL or `N/A — <reason>` with command + output or `file:line`.
- The gate for vos paths is the full `bun run gate`; e2e not run = unverified = FAIL.
- A UI change carries a `verify-change` "Manual verification" section in the PR.

### Target shape — one-round PRs
- One defect / one concern per PR.
- Minimal diff. Description matches the diff exactly.
- Test drives the original defect's given/when/then and asserts the fixed behaviour.
- Pre-PR checks green at the reviewed SHA.
- Fix round: every finding fixed with its own test. Nothing else touched.

### Rubric maintenance
Same finding class raised on ≥ 2 PRs and no gate covers it → Jira ticket proposing the gate. Edit both copies of this rubric in one PR.
<!-- rubric:end -->

## vos checks (when the diff touches vos)

Run for gate G11 whenever the diff touches `crates/vos-*`, `apps/`, `client/`, `packages/`, `e2e/`, `types/generated` or
the vos scripts. Classify each touched area, then read the files around each change, not just the hunks: **host**
(`crates/vos-*`: domain, application ports and use cases, adapters, the composition root; a protocol or schema change
reaches the client and every app), **app** (`apps/<id>`, Luau), **kit** (`packages/ui`: reaches every app), **client**
(`client/`: the shell and the renderer), **generated** (`types/generated`: must match `bun run codegen`), **tooling**
(scripts, e2e, skills). Every finding names `file:line`, the failure it causes and the fix; skip what lint enforces. The gate
itself is G1 (`bun run gate`).

**V1. Tests and coverage**
- New or changed Rust logic has a `cargo nextest` test in its layer, for success and expected failure: domain unit tests, use
  cases against the port fakes, adapter integration tests, and a host test when the change crosses layers. Logic sitting in
  an adapter, a Luau `render` or handler, or a Svelte page where it can't be unit-tested is a finding. New client or kit
  logic has bun tests next to it.
- Every new screen and state is covered: a journey that reaches it on the real host, or a story (a kit component in its own
  story, a renderer or shell state as a JSON-tree story on the fake host). List the states the code can reach (empty,
  loading, error, full, agent-driven, offline, the minimum window) and compare.
- Every journey in `JOURNEYS.md` has a spec with the same id in `apps/<app>/e2e/` (or `client/e2e`,
  `e2e/*.journeys.spec.ts`), driven by test ids. A new feature adds or updates a journey in the same PR.
- Interactions have `play` functions or journey assertions; a fixed bug has a regression check at the layer it lived in.
- Snapshot baselines changed: the PR shows the images were looked at. Any lowered threshold, skipped test or disabled axe
  rule is a finding unless the PR justifies it.

**V2. Accessibility** (the generic specs run axe WCAG 2.1 AA in light and dark on every story and app window; they don't catch)
- controls without an accessible name (icon-only buttons need a `label` in the tree or `aria-label` in the kit), wrong or
  missing roles on custom controls, labels not tied to their inputs;
- keyboard paths: every journey is doable with Tab, Shift+Tab, Enter, Space, Escape and arrows, no traps, sheets and menus
  return focus to their trigger, the focus ring is visible (styleguide section 9);
- state shown by colour alone (selection, errors, status dots) or text that relies on a raw colour (`bun scripts/design-audit.ts
  <page>` for shell pages; apps only use tokens); both themes hold AA;
- targets under 44 pt, text clipped at 125% text size or at the minimum window;
- live updates (agent replies, results pushed by tools) that a screen reader never hears.

**V3. Boundaries and security** (`bun run lint:deps` and `cargo deny check` enforce crate direction and framework placement)
- **Hexagon.** Policy sits in the right layer: rules about apps, grants and tools are domain or use cases, not adapters; a new
  outside need is a port with a fake, then an adapter; an adapter doesn't reach another adapter; no adapter or framework type
  leaks into a port signature. A new crate is added to `scripts/check-deps.ts` and `deny.toml` on purpose.
- **Missing capability means extend the host API.** A new capability changes the domain type and schema, the application
  port and use case, the adapter, and the generated output (`bun run codegen`, `codegen:check` green), with tests in each
  layer. A new route, raw socket or global is a finding. (An app reading the environment through `std:env` is by design:
  apps get every `std:`/`lib:`/`vnd:` module.)
- **Rust hygiene.** No `unwrap`, `expect`, `panic!` or `unsafe` outside tests (clippy denies them; a local `allow` needs a
  stated reason). External input (JSON-RPC params, tool arguments, app output, ACP messages, app refs) is parsed into typed
  values and errors are typed (`DomainError`, `PortError`, `AppError`); `anyhow` stays in the host binary.
- **neoc-vm stays vos-free.** The vos adapter embeds `neoc-vm`; vos policy (apps, `readOnly`, views, `app.*`, the `vos:*`
  modules) stays in `crates/vos-lua`. A new thread of the host's own that calls into Lua has `neoc_vm::VM_THREAD_STACK`.
- **App state hygiene (no permissions, no budgets, no security boundary).** A new `app.*` capability is typed in the generated
  declarations; no `app.*` function gives an app another app's data or state (VMs, environments and module tables are per
  app; that is state hygiene: `std:fs` and `vnd:rusqlite` reach everything, so do not review for isolation the modules do not
  have); SQL `ATTACH` stays denied through `app.db`; a Lua number that reaches Rust (a size, a sleep, an index) is bounded or
  an error, never a panic (release aborts on one); a VM, connection, watcher, listener, task or worker an adapter or a
  library module opens is registered with the app's `Life` and released on unload or replace. A Lua callback that nobody
  called (a request handler, a task slice) runs through the VM's call queue. A tool declared `readOnly` writes nothing
  through the `app.*` API (the read-only flag is inherited by the tasks and requests the call starts; the neoc modules are not
  covered), and one that writes isn't marked `readOnly`.
- **Agents.** Tool calls carry the agent's identity and are checked against its grants before the VM runs (deny by default);
  `/mcp` accepts only per-agent bearer tokens (a device session is not one); tokens are stored hashed and shown once. Agent
  and tool output (ACP messages, tool results, permission prompts, `/agui` events) is untrusted: rendered as text, never as
  HTML, URLs, file paths or commands without validation, and parsed before use in the client.
- **Network surface.** Every route except the static client, `/pair` and `/session` needs a paired device, including from
  loopback; the Origin/Host guard covers every route; pairing codes stay single-use, short-lived and rate limited; app-ref
  files are sha256-verified and size-capped before install; the add-by-URL preview runs nothing; TLS and cookie flags are
  not weakened. New external links open only `http(s)`.
- **The client runs no app code.** No app-specific knowledge in `client/` or `packages/ui`; nothing executable crosses the
  wire; the client writes to the host only through the protocol (Bindings, events). Apps reach other apps only through tools.

**V4. UI structure, design and clean code**
- Atomic placement makes sense (an organism isn't a page in disguise), the kit is used before a one-off, and a generic
  component goes into `packages/ui` with a story. Renderer adapters wrap kit components and take their props from the Rust
  schema. Test ids come from stable node keys, never indices or display text.
- Design: check the screenshots in the PR (or the baselines) against the styleguide checklist
  (`.claude/skills/app-design/STYLEGUIDE.md` section 11).
- A UI change has a "Manual verification" section from `verify-change` with screenshots of the states it claims.
- Run `clean-code-review` on the diff and carry over its findings above 0.5.

**V5. Readiness gate (apps).** Applies to a new app and to any change that alters what a person sees or does in an app. Every
row needs evidence; one missing row is a finding, and the app stays in its PR.

| Requirement | Evidence |
|---|---|
| Window size | `app.manifest.window` has `w`/`h` and `minW`/`minH`, with a comment saying why when the layout needs more than 320x240; the `desktop` project's resize check passes |
| Package | `app.json` is valid and agrees with `app.manifest` (id, icon, tint), `version` bumped for a change to a released app; `bun run build:apps` builds it, and it installs from Discover, opens, works and uninstalls cleanly on a real host (`bun run dev`) |
| Types and format | `bun run lint:luau` and `bun run lint:stylua` pass (not skipped) |
| Tests | the app's logic and tools are covered from Rust (`crates/vos-lua/tests`, or `crates/vos-host/tests` for views, data and agents), including `readOnly` behaviour |
| States | every screen and state is reached by a journey or a JSON-tree story (V1), green in the stories, desktop and journeys projects |
| Journeys | `JOURNEYS.md` with 4 to 8 journeys (goal, start state, observable "Done when"), each with a passing spec of the same id |
| Verified | the PR's "Manual verification" section (`verify-change`) covers light and dark, the minimum and large windows, Split View and Slide Over, and keyboard |
| Researched | `ux/<app>/round-<n>.md` from the `app-design` skill whose scorecard meets the exit criteria (all journeys succeed for all three participants, mean ease at least 6.0, no blocker or major open, conformance at least 90 with no category below 70), and no layout or flow change landed after that round |

A change to an app that doesn't alter what people see or do (a pure-logic fix, a new read-only tool) needs the gate, the
tests and green journeys, but not a new research round.

For an app whose earlier research rounds predate NEOC-115 (the Calculator's reports were dropped with `docs/`; they are in git at `51f47f4`), the next change to what a person sees or does starts a fresh round under `ux/<app>/`.

Severity: a bug, data loss, a host panic or stack overflow reachable from app input, a boundary violation, a missing gate or
readiness row, or an accessibility failure that stops a journey is a `bug`; new behaviour without a check, or an
accessibility or design defect people will hit, is an `issue`. Every finding blocks.

## Review note

Post on the PR. Every gate PASS, FAIL, or `N/A — <reason>`. No blanks.

```
## Review — <sha8> — READY TO MERGE | CHANGES NEEDED | HOLD

Mode: fan-out (4× opus, 5× when vos paths) | single-reader | degraded — <reason>
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
- [ ] G11 vos checks

Readiness (an app changed): <app>: window ✓ · package ✓ · types ✓ · tests ✓ · states ✓ · journeys ✓ · verified ✗ · researched ✗ (no ux/<app>/)
```

## Comment format

```
**[type]** summary

detail
```

- `bug` — breaks at runtime.
- `issue` — wrong but won't crash (incl. any clean-code defect: SRP/DRY/naming/coupling/dead/KISS, and any failed gate).
- `question` — need answer before a READY TO MERGE verdict.

Every finding blocks. No severity scale. No non-blocking tier. No "nit". No defer-to-follow-up. One unresolved thread per finding. READY TO MERGE is forbidden while any thread is open. Name the gate in the summary: `**[issue]** G5 — regression test asserts the attribute, not the behaviour`.

## Focus

Weight unhappy paths. Happy path probably works — author tested it. Find where it falls apart.

Regressions. Edge cases. Error paths. Concurrency. Rollback safety. Interface changes without migration. Silent reverts. On re-requests: the fix diff itself — fixes introduce the next round's findings.

## Out of scope

Only what the linter already enforces — don't duplicate the tool. Targeted runs the rubric demands (unhappy-path probes) are in scope. Everything else with a clean-code or correctness basis is a finding and blocks, regardless of size.
