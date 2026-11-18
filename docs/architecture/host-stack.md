# Rust host: spike results and decision memo

Everything below was built and run in this workspace (`rust-spike/`, rustc 1.98.1, macOS arm64). `cargo nextest run --workspace`:
**42 tests, 42 passed, 0.5 s.** Clippy: 0 errors, 21 pedantic warnings (spike code). `scripts/check-deps.py` and `cargo deny --exclude-dev check bans`: OK.
The spikes were restructured into the proposed hexagonal layout (section "Workspace layout"), so the layout is proven by compiling code, not only drawn.
~3.5k lines of code in 11 crates; the host binary is 5.5 MiB (release, stripped).

**Verdict: the stack works end to end. Recommend it as decided.** One test (`crates/agentos-host/tests/e2e.rs`) installs a Luau app from a `file://` app ref over `/rpc`, calls its tool from an `rmcp` client with a per-agent bearer token, shows `app.notify` reaching the UI socket, round-trips a UI event to a new view tree, uninstalls, and sees `tools/list_changed`.

## Answers per question

### 1. Luau via `mlua 0.12.2` (Luau 0.740, `luau0-src 0.22`) — `crates/agentos-luau`, tests `tests/vm.rs`
Features `luau, async, serialize, send`. Proven (all in `tests/vm.rs`, 9 tests):
- **One VM per app**, `lua.sandbox(true)` (builtins readonly, per-app globals; `io`/`os.execute`/`dofile`/`debug.getregistry` absent; two VMs have independent globals).
- **Memory limit** `set_memory_limit` (4 MiB test: bomb dies with `memory error`, VM stays usable).
- **CPU budget** via `set_interrupt`: `while true do end` dies at the budget (100 ms test). The kill flag is *sticky*, so `while true do pcall(...) end` cannot swallow it. The slice resets when a host `await` returns (waiting on SQLite is not app CPU). Luau's own pattern matcher polls the interrupt: a backtracking `string.find` dies at the budget too.
- **Capability table** `app.data.query/exec` (async fn on the blocking pool), `app.tools.register{}`, `app.notify{}`, `app.state.get/set`, `app.on(event, fn)`. Rust→Luau event call with a JSON table works (`serde` feature; `emit()`/`call_tool()`).
- **Source from bytes** (`lua.load(&[u8]).set_name("=app/main.luau")`; syntax errors report `app/main.luau:1`).
- **Unload frees**: drop Rust-held `Function`s, then the `Lua`. 300 VMs created, dropped, re-created: RSS growth 80 KiB total, so memory is reused.
- **Numbers** (`cargo run --release -p agentos-luau --example bench`): VM create + sandbox + API + run main chunk **0.14 ms (opt-level 3) / 0.19 ms (`z`)**; **~524 KiB Luau heap per VM** (mostly the stdlib; 100 apps ≈ 52 MB); event dispatch **1.8 µs (opt 3) / 3.1 µs (`z`)**; per-app in-memory SQLite open+migrate 0.04 ms.
- **`--!strict` annotations run** (Luau strips them at compile). Type *checking* is a separate tool: `luau-analyze` (Homebrew 0.718) checked a strict file against an API written as type aliases and flagged wrong field types, unknown members and number→string (`apps/strict_demo.luau`). **Type-definition files:** `types/agentos.d.luau` (`declare class …; declare app: AppApi`) is the shipping format; luau-lsp consumes it via `--definitions` / `luau-lsp.types.definitionFiles`. **Not verified here:** the Homebrew `luau-analyze` has no `--definitions` flag, and my attempt to download and run the `luau-lsp` release binary was blocked by the permission policy, so the `.d.luau` file is written but unchecked. First task in the real project: run luau-lsp against it.
- **Gotchas found:** (a) mlua registers a *filesystem* `require` on Luau: set `require = nil` before `sandbox(true)` and provide a bundle-only one. (b) `to_value` maps JSON null to a truthy lightuserdata: use `SerializeOptions::serialize_none_to_null(false)`. (c) Install `app` *before* `sandbox(true)`. (d) `send` makes `Lua: Send+Sync` (mutex inside), which is what lets tokio multi-thread run VMs; one VM executes one call at a time (an app is single-threaded by design).
- **Hot reload:** `notify 8.2` watcher → debounce → build a new VM → swap → old VM dropped (`src/reload.rs`, test passes with FSEvents). In the real host this is a `reload(app)` use case triggered by a dev-mode driving adapter (DB is kept; only the VM is rebuilt).

### 2. SQLite — recommend **`rusqlite 0.43.0` (bundled SQLite 3.54.1)** — `crates/agentos-sqlite`
- Design proven (7 tests): one `Connection` per app behind a mutex, all work in `spawn_blocking`; migrations numbered 1..N, each in its own transaction, recorded in `_migrations(version,name,checksum,applied_at)`, idempotent, incremental on app upgrade, **checksum-tamper detected**, failing migration leaves nothing; **authorizer denies `ATTACH`/`DETACH`** (an app's SQL can't escape its file).
- **Change notifications:** `update_hook` buffers `(table, op, rowid)`, `commit_hook` publishes one batch to a `tokio::broadcast`, `rollback_hook` discards. Verified: a multi-statement txn gives one batch; a rolled-back txn gives none. This beats calling `changes()` (which can't see triggers, cascades, or rollbacks).
- **sqlx 0.9.1 probe** (`spikes/sqlx-probe`) also works (dynamic `Migrator` from manifest data, `lock_handle().set_update_hook`). Why not: async-native but still a worker thread per connection; the hook needs the raw handle; **0.9 forces `AssertSqlSafe(..)` on every dynamic SQL string** (all app SQL is dynamic); `migrate!()`/compile-time checking don't apply to runtime-loaded apps; heavier compile. It shares `libsqlite3-sys 0.40` with rusqlite 0.43, so both link in one binary, but the versions must move together.
- **Gotcha:** rusqlite's `serde_json` feature stores a JSON *string* param as its quoted JSON text. Use own conversion (`to_sql_params`), covered by a test.

### 3. MCP — **`rmcp 3.11.0`** (`server, macros, transport-streamable-http-server`; client features for tests) — `crates/agentos-mcp`
- `ServerHandler` implemented by hand (`list_tools` from a runtime catalogue, `call_tool`), mounted via `StreamableHttpService` + `nest_service("/mcp")` on axum. The `ToolRegistry` port is implemented by `McpState`: `publish/retract` → `notify_tool_list_changed()` to every live peer.
- **Both protocol generations proven** (3 tests): legacy `2025-11-25` session (`list_changed` on the GET stream, `with_legacy_session_mode(true)`) **and** `2026-07-28`, which has *no sessions* and uses `subscriptions/listen` (implemented via `listen()` + a broadcast). Install/uninstall during an open session produced `tools/list_changed` and the new `tools/list`.
- **Auth + identity:** axum middleware → `Authorization: Bearer` → `AgentDirectory` port (constant-time compare) → `AgentIdentity` into request extensions → `ctx.extensions.get::<http::request::Parts>()` inside `call_tool`. 401 + `WWW-Authenticate` otherwise (tested for none/wrong/Basic). Per-agent permission test: "Claude" (granted `notes`) succeeds; "Other" gets an in-band `isError` result `permission denied: Other may not call notes__add_note`, and the VM never ran.
- Tool names are `<app>__<tool>`. `StreamableHttpServerConfig` has built-in `Host` allow-list (loopback by default) and optional `Origin` validation: keep both on.
- Risk: rmcp is moving fast (3.x with a new spec version); pin `=3.11.0` and re-run these 3 tests on every bump.

### 4. ACP — **`agent-client-protocol 5.3.0`** (+ `-schema 1.18.0`; features `process`, `stdio`) — `crates/agentos-acp`
- Client = `Client.v1().connect_with(AcpAgent::new(AcpAgentConfig::new(prog).args(..)), async |cx| { cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await })`. It spawns the child, speaks stdio, shuts it down. Runs fine under tokio.
- **Proven (3 tests):** handshake against a 25-line fake agent binary (`src/bin/fake_agent.rs`); `AgentConnector::start_run` = initialize + `session/new`; and the **real adapter**: `node …/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js` → `initialize` returns agent_info `@agentclientprotocol/claude-agent-acp 0.108.0`, protocol v1, in 0.27 s. No prompts, no model calls. No processes left behind.
- Not spiked: `session/prompt` streaming → AG-UI mapping (the next milestone), permission-request round trips, and the crate's MCP-over-ACP feature (`unstable_mcp_over_acp`) for handing agents our `/mcp` URL + token in `session/new`.

### 5. AG-UI — **hand-rolled**, `crates/agentos-agui` (163 lines incl. endpoint)
- Crates.io: `ag-ui 0.5.0-alpha.3` (third party, "working toward becoming official", 600 downloads), `ag-ui-core 0.1.0` (the community SDK inside `ag-ui-protocol/ag-ui/sdks/community/rust`, Aug 2025, client-oriented, no server), `agui-sdk 0.9.1`, `ag-ui-rs 0.3.1` (all single-maintainer). **No maintained official Rust crate.**
- The wire model is one serde enum: `#[serde(tag="type", rename_all="SCREAMING_SNAKE_CASE", rename_all_fields="camelCase")]` (15 variants: RUN_*, STEP_*, TEXT_MESSAGE_*, TOOL_CALL_*, STATE_*, CUSTOM). SSE endpoint = `POST /agui` taking `RunAgentInput`, returning `axum::response::Sse` of `data: {json}\n\n`; malformed input is a 422 before the stream starts (per the transport spec). Test prints the raw frames (RUN_STARTED, TEXT_MESSAGE_START/CONTENT×2/END, RUN_FINISHED) and checks shapes against docs.ag-ui.com events. Add variants as the client needs them.

### 6. HTTP stack — **axum 0.8.9 + tower-http 0.6.11 + hand-rolled JSON-RPC peer** — `crates/agentos-http`
- Proven (3 tests): `/rpc` WebSocket upgrade; **Origin + Host guard** (`AllowedOrigins`, middleware): evil Origin → 403 on the handshake, DNS-rebind Host rejected, no-Origin (native client) allowed by config; static serving with `ServeDir` + `index.html` SPA fallback + precompressed gzip.
- **JSON-RPC: hand-rolled peer** (~120 lines, `rpc.rs`; envelope types live in the domain). Recommended over `jsonrpsee 0.26.1`: we need the *server* to issue requests to the client and await the answer on the same socket. The test does exactly that (client request → handler calls `peer.request("client/echo")` → client answers → original response), plus notifications both ways, per-request tasks (no deadlock), pending-request cleanup on disconnect. jsonrpsee's server model is request/response + subscriptions (not tested here; from its docs).
- Gotcha found by the e2e: a notification emitted *during* a request can arrive before its response; clients must not assume order.

### 7. Shared types — **`ts-rs 12.0.1` (TS) + `schemars 1.2.3` (JSON Schema) + a 70-line Luau emitter** — `crates/agentos-codegen`
- Domain types derive `TS`/`JsonSchema` behind a `codegen` feature (derive-only, no I/O). `cargo run -p agentos-codegen --example generate` writes `generated/{types.ts, ui.luau, schemas.json}`. ts-rs honours serde attributes (`tag`, `rename_all`): `UiNode` comes out as the discriminated union `{ "type": "stack", … } | { "type": "button", … }`. The Luau emitter walks the JSON Schema (`oneOf`, `$ref`, recursive `#`, optional fields) into `export type`s; **test: the generated Luau passes `luau-analyze` and a wrongly-shaped node (`txt =` for `text =`) is a type error**.
- `specta 2.0.0-rc.28` / `specta-typescript 0.0.15` are still pre-release: not chosen. Rough edges: ts-rs warns on `#[serde(try_from)]` (we override with `ts(type="string")`); `#[serde(default)]` fields need `#[ts(optional)]` to become optional in TS.
- **Superseded by wave A2:** the component schema is Rust data in `agentos-domain` (`ui` module) and `agentos-codegen` emits `types/generated/{agentos.ts, agentos.d.luau, ui.luau, app.luau}` and `docs/reference/components.md`; see `docs/reference/components.md` and the `agentos-codegen` crate docs.

### 8. Distribution
- **Binary:** host, release, `opt-level="z"`, LTO, `codegen-units=1`, `strip`, `panic=abort`: **5.5 MiB** with Luau, bundled SQLite, rustls, axum, rmcp, ACP, notify. ~50 s release build on this machine. Opt-level 3 is 25-40% faster on VM creation/dispatch (0.14 vs 0.19 ms, 1.8 vs 3.1 µs); ship level 3 unless size matters (Luau's C++ is compiled by `cc` with the profile's opt-level, so `z` also affects VM speed).
- **Cross-compile:** pure crates `cargo check --target x86_64-unknown-linux-gnu` fine. Anything with C/C++ (Luau, SQLite, `aws-lc-sys` for rustls) fails without a cross C toolchain (`cc-rs: failed to find x86_64-linux-gnu-gcc`). Recommend **`cargo-zigbuild`** (or build natively per OS in release CI); the Docker daemon wasn't running, so Linux builds were **not verified**. macOS: native arm64 built; x86_64-apple-darwin is just `rustup target add` (same SDK). `bundled` SQLite + Luau mean no system libs to link.
- **Dev hot reload:** see section 1 (proven).

## Recommended crate list (versions resolved in `Cargo.lock`)
`tokio 1.54.1` · `axum 0.8.9` (ws, macros) · `tower-http 0.6.11` (fs) · `mlua 0.12.2` (luau, async, serialize, send) · `rusqlite 0.43.0` (bundled, hooks) · `rmcp 3.11.0` · `agent-client-protocol 5.3.0` (process, stdio) · `reqwest 0.13.5` (rustls) · `serde 1`/`serde_json 1.0.154` · `ts-rs 12.0.1` · `schemars 1.2.3` · `notify 8.2.2` · `thiserror 2` (libs) · `anyhow 1` (host binary only) · `async-trait 0.1` · `parking_lot 0.12` (no lock poisoning ⇒ no `unwrap`) · `sha2 0.10` · `tokio-util 0.7` · `tokio-stream`/`futures-util`. Dev: `tokio-tungstenite 0.30`, `cargo-nextest`, `cargo-deny`. Hand-rolled: JSON-RPC peer, AG-UI events. Not used: sqlx (probe only), jsonrpsee, specta.

## Workspace layout (hexagonal; each adapter is its own crate depending inward only)

```
                       agentos-host  (bin; wiring only; the ONLY place anyhow + every adapter appear)
                                  |
   driving adapters               |                         driven adapters
   agentos-http  (/rpc WS, guard, static, RpcHub)   agentos-luau   (AppRuntime)
   agentos-mcp   (/mcp, ToolRegistry)               agentos-sqlite (DataStore, AppData)
   agentos-agui  (/agui SSE)                        agentos-fetch  (Fetcher: file/http/https)
   agentos-acp   (AgentConnector, ACP client)       agentos-codegen (dev-time: TS + Luau + schemas)
                      \                                  /
                       agentos-application   ports (traits) + use cases (AppService), in-memory fakes (`testing`)
                                  |
                       agentos-domain        pure model: AppId, AppRef/Manifest (+sha256 verify), Permission,
                                             AgentIdentity, ToolSpec/QualifiedToolName, Notification,
                                             UiNode/UiPatch/UiEvent, JSON-RPC + protocol messages. No I/O.
```

| crate | responsibility | may depend on |
|---|---|---|
| `domain` | the model above; unit tests without any framework | serde, serde_json, sha2, thiserror (+ ts-rs/schemars behind `codegen`) |
| `application` | **ports**: `AppRuntime, DataStore/AppData, ToolRegistry, Notifier, EventSink, AgentConnector, AgentDirectory, Clock, Fetcher`. **Use cases** (`AppService`): install-from-url (fetch → sha256 verify → migrations → load → publish tools → announce), uninstall(+purge), dispatch UI event, tool call with permission check, start agent run. Runtime-free (tokio is a dev-dependency only) | domain, async-trait, parking_lot, thiserror |
| `luau`, `sqlite`, `fetch`, `mcp`, `acp`, `agui`, `http`, `codegen` | one adapter each | application, domain (never another adapter; dev-deps may) |
| `host` | build adapters, hand each to its port, mount routers, bind 127.0.0.1, graceful shutdown | everything |

- A `ports` crate split (adapters depend on traits only, not use cases) is cheap if `application` grows; driving adapters (`http`, `mcp`) legitimately need the use cases.
- Add later: `sdk` (the `agentos.d.luau` + stubs shipped to app authors), `registry` (a list of app-ref URLs, pure), `permissions` if policy grows beyond `Permission`.
- **Error types per layer:** `DomainError` → `PortError` (what adapters return; each adapter maps its errors with `From`) → `AppError` (use cases; wraps both) → JSON-RPC code / MCP `isError` at the edge. `anyhow` only in `main`.
- **Workspace config (in `Cargo.toml`):** `[workspace.package]`, `[workspace.dependencies]` with pinned versions, `[workspace.lints]`: clippy `pedantic` warn, `unwrap_used`/`expect_used`/`panic` deny, `unsafe_code = "forbid"`; `clippy.toml` with `allow-unwrap-in-tests`; `rustfmt.toml`. (Integration-test helper fns need a one-line `#![allow(clippy::unwrap_used)]` in each `tests/*.rs`; the in-memory fakes carry one `allow`.)
- **Dependency direction enforcement — recommend `scripts/check-deps.py`** (60 lines over `cargo metadata`): an explicit per-crate allow-list of internal edges, an allow-list of external crates for `domain`/`application`, and a framework deny-list (tokio, axum, mlua, rusqlite, reqwest, rmcp…) for the core. Proven: passes now; adding `application → luau` fails with `internal edge not allowed`. Keep `cargo-deny` too (`deny.toml`: `bans` with `wrappers` pins each framework to its adapter, plus licenses/advisories/sources). Rewrite the script in the repo's `bun` style next to `check-boundaries.ts` if preferred. Crate edges alone aren't enough (a stray `use tokio` in `application` would compile if the dep were present), hence the gate.
- **Test strategy per layer** (all demonstrated): domain = plain unit tests (7); application = use cases against in-memory port fakes, no mocking library (4: install/verify/tamper-no-state-change/uninstall/permission-before-VM); adapters = integration tests through the real library (luau: sandbox/limits/async/hot reload; sqlite; mcp with an rmcp client; acp against a fake agent + the real adapter; http with a WS client; fetch against a local server); end-to-end = `agentos-host/tests/e2e.rs` driving `/rpc` + `/mcp` + `/agui` of the real wiring. Run with `cargo nextest run --workspace` (0.5 s).

## Open risks
1. **Luau type tooling unverified** (luau-lsp + `.d.luau`, see §1). `luau-analyze` works for strict checking but needs definitions inlined unless luau-lsp is used.
2. **Hard CPU kill:** the interrupt covers Luau code and pattern matching; a *host* function that blocks (our SQLite call with a huge query) isn't covered by the Luau budget. SQLite needs `progress_handler`/`interrupt()` and `busy_timeout` (set: 2 s) plus a per-call wall-clock timeout in the runtime adapter; not built.
3. **Memory per VM ~0.5 MiB** baseline and one global mutex per VM: fine for dozens of apps; measure real apps. Per-VM *CPU fairness* relies on tokio's multi-thread pool (tested: a spinning VM didn't stall another app).
4. **rmcp / ACP / MCP spec churn** (rmcp 3.x already carries two protocol generations; ACP crate is at 5.x with a draft v2). Pin, re-run the adapter tests on each bump, keep each behind its port. Claude-side MCP clients today speak 2025-x (sessions); both are served.
5. **Notification ordering** on `/rpc` (notifications can overtake responses) must be part of the client contract.
6. **Auth on `/rpc`:** Origin+Host guard stops cross-site pages, but any local process can still connect; add a per-launch token (cookie or query) if the threat model includes other local users/processes.
7. **ACP→AG-UI mapping** (prompt streaming, permission requests, tool-call events) is the largest unproven piece; the transport pieces on both sides are proven.
8. **Linux/Windows builds unverified** (needs zigbuild/CI); `x86_64-pc-windows-gnu` target is installed but untried; `notify` and process spawning differ per OS.
9. **Luau ergonomics:** no `require` of files yet (bundle-only `require` to build), no debugger, error spans only as `app/main.luau:line`.

## What I did not do / failed
- Did not run `luau-lsp` (download executed under a policy block; I deleted it). Did not run Docker/Linux builds (daemon down). Did not test `jsonrpsee` (reasoned from docs). Did not test `https://` fetches (no network test; reqwest+rustls is compiled and works for `http://`).
- No file in `/Users/jonathanturnock/Projects/AgentOS` was modified; no processes left running.

## Running the host

The host is the product: one binary that serves the built client, `/rpc`, `/agui` and `/mcp`.

```sh
cargo run -p agentos-host                  # serve (bun run host does the same)
cargo run -p agentos-host -- pair          # print a fresh pairing code (alias: --new-pairing-code)
cargo run -p agentos-host -- --config f    # use a specific config file
RUST_LOG=agentos_host=debug,info cargo run -p agentos-host
```

It listens on `0.0.0.0:7400` by default. At start it prints the URLs it is reachable on (loopback, each LAN
address, the hostname) and a **pairing code**. `SIGINT` and `SIGTERM` shut it down gracefully: it stops
accepting, drains open connections for 3 seconds, unloads every app (their data is kept), stops agent
processes, and exits 0. Logging is `tracing`, filtered by `RUST_LOG` (default `info`), written to stderr; the
banner and pairing code go to stdout.

### Pairing

Every client route needs a paired device, **including from loopback**: there is no trusted-network shortcut.

1. The host prints a code like `K7QM-3XHR`: 8 characters from a 31-symbol alphabet without `0 O 1 I L`
   (about 40 bits), valid 10 minutes, single use. `agentos-host pair` issues another at any time, even while the
   host is running (they share `<data>/system.db`).
2. The client's static files load unauthenticated. The shell asks `GET /session` (`{"paired":false}`), shows its
   pairing screen, and posts `POST /pair` with `{"code": "...", "deviceName": "..."}`. The device name defaults
   to a label from the `User-Agent`.
3. The host answers with `Set-Cookie: agentos_session=<256-bit token>; Path=/; HttpOnly; SameSite=Strict`
   (plus `Secure` over TLS) and `{"device": {...}}`. The database stores only the SHA-256 of the token and of the
   code, with the device name, creation time and last-seen time.
4. `/rpc` and `/agui` (and any route added later: protected is the default) require that cookie, else `401`.
5. Wrong codes are rate limited: after 5 attempts in a minute, `429` with `Retry-After`, even for the right code;
   10 wrong guesses in 10 minutes destroy every outstanding code.
6. Sessions have no expiry; they last until revoked. The JSON-RPC methods `host.devices.list` (`{devices:[{id, name,
   createdAt, lastSeenAt, current}]}`) and `host.devices.revoke` (`{id}`) back a future Settings screen. Revoking a
   device hangs up its sockets and ends its session.

`/mcp` is separate: per-agent bearer tokens only. A paired session is not an MCP credential and an MCP token is
not a session. For local MCP clients the host writes `<data>/mcp.json` (mode 0600,
`{"url": ".../mcp", "headers": {"Authorization": "Bearer ..."}}`); the token changes on every start.

The **Origin/Host guard** covers every route. A request's `Host` must be a loopback address, one of this
machine's interface addresses (re-read every 10 s, so joining a network works without a restart), its hostname,
any `*.local` name, or a name in `allowed_hosts`; own names and addresses must carry the listening port. A browser
`Origin` must satisfy the same rule. Everything else gets `403`, which is what stops DNS rebinding and
cross-site WebSockets.

### LAN and TLS

Reaching the host from another device needs only the default bind; the client then opens
`http://<lan-address>:7400`. Plain HTTP on the LAN is fine for the desktop itself, but **a PWA's service worker
(install, offline) needs a secure context**, and only `localhost` is exempt. For a PWA on another device, serve TLS:

```toml
[tls]
cert = "certs/agentos.pem"   # certificate chain, PEM
key = "certs/agentos.key"    # private key, PEM
```

With TLS the host speaks HTTPS directly (rustls) and sets `Secure` on the cookie; `mcp.json` then holds an
`https://` URL. Use a certificate the devices trust (a local CA such as mkcert, or a real name via a tunnel or
reverse proxy). Behind a TLS-terminating proxy, leave TLS off, set `secure_cookies = true` and list the proxy's
name in `allowed_hosts`.

### Configuration

Defaults, then `agentos.toml`, then environment variables. The file is `--config <path>`, else
`AGENTOS_CONFIG`, else `./agentos.toml`, else `<data_dir>/agentos.toml`. Unknown keys are an error.

| `agentos.toml` | environment | default |
|---|---|---|
| `data_dir` | `AGENTOS_DATA` | `~/.agentos` (mode 0700): `system.db`, `apps/<id>.db`, `mcp.json` |
| `bind` | `AGENTOS_BIND` | `0.0.0.0` |
| `port` | `AGENTOS_PORT` | `7400` (`0` picks a free port) |
| `client_dir` | `AGENTOS_CLIENT_DIR` | `client/dist`, else `apps/desktop/dist`; none built = API only |
| `registries` | `AGENTOS_REGISTRIES` (comma list) | none: app-ref URLs installed at start |
| `allowed_hosts` | `AGENTOS_ALLOWED_HOSTS` (comma list) | none |
| `dev_client_port` | `AGENTOS_DEV_CLIENT_PORT` | none: also accept the Vite dev server on this port |
| `dev_reload` | `AGENTOS_DEV_RELOAD` | off: watch the `file://` app bundles and rebuild an app's VM (database kept) when one changes |
| `secure_cookies` | `AGENTOS_SECURE_COOKIES` | on when TLS is configured |
| `[tls] cert`, `key` | `AGENTOS_TLS_CERT`, `AGENTOS_TLS_KEY` | TLS off |

An explicit `client_dir` that is not built is an error; the default quietly falls back to the old
`apps/desktop/dist` until the client moves to `client/`.

### The gate

`bun run lint` (adds `cargo fmt --check`, clippy with `-D warnings`, `scripts/check-deps.ts`, `cargo deny check`,
`lint:luau`, `lint:stylua`, `codegen:check`), `bun run typecheck` (adds `cargo check --workspace --all-targets` and
`tsc -p types`), `bun run test` (adds `cargo nextest run --workspace` and `bun test types`), `bun run format` (adds
`cargo fmt` and StyLua). `codegen:check` fails when the committed `types/generated/*` and `docs/reference/components.md`
are stale against the Rust schema (`bun run codegen` regenerates them). `lint:luau` type-checks every Luau app and
fixture (and the generated `ui.luau`) with `luau-lsp analyze` against the generated
`types/generated/agentos.d.luau`, resolving `require("@agentos/ui")` through the root `.luaurc`; a negative fixture
(`negative/*.luau`, lines marked `-- expect-error`) must be rejected, so a broken definitions file cannot pass
silently. `lint:luau` and `lint:stylua` skip with a message when `luau-lsp` or
`stylua` is not installed.

## Apps: lifecycle, data and the Luau API (wave A4)

- **Installed apps persist.** `system.db` records every install (`installed_apps`: id, app-ref URL, version,
  enabled). At start `AppService::restore_installed` re-fetches each app from its URL (re-verifying checksums) and
  loads it; the configured `registries` are then applied on top (missing ones installed, failed ones retried). An
  app that cannot load is marked **failed** (and announced as `app.failed`), the others start. Uninstall forgets
  the record; shutdown keeps it.
- **Lifecycle over `/rpc`:** `app.install`, `app.uninstall {app, purge?}`, `app.list`, `app.enable`,
  `app.disable` (VM unloaded, tools retracted, data kept), `app.deleteData` (purge, then reload on an empty
  database), `app.reload` (dev: rebuild the VM from source, keep the database; the old version keeps running if the
  new one does not load).
- **The app API** is typed in `types/generated/agentos.d.luau` (generated from `crates/agentos-codegen/src/luau/app_api.luau.tpl`): `app.manifest`, `app.db` (`migrate`, `exec`,
  `all`, `one`, `tx`, and the sources `query` / `row` / `scalar`), `app.kv`, `app.state`, `app.tools.register`
  with `app.t` input builders, `app.notify`, `app.shell`, `app.window`.
  `require` is bundle-only (`./x`, `../x`, `@agentos/app`, host modules).
- **Live sources** (`agentos-application::live`): the SQLite authorizer reports every table a prepared query reads,
  `commit_hook` batches become `ChangeBatch`es (coalesced per frame), and `LiveSources::refresh` re-runs the
  affected subscriptions in one read transaction and diffs them (ordered window keys + changed rows by content hash).
- **Two-way writes** (`AppService::write_field`): `baseRev` check, three-way merge of text
  (`agentos_domain::merge3`: non-overlapping edits compose, overlapping edits go to the human), human wins for
  scalars, the replaced value kept as a revision for 30 days (`_ui_revisions`) with an undo (`restore_revision`).
- **SQLite gotchas found:** the update hook is blind to `WITHOUT ROWID` tables and `REPLACE` conflict deletes (tables
  an authorized statement writes are published too); a transaction holds an async write gate so nothing interleaves
  with it; a statement past its wall-clock budget is interrupted by the progress handler.
- **Luau gotchas found:** the interrupt also fires when mlua resumes the coroutine to poll an await, so the CPU
  clock is paused while the app waits on the host (and a kill lands when the call returns); a Rust future that is
  aborted mid-await is only dropped at the next GC, so no lock or transaction is held across an await (a call that
  was cut off cannot wedge the next one); `string` coercion makes `raw_get::<String>(5)` succeed for a number, so
  check the variant.

### Windows and the view protocol (wave A4, phase 2)

- **A window is a view instance.** `app.window(name, { client, state, render, onOpen, onClose })` declares a kind;
  `view.open` creates an instance with its own `w` (per-window `w.state`, `w.client` Refs and `:get`/`:set`, `w.env`
  Refs, `w:toast/focus/scrollTo/announce/copy/close`). `render(w)` runs as a pure, tracked scope: state it reads is
  the instance's dependency set, and a write inside it raises. The Luau node tree (from the generated
  `@agentos/ui`) becomes the wire tree, **handlers stay closures in the VM keyed `path#event`**, and the tree is
  validated (`validate_with`, with the view's client and source declarations) before it leaves the VM.
- **Sources per render.** A source factory called in `render` returns a handle `{["$source"] = id}` (any other field
  is a `src.<id>.<field>` ref; `$`-prefixed keys are nil). One id per (declaration, params) pair, stable across
  renders (`s2`, then `s2_2` for a second param set), so ops do not churn. `view.open` subscribes each source with its
  params resolved from the client's values; `source.set` re-parameterises or moves the window; a re-render that starts
  to use a new source declares it in `data.patch.declare` with its first window.
- **Ordering.** After a handler (or an agent tool, through the `AppObserver`) the view service *settles*: re-render
  every view whose last render read state that changed (keyed `diff` -> `view.ops`, `base`/`rev`; a tree over 1 000
  ops goes as `view.tree`), deliver the windows' commands and `client.set`, refresh the live sources. All of it is
  queued on the socket before the response, which therefore means "the UI already reflects it". Notifications from
  the client are handled in order, inline, so `client.state` is FIFO ahead of a later event.
- **Two-way writes.** `row.set {source, key, field, value, baseRev}`: the binding kind (text / scalar) comes from the
  component schema (`ui::bindings`); the merge base is the value the host last sent for that row field (a ring of
  8); the ack carries `resolution` (`accepted` / `merged` / `replaced` / `rejected`) and, when a value was replaced,
  `undo {revisionId, lost}` that `row.restore` puts back. `persist = "app"` client values are stored in `_ui_state`
  and come back in the next `view.open`.
- **Gaps.** `state.set` (two-way bound window state) answers an error: app/window state is read as values in render,
  so there is no `state.*` ref to bind; `app:openWindow` has no host-to-client message yet (the requests queue for the
  shell); `view.resize` / `view.visibility` are accepted and ignored; a toast action's `onTap` is not routed back;
  renders are not coalesced per frame; host restart has no grace period (a reconnect opens fresh instances that
  adopt the client's persisted values).

## Agents over AG-UI, identity and permissions (wave B3)

Agents are **contacts** in `system.db`. The host launches each over ACP (stdio), keeps the process alive, and exposes
its chats to the client over AG-UI (`/agui`, HTTP + SSE, behind the device-session guard). ACP never reaches the
browser: `agentos-acp` maps ACP to the domain's `AgentEvent`, the application's `ChatService` folds those into a thread
(history, state, interrupts) and into AG-UI events, `agentos-agui` is the HTTP/SSE edge.

### The `/agui` contract

All routes need a paired device (`401` otherwise). Errors before a stream starts are JSON `{"error": "..."}`: `422` for a
body that is not a `RunAgentInput`, `404` unknown agent or thread, `409` a run is already in progress on the thread,
`502` the agent process could not start.

| route | purpose |
|---|---|
| `POST /agui/threads` `{threadId, agentId}` | Ensure a thread: start the agent process if needed, `session/new` (with the agent's own MCP token), idempotent for the same ids. Returns a `ThreadSnapshot`. |
| `GET /agui/threads/{id}` | The `ThreadSnapshot`: `{threadId, agentId, running, state, messages, interrupts}`. |
| `POST /agui` | A run: `RunAgentInput` (`threadId`, `runId`, `messages`, `state`, `tools`, `context`, `forwardedProps`, optional `resume`). The **last `user` message** is the prompt (the ACP session holds the context); `forwardedProps.agentId` creates the thread if `POST /agui/threads` was not called. Answers `text/event-stream`, one `data: <event json>` frame per AG-UI event. |
| `POST /agui/threads/{id}/cancel` | ACP `session/cancel` (also resolves a pending permission as cancelled). `204`. A client that drops the SSE connection mid-run cancels the same way. |
| `POST /agui/threads/{id}/settings` | `{"mode": "<id>"}` or `{"config": {"id": "<option id>", "value": <string or bool>}}`: ACP `session/set_mode` / `session/set_config_option`. Answers the new `ThreadState`; a running stream also gets a `STATE_DELTA`. |

**Events** (names and fields as docs.ag-ui.com; `camelCase`). One run is `RUN_STARTED`, a `STATE_SNAPSHOT`, the turn's
events, then `RUN_FINISHED` or `RUN_ERROR`.

| ACP | AG-UI |
|---|---|
| `session/prompt` accepted, finished with a stop reason | `RUN_STARTED` ... `RUN_FINISHED {outcome:{type:"success"}}` (`result: {stopReason}`) |
| prompt failed, agent exited, a `refusal` stop | `RUN_ERROR {message, code}` (`agent_exited`, `prompt_failed`, `refusal`) |
| `agent_message_chunk` | `TEXT_MESSAGE_START {role:"assistant"}`, `TEXT_MESSAGE_CONTENT`, `TEXT_MESSAGE_END` (a message spans consecutive chunks) |
| `agent_thought_chunk` | `REASONING_START`, `REASONING_MESSAGE_START {role:"reasoning"}`, `REASONING_MESSAGE_CONTENT`, `REASONING_MESSAGE_END`, `REASONING_END` |
| `tool_call` | `TOOL_CALL_START {toolCallName: title}`, `TOOL_CALL_ARGS` (raw input as JSON text, if any), `TOOL_CALL_END`, and a `CUSTOM agentos.tool` |
| `tool_call_update` | `CUSTOM agentos.tool {toolCallId, title, kind, status}` (`pending`, `in_progress`, `completed`, `failed`); on `completed`/`failed` with text output, `TOOL_CALL_RESULT` |
| `plan` | `STATE_DELTA` replacing `/plan` |
| `current_mode_update`, `config_option_update`, `available_commands_update`, `usage_update`, `session_info_update` | `STATE_DELTA` (RFC 6902 `replace`/`add`) under `/session/...` |
| `session/request_permission` | the run ends: `RUN_FINISHED {outcome:{type:"interrupt", interrupts:[...]}}` (AG-UI interrupts) |
| user-visible host errors (not ACP) | `RUN_ERROR` |

**Permission requests are AG-UI interrupts.** ACP's `session/request_permission` blocks the agent mid-turn. The host
ends the current run with `RUN_FINISHED {outcome:{type:"interrupt", interrupts:[Interrupt]}}` and keeps the ACP turn
suspended (up to 10 minutes, `expiresAt`, then it resolves as cancelled). The interrupt is
`{id, reason:"tool_call", message: <tool title>, toolCallId?, expiresAt, responseSchema:{type:"object", properties:
{optionId:{type:"string", enum:[...]}}, required:["optionId"]}, metadata:{"agentos:options":[{optionId, name, kind}],
"agentos:tool":{title, kind}}}` where `kind` is ACP's `allow_once`, `allow_always`, `reject_once` or `reject_always`.
The client answers with a **new run on the same thread** whose input has `resume:[{interruptId, status:"resolved",
payload:{optionId}}]` (or `status:"cancelled"` to deny) and no new user message; that run streams the rest of the
turn (`RUN_STARTED` again, ...). A reload re-offers pending interrupts through `GET /agui/threads/{id}`. A new user
message while interrupts are pending, a resume that misses an interrupt, an unknown id: `RUN_ERROR`.

**State** (what AG-UI does not define, typed in `agentos_domain::thread`, zod-validated in the client): `ThreadState =
{ session: SessionState, plan: PlanEntry[] | null }`.

```jsonc
"session": {
  "agent":   { "name": "claude-agent-acp", "version": "0.108.0" } | null,
  "title":   "string" | null,
  "toolsConnected": true,                       // the agent got /mcp (HTTP MCP capability)
  "modes":   { "current": "default", "available": [{ "id", "name", "description"? }] } | null,
  "config":  [ { "id": "model", "name": "Model", "description"?, "category"?: "model" | "thought_level" | "mode" | string,
                 "kind": "select", "current": "opus", "options": [{ "value", "name", "description"? }] },
               { "id", "name", "kind": "boolean", "current": true } ],
  "commands": [{ "name": "compact", "description": "...", "hint"?: "..." }],   // slash commands: send "/name args" as the user message
  "usage":   { "used": 12000, "size": 200000, "cost"?: { "amount": 0.12, "currency": "USD" } } | null
}
```

**Thread history** (`ThreadSnapshot.messages`, AG-UI messages in order): `{id, role:"user", content}`; `{id,
role:"assistant", content}` (a text segment); `{id, role:"reasoning", content}`; a tool call is an assistant message
`{id, role:"assistant", toolCalls:[{id, type:"function", function:{name, arguments}}], agentos:{title, kind,
status}}` followed by `{id, role:"tool", toolCallId, content}` once it has output; a failed run is `{id, role:"system",
content, agentos:{error:true}}`. History lives in `system.db` (`thread_messages`); the ACP session does not survive a
host restart (a new one starts with the next prompt; the transcript stays).

**Agents and tokens.** `Agent {id, name, avatar?, role, runtime:{command, args, env}, grants, approval}`. Grants are
**deny by default**: `{"apps": []}` means no tools; `{"apps": ["notes"]}` is an allow-list; `{"all": true}` is the
explicit "all apps" option. `approval` (`ask` | `auto_read_only` | `allow_all`, default `auto_read_only`) says what the
host does with an agent's ACP permission requests about AgentOS tools: `auto_read_only` allows a request for a tool the
app declared `readOnly` and prompts for the rest, `ask` prompts always, `allow_all` never prompts. A persistent
**MCP bearer token** is stored hashed (SHA-256; shown once, at `agents.create` and `agents.token.rotate`), for agents the
host does not launch. A host-launched agent gets a **fresh random token per process launch**, kept only as a hash in
memory and handed to the agent in `session/new`: every `tools/call` is attributed (`ctx.actor = {kind="agent", id,
name}`) and checked against that agent's grants. `tools/list` is filtered to the agent's granted apps; `tools/list_changed`
goes to that agent's sessions when its grants change or an app it may use is published or retracted.

`/rpc` methods (for a future Contacts/Settings UI): `agents.list`, `agents.create`, `agents.update`, `agents.delete`,
`agents.grants.set`, `agents.token.rotate`.

**`readOnly` is enforced.** A tool declared `readOnly = true` runs with app writes blocked: the app's SQLite handle runs
with writes denied (`app.db:exec`, `db:tx` writes, DDL, and a write smuggled through `db:all ... returning` all fail),
`app.kv` `set`/`delete` raise, and `app.db:migrate` raises. A tool that must write is not `readOnly`.

**Tool names** are local identifiers `[a-z][a-z0-9_]*` (no dots); the MCP name is `<appId>__<tool>`.
