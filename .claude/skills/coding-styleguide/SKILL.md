---
name: coding-styleguide
description: Use while writing or changing any AgentOS TypeScript or Svelte code (an app module, packages/sdk, packages/ui, the desktop host, scripts). The house rules for where code belongs (the SDK as the port layer, apps as adapters, desktop as composition root, atomic UI layers), types and expected errors, zod at trust boundaries, and Svelte 5 runes. Read references/typescript-svelte.md for the language details.
---

# AgentOS coding styleguide

Read the neighbouring code and its tests first and match them. Apply these rules to the code you are changing;
don't migrate unrelated APIs, add dependencies or change configuration on the way.

## Where code belongs

AgentOS is hexagonal: a PWA client and a Bun server, with apps installed as bundles at runtime. The SDK is the port
layer, so nothing crosses a process or package seam except through it.

| Role | AgentOS |
|---|---|
| Ports (contracts) | `packages/sdk`: `app/server` (`AppServerContext`), `app/client` (`AppClientApi`), `host/protocol` (every JSON-RPC method with its zod schema), `bundle` (bundle.json and known-apps formats), tool types |
| Domain | `apps/<id>/src/shared`: pure, synchronous, no framework, no SDK runtime; bun-tested |
| Driving adapters | an app's `src/client` (calls `useAppApi()`), its tools (`ctx.registerTool`) |
| Driven adapters | `apps/server/src` (app runtime, known apps, tool registry, MCP endpoint, ACP agent host, SQLite); an app's `src/server` (its own SQLite, Bun APIs) |
| Composition roots | `apps/server/src/server.ts` (`startServer`), the client's `boot.ts` / `web/main.ts` and `App.svelte` |
| Test adapter | `@agentos/sdk/testing` (in-memory host); an app's `src/testing.ts` / `src/mock/` |

The boundary rules (enforced by `bun run lint:boundaries`, `scripts/check-boundaries.ts`):

- An app's `src/client` imports only `@agentos/sdk` (root, `app/client`), `@agentos/ui`, `svelte`, `zod` and its own
  files: these are shared with the host's runtime. Its `src/server` may use `bun:*`, `node:*` and npm packages
  declared in its own `package.json`; never host internals, another app or client-side packages.
- `src/client` and `src/server` never import each other: they talk through `api.invoke` / `ctx.handle` /
  `ctx.emit`. A server module that needs Bun ships `src/mock/server.ts` for web mode and stories.
- Server modules are registered and unregistered at runtime: release everything they open in the dispose function
  `setup` returns (or on `ctx.signal`), or the old version keeps running beside the new one.
- Apps reach other apps only through tools (`api.tools.call('<app>__<tool>')`), never by import.
- All client ↔ server traffic is the SDK protocol over the one `/rpc` WebSocket: no raw `WebSocket`, no extra
  `Bun.serve` routes or endpoints. The server never imports app packages: apps arrive as bundles.

**Missing a host capability?** Extend the port, never open a side channel: add the method to `AppServerContext` or
`AppClientApi`, or the method to `host/protocol` with its zod schema, implement it in the server and in the memory
host, and test both. That is an SDK change: call it out in the MR.

UI layering is atomic (`docs/design/DESIGN_LANGUAGE.md`, `bun run lint:atomic`): atoms < molecules < organisms <
templates < pages, each importing only strictly lower layers. Reach for `@agentos/ui` first; a generic need goes
into the kit (with a story and a required `testid` prop), never into one app. The kit knows nothing about apps.

## Types and errors

- `strict` stays on. Untrusted values are `unknown` until a zod schema parses them. No `any`, no `as` on data you
  didn't construct, no non-null `!` (narrow, or state the invariant in a comment where it is established).
- Model alternatives as discriminated unions and close switches with a `never` check.
- Expected failures: return `[value, null] | [null, error]` for new APIs where the caller must branch, with an
  error that has a `kind`. Throw only for bugs and broken invariants. Don't churn existing throwing APIs (the
  Calculator's `evaluate` throws, and its callers rely on that).
- Framework contracts win: a tool's `run` throws an `Error` with an agent-readable message for rejected input (the
  MCP server returns it as `isError`); a `ctx.handle` handler that throws reaches the client as a JSON-RPC error
  carrying only its message, so a failure the client must branch on is returned as data, not thrown.

## Trust boundaries (parse with zod here)

| Boundary | Where |
|---|---|
| Client → server | `ctx.handle(channel, payload)`: `payload` is `unknown`; parse it first. Host methods are parsed by `createProtocolDispatcher`; a new method needs its schema in `host/protocol`. |
| Known apps | `known-apps.json` (`knownAppsSchema`), each `bundle.json` (`bundleDescriptorSchema`) and the bundle bytes (sha256-verified before install). Clients install by id, never by URL. |
| Agents → tools | the tool's zod `input` shape (validated by the registry). Describe every field: agents read it. |
| Disk | `ctx.storage.get<T>()` is an unchecked cast over data an older version may have written: parse it. |
| Agents → UI | agent messages, tool results and permission prompts are untrusted text: render them as text nodes (`atoms/RichText.svelte`), never `{@html}`. |
| Another app | `api.tools.call<T>()` results are another app's output: parse what you rely on. |

## Before you finish

Match `docs/TESTING.md`: shared logic gets bun tests next to it, every screen and state a story, every journey a
spec, every interactive element a `data-testid`. UI changes go through the `verify-change` skill.
