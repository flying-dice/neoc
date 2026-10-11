---
status: Accepted
date: 2026-05-04
---

# 0008. `lib:http` — Static file server with directory index

Date: 2026-05-04

## Status

Accepted

## Context

Scripts need to expose local files over HTTP with zero tooling. Motivating case: Playwright screenshots browsable immediately via single function call.

**Why `lib:*`?** Batteries-included static serving. Must not depend on `vnd:*` (dependency flows downward).

**Glob-scoped vs directory-root model.** Go/Rust serve entire directory trees. In embedding context: risky (accidental sensitive file exposure). `lib:http` inverts: callers provide glob, only matched files reachable. Explicit allowlist. Files created after startup not served (acceptable; API compatible with future re-glob).

**Alternatives considered:**

| Approach | Verdict |
|---|---|
| Wrap `tower_http::ServeDir` via `vnd:*` | Violates `lib:*`/`vnd:*` boundary. Rejected. |
| Raw `tokio::net::TcpListener` + manual HTTP/1.1 | Correct for scope. Accepted. |
| Directory-root model | Risky for scripts. Rejected for glob-scoped. |
| `lib:mime` for MIME | Uses `mime_guess` directly (same crate). Accepted. |

## Decision

`lib:http` exposes `serve_static(glob, opts)`:

```lua
local http = require("lib:http")

http.serve_static("./screenshots/**/*.png", { port = 8080 })

http.serve_static("./dist/**/*", {
    port             = 8080,
    host             = "127.0.0.1",
    directory_index  = true,
    mime_overrides   = { [".wasm"] = "application/wasm" },
})
```

### Behaviour

1. **Glob resolution** — resolves via `globset` + `walkdir` into `HashSet<PathBuf>` of absolute paths. Immutable for server lifetime.
2. **Index** (`GET /`) — HTML listing: file name, size, mtime. Image types get `<img>` thumbnails. Dark/light mode.
3. **File serving** — `Content-Type` via `mime_guess` + `mime_overrides`. `Content-Length`, `Last-Modified` from metadata. `Accept-Ranges: none` (v1).
4. **404** — any path not in allowlist. No traversal possible (absolute-path set).
5. **Blocking** — blocks coroutine via `async_function` (ADR 0006). Ctrl-C/SIGTERM terminates.

### Security

- No path traversal — allowlist is canonical absolute paths, filesystem not consulted for routing.
- No symlink following beyond initial glob (`follow_links(false)`).
- Binds `127.0.0.1` by default. `0.0.0.0` requires explicit opt-in.
- No TLS. No write methods (`POST`/`PUT`/`DELETE` → 405).

### Deferred

- Range requests (`Accept-Ranges: none` set explicitly)
- Live reload (re-glob on request)

## Consequences

**Easier:**
- Zero-friction static serving from Lua — one call, no external tools.
- Glob allowlist → path traversal structurally impossible.
- `mime_overrides` unblocks callers from upstream patches.

**Harder:**
- Post-startup files invisible (stale listing until re-glob enhancement).
- Manual HTTP/1.1 parsing must handle edge cases conservatively.
- No range requests — large file streaming inefficient.

## References

- Issue: flying-dice/neoc#19
- ADR 0006 — Hidden async
- ADR 0003 — Namespace structure
- [Node.js `http-server`](https://www.npmjs.com/package/http-server)
- [Deno `std/http/file_server`](https://jsr.io/@std/http/doc/file-server)
- [Go `net/http.FileServer`](https://pkg.go.dev/net/http#FileServer)
- [`tower_http::ServeDir`](https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html)
