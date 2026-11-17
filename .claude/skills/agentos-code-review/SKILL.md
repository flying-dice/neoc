---
name: agentos-code-review
description: Use on every AgentOS merge request / pull request, and before calling any branch ready to merge. Reviews the change for what actually bites here (missing tests, stories and journey specs; accessibility; app boundary and SDK contract violations; sandbox and untrusted agent output; atomic layers, test ids and design-language conformance; clean code via clean-code-review), checks the app readiness gate (min window size, stories, journeys, verify-change, UX research round), and ends with a verdict of READY TO MERGE or STAYS IN MR.
---

# AgentOS code review

Nothing reaches `main` half-done. This review decides whether a change merges now or stays in its MR. Be concrete:
every finding names `file:line`, the failure it causes and the fix. Skip nitpicks and anything lint enforces.

## 0. Scope

```sh
gh pr view <n> --json title,body,headRefName,files   # or work from the branch
git diff main...HEAD --stat && git diff main...HEAD
```

Classify each touched area: **app** (`apps/<id>`, not desktop), **kit** (`packages/ui`: reaches every app), **SDK**
(`packages/sdk`: the contract; a `host/protocol` change touches both processes), **desktop** (host, shell, built-in
apps), **tooling** (scripts, e2e, CI, skills). Read the files around each change, not just the hunks.

For a diff over ~400 lines or more than 3 areas, give each of sections 2 to 5 to its own sub-agent with the diff and
file list (not your hypotheses), then merge and dedupe the findings yourself.

## 1. The gate

The PR's CI (`gh pr checks <n>`) or a local run must show all green on the head commit:

```sh
bun run lint && bun run typecheck && bun run test && bun run test:e2e
```

Red, missing, or run against an older commit means **STAYS IN MR**. If you couldn't run or see e2e, say so; it is
then unverified, not passed.

## 2. Tests and coverage

- New or changed logic in `src/shared` (or the SDK, or desktop `lib/`) has bun tests for success and expected
  failure, next to the code. Logic sitting in a page or `ctx.handle` handler where it can't be unit-tested is a
  finding.
- Every app screen and state has a story (`tags: ['app']`, inside `AppWindow`): empty, loading, error, full,
  agent-driven, and the minimum window. List the states the code can reach and compare with the stories.
- Every journey in `JOURNEYS.md` has a spec with the same id in `apps/<app>/e2e/`, driven by test ids. A new feature
  adds or updates a journey in the same MR.
- Interactions have `play` functions or journey assertions; a fixed bug has a regression check.
- Snapshot baselines changed: the PR shows the images were looked at. Any lowered threshold, skipped test or
  disabled axe rule is a blocker unless the PR justifies it.

## 3. Accessibility

The generic specs run axe (WCAG 2.1 AA) in light and dark on every story and app window; they don't catch:

- controls without an accessible name (icon-only buttons need `aria-label`), wrong or missing roles on custom
  controls, labels not tied to their inputs;
- keyboard paths: every journey is doable with Tab / Shift+Tab / Enter / Space / Escape / arrows, no traps,
  sheets and menus return focus to their trigger, focus ring visible (DESIGN_LANGUAGE §9);
- state shown by colour alone (selection, errors, status dots) or text that relies on a raw colour
  (`bun scripts/design-audit.ts <app>`); tokens only, so both themes hold AA;
- targets under 44 pt, text clipped at 125% text size or at the minimum window;
- live updates (agent replies, results pushed by tools) that a screen reader never hears.

## 4. Boundaries and security

`bun run lint:boundaries` enforces the import rules (`scripts/check-boundaries.ts`). Review what it can't see:

- The SDK is the only contract. A new host capability changes `AppMainContext` / `AppRendererApi` or
  `host/protocol` with a zod schema, is implemented in the desktop host **and** the memory host, and is tested.
  Anything else (globals, `postMessage`, sniffing `window`, duplicated channel strings) is a blocker.
- `ctx.handle` payloads are `unknown` and are parsed before use; tool inputs have described zod shapes;
  `ctx.storage.get<T>` and `api.tools.call<T>` results are parsed where the code relies on their shape.
- Agent and tool output (ACP messages, tool results, permission prompts) is untrusted: rendered as text, never as
  HTML, URLs, file paths or commands without validation. Read-only tools (`readOnly: true`) really are read-only:
  an agent calls them without approval.
- Sandbox and IPC: `webPreferences`, navigation and window-open handlers, `trustedFrames`, and the MCP server
  (127.0.0.1, bearer token) are not weakened. New external links open only `http(s)`.
- Apps reach other apps only through tools; the desktop knows apps only through the registries.

## 5. UI structure, design and clean code

- Atomic placement makes sense (an organism isn't a page in disguise), the kit is used before a one-off, and a
  generic component goes into `packages/ui` with a story. Test ids follow `docs/TESTING.md` naming (stable ids,
  never indices or display text).
- Design language: check the screenshots in the PR (or baselines) against DESIGN_LANGUAGE §11.
- A UI change has a "Manual verification" section from `verify-change` with screenshots of the states it claims.
- Run the `clean-code-review` skill on the diff; carry over its findings above 0.5 at their severity.

## 6. Readiness gate (apps)

Applies to a new app, and to any change that alters what a person sees or does in an app. Every row needs evidence;
one missing row means **STAYS IN MR**.

| Requirement | Evidence |
|---|---|
| Window size | `manifest.window` has `width`/`height`, and `minWidth`/`minHeight` with a comment saying why, when the layout needs more than 320x240 |
| Stories | a story for every screen and state (section 2), green in the stories project |
| Journeys | `JOURNEYS.md` with 4 to 8 journeys (goal, start state, observable "Done when"), each with a passing spec of the same id |
| Wiring | both `installedApps.ts` registries, `apps/desktop/package.json`, `electron.vite.config.ts` `workspacePackages`; main module browser-safe or `src/mock/main.ts` |
| Verified | the PR's "Manual verification" section (`verify-change`) covers light and dark, the minimum and large windows, Split View / Slide Over and keyboard |
| Researched | `docs/ux/<app>/round-<n>.md` from the `app-design` skill whose scorecard meets the exit criteria (all journeys succeed for all three participants, mean ease ≥ 6.0, no blocker or major open, conformance ≥ 90 with no category below 70), and no layout or flow change landed after that round |

Changes to an app that don't alter what people see or do (a pure-logic fix, a new read-only tool) need the gate,
the tests and green journeys, but not a new research round.

## 7. Report

```md
## AgentOS review: <PR title or branch>

**Verdict: READY TO MERGE** | **Verdict: STAYS IN MR** (one line on why)

Gate: lint ✓ · typecheck ✓ · test ✓ · e2e ✓ (CI run <link> on <sha> | local)
Readiness: <app>: window ✓ · stories ✓ · journeys ✓ · wiring ✓ · verified ✗ · researched ✗ (no docs/ux/<app>/)

### Findings
1. **Blocker**, `apps/notes/src/main/index.ts:42`: `ctx.handle('save')` writes `payload` to storage unparsed.
   Failure: a renderer sends `{ title: 5 }`; the next load renders `undefined` and the list crashes on `.trim()`.
   Fix: parse with the `Note` schema from `src/shared` and return a validation result.
2. **Major**, …

### Not checked
- e2e not run locally (no macOS runner): relied on CI run <link>.
```

Severities: **blocker** (a bug, data loss, security or boundary violation, a missing gate or readiness item, an
accessibility failure that stops someone finishing a journey); **major** (new behaviour without a check, an
accessibility or design-language defect people will hit, a PANIC finding with a named trigger); **minor**
(worth a follow-up issue, doesn't block).

**READY TO MERGE** only when the gate is green on the head commit, the readiness gate is complete for every app
change, and there are no blocker or major findings. Otherwise **STAYS IN MR**, with the list of what must change.
Post the review on the PR (`gh pr comment <n> --body-file …`) only when asked.
