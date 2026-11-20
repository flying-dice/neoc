# Rust in AgentOS (`crates/`)

The host is a Cargo workspace (`Cargo.toml`, `rust-toolchain.toml`, edition 2021, `rustfmt.toml`: 110 columns,
`clippy.toml`). `bun run lint:rust` is the gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
-- -D warnings`, `scripts/check-deps.ts`, `cargo deny check`. `bun run format` runs `cargo fmt`.

## Layering

| Crate | Owns | May use |
|---|---|---|
| `agentos-domain` | the pure model: ids, app refs and manifests, tool grants, agent identity, tools, notifications, the view tree and component schema, protocol messages, `merge3`. Unit tests. | `serde`, `serde_json`, `sha2`, `thiserror`; `ts-rs` and `schemars` only behind the derive-only `codegen` feature |
| `agentos-application` | **ports** (traits) and **use cases** (`AppService`, `ChatService`, `DeviceService`, live sources, write-back), plus in-memory fakes in `testing`. Runtime-free: tokio is a dev-dependency. | `agentos-domain`, `async-trait`, `parking_lot`, `thiserror`, `tracing`, small runtime-free crates |
| adapters (`lua`, `sqlite`, `fetch`, `acp`, `agui`, `http`, `mcp`) | one port or one protocol each | application and domain; **never another adapter** (dev-dependencies may) |
| `agentos-codegen` | dev-time generation from the domain schema; not linked into the host | domain |
| `agentos-host` | composition root: build, wire, mount, bind, shut down. No business logic. | everything; the only crate with `anyhow` |

Rules of thumb:

- A new behaviour goes in the innermost layer that can express it. A rule about apps, permissions or tools is domain or a
  use case, not an adapter. An adapter translates (SQLite rows, mlua values, HTTP, ACP messages) and holds no policy.
- A new need from the outside world is a **port** in `agentos-application::ports` first, with a fake in `testing`, then an
  adapter. A driving adapter (`http`, `mcp`, `agui`) calls use cases, never reaches an adapter.
- A framework crate appears only in its adapter (`deny.toml` pins it with `wrappers`): mlua in `neoc-vm` and the `neoc` CLI (the `lua` adapter reaches it through neoc-vm's re-export), rusqlite in
  `sqlite`, rmcp in `mcp`, agent-client-protocol in `acp`, axum in the driving adapters and the host. A new crate must
  be added to `scripts/check-deps.ts` on purpose.
- Dependencies are declared once in `[workspace.dependencies]` with pins for the fast movers (`rmcp`, `agent-client-protocol`,
  `mlua`); crates say `{ workspace = true }`. rustls only: no OpenSSL.
- The host loads and unloads apps while running: whatever an adapter opens for an app (VM, connection, watcher, process)
  is dropped on unload. No lock or transaction is held across an `.await`.

## Errors

- One error type per layer, `thiserror`, no stringly `Box<dyn Error>`: `DomainError` (invalid ids, bad manifests, checksum
  mismatch) to `PortError` (`NotFound`, `Rejected`, `Internal`: what adapters return, each adapter maps its own errors
  with `From`) to `AppError` (use cases; wraps both, plus `NotInstalled`, `NotRunning`, ...) to a JSON-RPC code or an MCP
  `isError` result at the edge. Add a variant when a caller must branch; otherwise reuse one.
- `anyhow` only in the host binary (`main.rs` and startup). Libraries never return it.
- Propagate with `?`. Add context where the cause is not obvious from the variant. A user or app mistake is `Rejected`
  (it reaches the caller with a message); a broken invariant is `Internal`.
- Error messages are lowercase, no trailing period, and say what was wrong with the value
  ("app calculator is disabled"), because they reach agents and the UI.

## No panics, no unsafe

`[workspace.lints]` denies `clippy::unwrap_used`, `expect_used` and `panic`, and forbids `unsafe_code`. Tests may use
`unwrap`, `expect` and `panic!` (`clippy.toml`; an integration-test file with helpers has a one-line
`#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`).

- Input from outside (JSON-RPC params, tool arguments, app output, agent messages, files, the network) never reaches an
  index, `unwrap` or slice that can fail: parse it into a typed value and return the error.
- Prefer `parking_lot` locks (no poisoning, so no `unwrap`) and `get` over indexing. A branch that cannot happen is a
  typed error (`Internal`) or restructured away, not `unreachable!`.
- An adapter that truly needs `unsafe` opts out locally, with a comment saying why, and a review note. Almost none should.

## Clippy and style

- Clippy `pedantic` is on at warn, and the gate runs `-D warnings`, so fix the lint; allow one only at the narrowest scope
  with a reason. Allowed workspace-wide: `module_name_repetitions`, `missing_errors_doc`, `must_use_candidate`.
- Names say the domain concept (`AppId`, `ToolSpec`, `QualifiedToolName`); validated newtypes have one constructor
  (`AppId::parse`). Make invalid states unrepresentable; close `match`es without a wildcard when the enum is ours.
- Public items in library crates carry a `///` line that says what they are for; modules start with a `//!` that names the
  ring or the port the crate implements (see any `lib.rs`).
- Async ports use `async-trait`; blocking work (SQLite) runs on the blocking pool. Shared state is `Arc<...>` handed in by
  the host, not a global. Logging is `tracing` (`info!`, `warn!` with fields), never `println!` in a library.
- Serialisation: `serde` with explicit attributes on wire types (`tag`, `rename_all`); a wire type that the client also
  uses derives `TS` behind the `codegen` feature, and `bun run codegen` regenerates `types/generated/*`
  (`bun run codegen:check` fails when stale).

## Tests

`cargo nextest run --workspace --exclude neoc --exclude neoc-vm`. Domain: unit tests. Application: use cases against the fakes in `testing`, no mocking
library. Adapters: integration tests in `tests/` through the real library. Host: `crates/agentos-host/tests` start the real
wiring on an ephemeral loopback port. Test the failure path beside the success path; a bug fix gets a regression test in the
layer where it lived. See `docs/TESTING.md`.
