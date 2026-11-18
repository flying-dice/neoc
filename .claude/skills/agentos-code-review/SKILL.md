---
name: agentos-code-review
description: Use on every AgentOS merge request / pull request, and before calling any branch ready to merge. Reviews the change for what actually bites here (missing tests, stories and journey specs; accessibility; crate-direction, Luau sandbox, permission and protocol violations; untrusted agent output; atomic layers, test ids and design-language conformance; clean code via clean-code-review), checks the app readiness gate (min window size, tests, journeys, verify-change, UX research round), and ends with a verdict of READY TO MERGE or STAYS IN MR.
---

# AgentOS code review

Nothing reaches `main` half-done. This review decides whether a change merges now or stays in its MR. Be concrete:
every finding names `file:line`, the failure it causes and the fix. Skip nitpicks and anything lint enforces.

## 0. Scope

```sh
gh pr view <n> --json title,body,headRefName,files   # or work from the branch
git diff main...HEAD --stat && git diff main...HEAD
```

Classify each touched area: **host** (`crates/*`: domain, application ports and use cases, adapters, the composition
root; a protocol or schema change reaches the client and every app), **app** (`apps/<id>`, Luau), **kit** (`packages/ui`:
reaches every app), **client** (`client/`: the shell and the renderer), **generated** (`types/generated`,
`docs/reference`: must match `bun run codegen`), **tooling** (scripts, e2e, skills, docs). Read the files around each
change, not just the hunks.

For a diff over ~400 lines or more than 3 areas, give each of sections 2 to 5 to its own sub-agent with the diff and
file list (not your hypotheses), then merge and dedupe the findings yourself.

## 1. The gate

There is no hosted CI. Run the gate yourself on the PR's head commit (`gh pr checkout <n>`) and see it all green:

```sh
bun run lint && bun run typecheck && bun run test && bun run test:e2e
```

Red, missing, or run against an older commit means **STAYS IN MR**. If you couldn't run or see e2e (it needs a Mac for the
baselines), say so; it is then unverified, not passed. A `SKIPPED` from `lint:luau` or `lint:stylua` (tool not installed)
is not a pass: install the tool and rerun.

## 2. Tests and coverage

- New or changed Rust logic has a `cargo nextest` test in its layer, for success and expected failure: domain unit tests,
  use cases against the port fakes, adapter integration tests, and a host test when the change crosses layers. Logic sitting
  in an adapter, a Luau `render` or handler, or a Svelte page where it can't be unit-tested is a finding. New client or kit
  logic has bun tests next to it.
- Every new screen and state is covered: a journey that reaches it on the real host, or a story (a kit component in its own
  story, a renderer or shell state as a JSON-tree story on the fake host). List the states the code can reach (empty,
  loading, error, full, agent-driven, offline, the minimum window) and compare.
- Every journey in `JOURNEYS.md` has a spec with the same id in `apps/<app>/e2e/` (or `client/e2e`, `e2e/*.journeys.spec.ts`),
  driven by test ids. A new feature adds or updates a journey in the same MR.
- Interactions have `play` functions or journey assertions; a fixed bug has a regression check at the layer it lived in.
- Snapshot baselines changed: the PR shows the images were looked at. Any lowered threshold, skipped test or
  disabled axe rule is a blocker unless the PR justifies it.

## 3. Accessibility

The generic specs run axe (WCAG 2.1 AA) in light and dark on every story and app window; they don't catch:

- controls without an accessible name (icon-only buttons need a `label` in the tree or `aria-label` in the kit), wrong or
  missing roles on custom controls, labels not tied to their inputs;
- keyboard paths: every journey is doable with Tab / Shift+Tab / Enter / Space / Escape / arrows, no traps,
  sheets and menus return focus to their trigger, focus ring visible (DESIGN_LANGUAGE §9);
- state shown by colour alone (selection, errors, status dots) or text that relies on a raw colour
  (`bun scripts/design-audit.ts <page>` for shell pages; apps only use tokens); both themes hold AA;
- targets under 44 pt, text clipped at 125% text size or at the minimum window;
- live updates (agent replies, results pushed by tools) that a screen reader never hears.

## 4. Boundaries and security

`bun run lint:deps` and `cargo deny check` enforce crate direction and framework placement. Review what they can't see:

- **Hexagon.** Policy sits in the right layer: rules about apps, permissions, grants and tools are domain or use cases,
  not adapters; a new outside need is a port with a fake, then an adapter; an adapter doesn't reach another adapter; no
  adapter or framework type leaks into a port signature. A new crate is added to `scripts/check-deps.ts` and `deny.toml`
  on purpose.
- **Missing capability = extend the host API.** A new capability changes the domain type and schema, the application port
  and use case, the adapter, and the generated output (`bun run codegen`, `codegen:check` green), with tests in each layer.
  Anything else (a new route, a raw socket, a global, a flag read from the environment inside an app) is a blocker.
- **Rust hygiene.** No `unwrap`, `expect`, `panic!` or `unsafe` outside tests (clippy denies them; a local `allow` needs a
  stated reason). External input (JSON-RPC params, tool arguments, app output, ACP messages, app refs) is parsed into typed
  values and errors are typed (`DomainError`, `PortError`, `AppError`); `anyhow` stays in the host binary.
- **App isolation (no permissions, no budgets).** A new `app.*` capability is typed in the generated definitions;
  nothing gives an app another app's data or state (VMs, environments and module tables are per app); SQL `ATTACH`
  stays denied; a VM, connection, watcher, listener, task or worker an adapter or a library module opens is
  registered with the app's `Life` and released on unload or replace. A Lua callback that nobody called (a request
  handler, a task slice) runs through the VM's call queue. A tool declared `readOnly` really writes nothing (the data
  layer enforces it; check the tool's code agrees), and one that writes isn't marked `readOnly`.
- **Agents.** Tool calls carry the agent's identity and are checked against its grants before the VM runs (deny by
  default); `/mcp` accepts only per-agent bearer tokens (a device session is not one); tokens are stored hashed and shown
  once. Agent and tool output (ACP messages, tool results, permission prompts, `/agui` events) is untrusted: rendered as
  text, never as HTML, URLs, file paths or commands without validation, and parsed before use in the client.
- **Network surface.** Every route except the static client, `/pair` and `/session` needs a paired device, including from
  loopback; the Origin/Host guard covers every route; pairing codes stay single-use, short-lived and rate limited; app-ref
  files are sha256-verified and size-capped before install; the add-by-URL preview runs nothing; TLS and cookie flags are
  not weakened. New external links open only `http(s)`.
- **The client runs no app code.** No app-specific knowledge in `client/` or `packages/ui`; nothing executable crosses the
  wire; the client writes to the host only through the protocol (Bindings, events).
- Apps reach other apps only through tools.

## 5. UI structure, design and clean code

- Atomic placement makes sense (an organism isn't a page in disguise), the kit is used before a one-off, and a
  generic component goes into `packages/ui` with a story. Renderer adapters wrap kit components and take their props
  from the Rust schema. Test ids follow `docs/TESTING.md` naming (stable ids from node keys, never indices or display text).
- Design language: check the screenshots in the PR (or baselines) against DESIGN_LANGUAGE §11.
- A UI change has a "Manual verification" section from `verify-change` with screenshots of the states it claims.
- Run the `clean-code-review` skill on the diff; carry over its findings above 0.5 at their severity.

## 6. Readiness gate (apps)

Applies to a new app, and to any change that alters what a person sees or does in an app. Every row needs evidence;
one missing row means **STAYS IN MR**.

| Requirement | Evidence |
|---|---|
| Window size | `app.manifest.window` has `w`/`h` and `minW`/`minH`, with a comment saying why when the layout needs more than 320x240; the `desktop` project's resize check passes |
| Package | `app.json` is valid and agrees with `app.manifest` (id, icon, tint), `version` bumped for a change to a released app; `bun run build:apps` builds it, and it installs from Discover, opens, works and uninstalls cleanly on a real host (`bun run dev`) |
| Types and format | `bun run lint:luau` and `bun run lint:stylua` pass (not skipped) and the app is `--!strict` |
| Tests | the app's logic and tools are covered from Rust (`crates/agentos-lua/tests`, or `crates/agentos-host/tests` for views, data and agents), including `readOnly` behaviour |
| States | every screen and state is reached by a journey or a JSON-tree story (section 2), green in the stories, desktop and journeys projects |
| Journeys | `JOURNEYS.md` with 4 to 8 journeys (goal, start state, observable "Done when"), each with a passing spec of the same id |
| Verified | the PR's "Manual verification" section (`verify-change`) covers light and dark, the minimum and large windows, Split View / Slide Over and keyboard |
| Researched | `docs/ux/<app>/round-<n>.md` from the `app-design` skill whose scorecard meets the exit criteria (all journeys succeed for all three participants, mean ease ≥ 6.0, no blocker or major open, conformance ≥ 90 with no category below 70), and no layout or flow change landed after that round |

Changes to an app that don't alter what people see or do (a pure-logic fix, a new read-only tool) need the gate,
the tests and green journeys, but not a new research round.

## 7. Report

```md
## AgentOS review: <PR title or branch>

**Verdict: READY TO MERGE** | **Verdict: STAYS IN MR** (one line on why)

Gate: lint ✓ · typecheck ✓ · test ✓ · e2e ✓ (local, on <sha>)
Readiness: <app>: window ✓ · package ✓ · types ✓ · tests ✓ · states ✓ · journeys ✓ · verified ✗ · researched ✗ (no docs/ux/<app>/)

### Findings
1. **Blocker**, `crates/agentos-lua/src/api/misc.rs:153`: `app.agents:list` returns the agents' launch commands.
   Failure: an app can read what only the host may see.
   Fix: return `{ id, name, avatar, role }` only, plus a test that the roster rows carry nothing else.
2. **Major**, …

### Not checked
- e2e not run (not on a Mac): the gate is unverified, so the verdict is STAYS IN MR.
```

Severities: **blocker** (a bug, data loss, security, sandbox, permission or boundary violation, a missing gate or readiness
item, an accessibility failure that stops someone finishing a journey); **major** (new behaviour without a check, an
accessibility or design-language defect people will hit, a PANIC finding with a named trigger); **minor**
(worth a follow-up issue, doesn't block).

**READY TO MERGE** only when the gate is green on the head commit, the readiness gate is complete for every app
change, and there are no blocker or major findings. Otherwise **STAYS IN MR**, with the list of what must change.
Post the review on the PR (`gh pr comment <n> --body-file …`) only when asked.
