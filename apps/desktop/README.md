# @agentos/desktop

The AgentOS client: an installable PWA (Vite + Svelte 5) that is the iPadOS-style shell. It talks to the Bun server
(`apps/server`) over one WebSocket with JSON-RPC 2.0 (`@agentos/sdk/host/client`), and loads installed apps' client
bundles from the server.

```sh
bun run dev        # with the server: from the repo root, `bun run dev` (server :7400 + this on :5173)
bun run dev:web    # web mode on :6200: the shell on the in-memory host, repo apps from source, no server
bun run build      # → dist/ (served by the server in production)
```

Entry points: `src/renderer/index.html` → `src/boot.ts` (real server) and `src/renderer/web.html` → `src/web/main.ts`
(in-memory host). See `apps/README.md` for building apps and `docs/TESTING.md` for tests.
