---
name: clean-code-review
description: Clean-code audit of an AgentOS change (the current diff or a branch) against SRP, DRY, naming, coupling, dead code, KISS, BOUNDARY (the SDK, process and atomic seams) and PANIC (unchecked casts, non-null assertions, unvalidated IPC, agent or tool input). Fixes what this session introduced and tags the rest as scored TODO markers. Use after implementing a change and before the pre-commit skill; agentos-code-review delegates its clean-code pass here.
---

# Clean-code review

## Scope

Review the changed files (`git diff main...HEAD`, plus uncommitted work) and the code they touch directly. Skip
test boilerplate, framework-required shapes (Storybook CSF, Playwright fixtures, `defineMainApp`) and pre-existing
issues outside the diff. Lint already covers formatting, atomic layers, test ids and the mechanical boundary
rules; don't report what `bun run lint` reports.

More than 50 changed lines or more than 3 files: run one sub-agent per principle group (1 to 3, 4 to 6, 7, 8), each
given the diff and the file list, not your conclusions. Otherwise one inline pass.

## Principles

1. **SRP**: a function or component doing two jobs; a page mixing IPC, parsing and layout; a `ctx.handle` handler
   holding logic that belongs in `src/shared`, where it could be unit-tested.
2. **DRY**: copy-pasted blocks, constants or near-identical components, especially a second app re-implementing a
   control the kit has or should have.
3. **NAMING**: names that hide intent (`data`, `handle`, `manager`, `util`); channel, tool and test-id names that
   break the conventions (`<appId>__<tool>`, `docs/TESTING.md` test ids).
4. **COUPLING**: concrete dependencies built inline instead of injected (`AppHost`, `ToolRegistry` and the emitter
   are injected in the desktop); module-level mutable state outside a `*.svelte.ts` store.
5. **DEAD**: unused exports, props, handlers, tools or stories; unreachable branches; commented-out code.
6. **KISS**: abstraction without a second user, premature options, `$effect` where `$derived` does. Ask why up to
   five times; if no answer survives, it is a violation.
7. **BOUNDARY**: coupling across a real AgentOS seam that lint can't see. Seams: app ↔ host (only the SDK), client
   ↔ server (only `invoke` / `handle` / `emit` and `host/protocol` over JSON-RPC), app ↔ app (only tools), agent ↔
   host (ACP and MCP, untrusted), registry ↔ server (bundles, sha256-verified), kit ↔ app (the kit is generic),
   atomic layers. Flag: server or Bun types leaking into client-facing SDK signatures; stringly-typed methods shared
   by copy instead of one constant; a protocol change without its zod schema or memory-host twin; app-specific
   knowledge in the client or server (apps arrive as bundles); app knowledge in `packages/ui`; a server module that
   opens something its dispose doesn't release.
8. **PANIC**: a reachable crash or silent corruption from outside input. In TypeScript: `as T` over `unknown`
   (IPC payloads, `JSON.parse`, `ctx.storage.get<T>`, `api.tools.call<T>`, ACP notifications); a non-null `!`;
   indexing assumed defined; a throw inside an `api.on` listener or `$effect` that nothing catches; a fallible
   promise fired with `void` and no catch; `{@html}` or `innerHTML` with agent or tool text.

## Gates on BOUNDARY and PANIC

- **BOUNDARY** needs a real seam: name the thing that changes independently (the host, another app, the agent, the
  protocol version). If you can't, it is not a finding (KISS).
- **PANIC** needs a named triggering input from outside: a renderer message, an agent or tool payload, a stored
  value from an older version, a file, the network. Tests are exempt. A cast or `!` guarded by an invariant
  established just above is fine; ask for a comment stating the invariant.

## Report and tag

Report each finding with file, line range, principle, a one-line description and a severity from 0 to 1. Above
0.5: fix the ones this session introduced now; tag the rest where they occur:

```ts
// TODO: clean-code - 0.7 - PANIC: storage.get<HistoryEntry[]> trusts data an older version wrote; parse with zod
```

In Svelte markup use `<!-- TODO: clean-code - <score> - <PRINCIPLE>: … -->`. The `refactor` skill clears markers
one at a time; `pre-commit` blocks on any above 0.5.
