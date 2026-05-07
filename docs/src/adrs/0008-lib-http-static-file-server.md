---
status: Accepted
date: 2026-05-04
---

# 0008. `lib:http` — Static file server with directory index

Date: 2026-05-04

## Status

Accepted

## Context

`neoc` targets LLM-agent and automation workflows where scripts need to expose local files over HTTP with zero external tooling. The canonical motivating case: a Playwright session captures screenshots, and the agent needs to immediately browse them in a browser. The developer should be able to serve them with a single `http.serve_static(glob, opts)` call — no npm, no Python, no separate server process.

### Why a `lib:*` module?

`lib:*` modules are blessed gap-fillers for capabilities the Lua standard library omits. Batteries-included static file serving falls squarely in that lane. It must not depend on `vnd:*` modules (the dependency graph flows strictly downward); it may depend on `std:*` primitives, Rust std, and focused crates that do not cross the `vnd` boundary.

### The glob-scoped model vs. a directory-root model

The Go `net/http.FileServer` and Rust `tower_http::ServeDir` both serve an entire directory tree rooted at a given path. The security model is: give the operator the right root, and everything under it is served. In an embedding context like `neoc`, that assumption is unsafe — a script author may accidentally point the root at a directory that contains sensitive files alongside the intended assets.

`lib:http` inverts this: callers provide a glob pattern, and only files that match the pattern at startup are reachable. Nothing not in the initial match set is ever served, regardless of what lives on the filesystem. This is an explicit allowlist rather than a deny-by-root boundary.

Tradeoff: files created after startup are not served (no live reload). This is acceptable for the target workflow and called out explicitly as a non-goal. A future enhancement could re-glob on each index request; the API is compatible with that change.

### Alternatives considered

| Approach | Verdict |
|---|---|
| **Wrap `tower_http::ServeDir` via a `vnd:*` module** | Violates the `lib:*` / `vnd:*` boundary. `lib:*` must not import `vnd:*`. Rejected. |
| **Wrap `actix-files`** | Same boundary violation. Also pulls in the full actix-web framework stack. Rejected. |
| **Use raw `tokio::net::TcpListener` + manual HTTP/1.1 parsing** | The issue's original proposal. Avoids all framework dependencies. Correct for the scope of static serving — request parsing is trivial (read method + path + a handful of headers). Accepted. |
| **Serve a directory root (Go/Rust consensus model)** | Safer for compiled binaries; risky for scripts. The glob allowlist is a better fit for a sandboxed scripting context. Rejected in favour of glob-scoped approach. |
| **Defer to `lib:mime` for MIME detection** | `lib:mime` has landed. `lib:http` uses `mime_guess` directly (same crate already pulled by `lib:mime`; no new dependency). `mime_overrides` gives callers extension points without waiting on upstream patches. Accepted. |

### Prior art signals

- **Node.js `http-server`** — the UX gold standard. Auto-generated directory listings with file name, size, and modified time. MIME via the `mime` npm package. Range request support. Cache headers configurable. `http-server` treats listing as a first-class feature, not an afterthought.
- **Bun** — `Bun.serve` static property pre-loads fixed routes; `Bun.file()` handles dynamic serving. No built-in directory listing. Ships a hardcoded MIME map (~200 extensions); nobody complains. Proves that an inline map is production-acceptable.
- **Deno `std/http/file_server`** — `serveDir(req, { fsRoot, showDirListing, showIndex })`. Closest analogue to this proposal. Serves a directory tree (not glob-scoped). `showDirListing: true` generates an HTML index. ETag via size+mtime (cheap, no hashing). Full `Range` support. Blocks path traversal via symlink resolution.
- **Go `net/http.FileServer`** — directory listing auto-generated unless `index.html` present. MIME via `mime.TypeByExtension` backed by OS MIME databases. Range requests via `http.ServeContent`. Accepts `fs.FS` interface — clean abstraction for virtual/embedded filesystems.
- **Rust `tower_http::ServeDir`** — directory listing disabled by default; opt-in. MIME via `mime_guess`. Range requests supported. Directory listing generator is not built-in; requires a custom fallback.

## Decision

`lib:http` exposes a `serve_static(glob, opts)` function to Luau scripts. It resolves a glob pattern into an allowlisted set of file paths at call time, binds a minimal HTTP/1.1 server on a configurable port, and blocks the calling coroutine until the process receives a termination signal.

### Luau API

```lua
local http = require("lib:http")

-- Serve all matched files. Blocks until Ctrl-C / SIGTERM.
http.serve_static("./screenshots/**/*.png", { port = 8080 })

-- Full options table (all fields optional):
http.serve_static("./dist/**/*", {
    port             = 8080,        -- TCP port to bind (default: 8080)
    host             = "127.0.0.1", -- Bind address (default: "127.0.0.1")
    directory_index  = true,        -- Serve auto-generated index at GET / (default: true)
    mime_overrides   = {            -- Extend or override the built-in MIME map
        [".wasm"] = "application/wasm",
    },
})
```

### Behaviour

1. **Glob resolution** — on call, resolve the glob via `globset` + `walkdir` (both already in `Cargo.toml`) into a `HashSet<PathBuf>` of absolute paths. This set is immutable for the lifetime of the server.

2. **Index page** (`GET /`) — when `directory_index` is `true` (the default), return an HTML page listing every matched file as a clickable link. Sorted alphabetically. Shows file name, size (human-readable), and last-modified timestamp. Minimal styling with `prefers-color-scheme` dark/light support. Image types (`png`, `jpg`, `gif`, `svg`, `webp`) additionally render an `<img>` thumbnail with `loading="lazy"` and a `max-height` constraint — this is a primary use case and the listing should be immediately useful.

3. **File serving** (`GET /<relative-path>`) — if the requested path resolves to a file in the allowlist, serve it with:
   - `Content-Type` inferred from the extension via `mime_guess` (extended by `mime_overrides`)
   - `Content-Length` from `fs::metadata`
   - `Last-Modified` from `fs::metadata` mtime (already retrieved for the index; cost is zero)
   - `Accept-Ranges: none` for v1 (range requests deferred)

4. **404** — any path not in the resolved allowlist returns a clean 404 HTML page. No path traversal is possible: the allowed set is computed at startup from absolute paths, and incoming request paths are resolved against that set — not against the filesystem directly.

5. **Blocking semantics** — `serve_static` blocks the calling Lua coroutine via `mlua`'s `async_function` bridge (consistent with ADR 0006). Tokio handles the accept loop. Ctrl-C / SIGTERM terminates the server and the coroutine resumes.

### Implementation

- Module: `src/lua/lib/http.rs`, `pub fn module(lua: &Lua) -> mlua::Result<Table>`
- Registration: `src/lua/lib/mod.rs`, alphabetical
- HTTP server: `tokio::net::TcpListener` → async accept loop → read request line + headers → route → write response. No HTTP framework. The request surface for static serving is narrow: parse method, path, and `Host`; ignore everything else.
- MIME detection: uses `mime_guess` (the same crate already pulled by `lib:mime`; no new dependency). Extension is extracted from `Path::extension()`, passed to `mime_guess::from_ext`, and falls back to `application/octet-stream` for unknown extensions. `mime_overrides` entries are checked first, allowing callers to extend or override without a patch release.
- Glob matching: `globset::GlobSet` compiled at call time, walked via `walkdir::WalkDir`.
- Index page: rendered as a `String` computed once at startup (not per-request) since the file set is immutable. Regeneration happens only if a future enhancement re-globs on index requests.

### Security model

- **No path traversal** — the allowlist is a `HashSet<PathBuf>` of canonical absolute paths. Incoming request paths are looked up in this set; the filesystem is not consulted for routing decisions. A request for `/../etc/passwd` cannot match any entry in the set.
- **No symlink following beyond the initial glob** — `walkdir` follows symlinks by default; `lib:http` configures it with `follow_links(false)` so symlinks outside the glob root do not widen the allowlist.
- **Bind address defaults to `127.0.0.1`** — localhost-only by default. Callers who need network-wide access (e.g. containerised environments where localhost is not reachable from the browser) must explicitly pass `host = "0.0.0.0"`. This follows the principle of least exposure: the safe default requires no additional configuration.
- **No TLS** — consistent with the existing HTTP-only stance in `neoc`. TLS is out of scope.
- **No write methods** — `POST`, `PUT`, `DELETE`, `PATCH` return `405 Method Not Allowed`. The server is read-only.

### MIME type handling

Extension-based detection is the consensus across Node.js (mime package), Bun (hardcoded map), Deno (@std/media-types), Go (mime.TypeByExtension), and Rust (mime_guess). Content sniffing is not performed — it requires reading file bytes and introduces ambiguity. `lib:http` delegates to `mime_guess` (the same crate `lib:mime` already uses), giving coverage of ~1000 extensions with no inline maintenance burden. Unknown extensions fall back to `application/octet-stream`, which is correct behaviour (the browser will download rather than render, which is safe).

The `mime_overrides` option allows callers to add or override entries without waiting for a patch release. This addresses the most common forward-compatibility concern.

### Directory index behaviour

The index is auto-generated and on by default (`directory_index = true`). This is the primary differentiator from Rust's `tower_http::ServeDir` (which requires explicit opt-in and has no built-in listing generator) and is the core value proposition of the module. Turning it off (`directory_index = false`) makes `GET /` return 404, which is useful when the server is being used as a pure file endpoint rather than a browsable listing.

There is no `index.html` fallback: glob-matched files are routable by their relative path only. If an `index.html` is in the glob set, it is served at `GET /index.html`, not at `GET /`. This keeps the routing model simple and predictable.

### Range requests

Deferred. Screenshots and typical web assets do not require resumable downloads or video scrubbing. `Accept-Ranges: none` is set on all responses. A follow-on issue should implement `206 Partial Content` + `Accept-Ranges: bytes` — Go and Deno both treat range support as table-stakes for a production file server. The header is set explicitly to `none` (not omitted) so clients do not attempt range requests and receive confusing errors.

## Consequences

**Easier:**
- Zero-friction static serving from Luau scripts — one function call, no external tooling.
- The glob-scoped allowlist gives a stronger safety guarantee than directory-root models; path traversal is structurally impossible.
- Thumbnail previews in the index page make the screenshot-browsing workflow immediately useful without a separate image viewer.
- `mime_overrides` means callers are not blocked on upstream patches for new file types.
- `Last-Modified` headers are free (metadata is already fetched for the index page) and enable conditional `GET` with `If-Modified-Since`, reducing unnecessary transfers.

**Harder / risks introduced:**
- Files created after the server starts are not visible. Scripts that generate files while the server runs will see a stale listing. The fix (re-glob on index request) is a clean layering concern but requires a follow-on change.
- Manual HTTP/1.1 parsing avoids framework debt but must handle edge cases: malformed request lines, missing `Host`, oversized headers. The implementation must be conservative — return 400 on any parse failure rather than attempting to recover.
- No range request support means large files (video, large PDFs) cannot be streamed efficiently. Callers expecting `206` behaviour will be disappointed. Mitigated by setting `Accept-Ranges: none` explicitly.
- `mime_overrides` is still needed for edge cases not covered by `mime_guess`, but the maintenance burden is minimal — `mime_guess` handles the long tail automatically.
- `127.0.0.1` as the default bind address means the server is not reachable from other hosts out of the box. Callers in containerised or networked environments must explicitly pass `host = "0.0.0.0"` to expose the server on all interfaces. This is the correct trade-off: safe by default, opt-in for exposure.

## References

- Originating issue: flying-dice/neoc#19
- `lib:mime` (landed): provides `mime_guess` as a shared dependency
- ADR 0006 — Hidden async with synchronous-looking Lua APIs
- ADR 0003 — Three-namespace module structure (`std`, `lib`, `vnd`)
- [Node.js `http-server`](https://www.npmjs.com/package/http-server) — UX reference
- [Deno `std/http/file_server`](https://jsr.io/@std/http/doc/file-server) — closest analogue
- [Go `net/http.FileServer`](https://pkg.go.dev/net/http#FileServer) — range request and MIME reference
- [`tower_http::ServeDir`](https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html) — Rust ecosystem consensus
