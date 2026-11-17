# AgentOS apps

`apps/desktop` is the **host OS**. Every other folder here is an **app module**: a workspace package that only talks to the host through `@agentos/sdk`.

```
apps/calculator/
  src/manifest.ts      id, name, icon, window size, dock: true/false
  src/main/index.ts    defineMainApp(ctx => …): IPC handlers, tools, storage
  src/renderer/        atomic layers + index.ts → defineRendererApp({ component })
  src/shared/          pure logic used by both sides
```

## How the pieces connect

```
 app renderer ──api.invoke──▶ host preload/IPC ──▶ app main  (ctx.handle)
 app renderer ◀──api.on────── host renderer hub ◀── app main (ctx.emit)
                                     │
 app main ──ctx.registerTool──▶ ToolRegistry ──▶ MCP server (127.0.0.1, bearer token)
                                     │                 ▲
 any app renderer ──api.tools.call───┘                 │ session/new mcpServers
                                         ACP agents ◀── AgentHost (main, ACP client)
```

- **Renderer API** (`useAppApi()` from `@agentos/sdk/app/renderer`): `invoke`, `on`, `tools.list/call`, `host.openApp`.
- **Main API** (`defineMainApp(ctx)` from `@agentos/sdk/app/main`): `handle`, `registerTool`, `emit`, `openWindow`, `storage`, `log`.
- **Tools** are namespaced `<appId>__<name>` and validated with zod. The same tool serves ACP agents through MCP, app renderers through IPC, and any local MCP client through `userData/mcp.json`.
- **Agents**: `apps/desktop/src/main/platform/acp/agents.ts` lists ACP agents. The host spawns them over stdio and passes the AgentOS MCP server into every session.

## The SDK is the only contract

Everything crosses process boundaries through `@agentos/sdk`. Nothing is inferred from an implementation:

| Import | Used by | Provides |
| --- | --- | --- |
| `@agentos/sdk` | everyone | `defineManifest`, tool types |
| `@agentos/sdk/app/main` | app main entry | `defineMainApp`, `AppMainContext` |
| `@agentos/sdk/app/renderer` | app UI | `defineRendererApp`, `useAppApi` |
| `@agentos/sdk/host/protocol` | host | every IPC channel: request/response types + zod validation |
| `@agentos/sdk/host/main` | host main | `registerHostIpc` (exhaustive, validated, sender-checked), `createHostEmitter` |
| `@agentos/sdk/host/preload` | host preload | `createHostBridge` → `window.agentos` |
| `@agentos/sdk/host/renderer` | host renderer | `hostBridge()`, `createAppRuntime` |

Security model:
- The renderer is sandboxed (`sandbox`, `contextIsolation`, no Node).
- The preload exposes one frozen `window.agentos` (`HostBridge`).
- Main accepts IPC only from the top-level renderer frame, and parses every argument with the protocol's zod schemas.
- Navigation away from the renderer is blocked, and only `http(s)` links open, in the system browser.
- The MCP server binds to 127.0.0.1 and requires a per-launch bearer token.

## Adding an app

1. Copy `apps/calculator` to `apps/<id>`, rename the package `@agentos/app-<id>`, and set the manifest `id`.
2. Add `"@agentos/app-<id>": "workspace:*"` to `apps/desktop/package.json` and to `workspacePackages` in `electron.vite.config.ts`.
3. Register it in both registries:
   - `apps/desktop/src/main/platform/installedApps.ts`: manifest + main module
   - `apps/desktop/src/renderer/src/lib/installedApps.ts`: manifest + lazy renderer import
4. Give it, from the first commit: `manifest.window.minWidth/minHeight` if it needs more than 320x240; a
   `.storybook/` (copy `apps/calculator/.storybook`, plus the storybook scripts and devDependencies) with a story
   per screen and state (`tags: ['app']`, inside `AppWindow`); `JOURNEYS.md` with matching `e2e/journeys.spec.ts`;
   a `data-testid` on every interactive element. Its main module must be browser-safe for headless tests.
5. Run `bun install`, then `bun run lint && bun run typecheck && bun run test:e2e`.
6. Design it with the `app-design` skill (journeys → research panel → conformance score → iterate). The app stays
   in its MR until the readiness gate in `.claude/skills/agentos-code-review` holds.

Apps must not import from `apps/desktop` or from other apps, and must not import `electron` directly
(`bun run lint:boundaries` enforces the full rule set; see AGENTS.md → "Boundaries").
