# Agents

## Short change → test loops

Every runtime module is its own crate, so build and test the smallest crate that holds the change, never the
workspace:

- `std:*` is `neoc-vm-std`; a `lib:<m>` module is `neoc-vm-lib-<m>` (`crates/neoc-vm-lib/<m>`); a `vnd:<m>` module is
  `neoc-vm-vnd-<m>` (`crates/neoc-vm-vnd/<m>`); the engine, lifecycle and host are `neoc-vm-core`.
- While iterating: `cargo check -p <crate> --all-targets`, then `cargo test -p <crate> <test name>` for the one test.
- Before calling it done: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo test -p <crate>`.
- Once, at the end, the cross-module checks: `cargo test -p neoc-vm` (the scripts suite and the strict-Luau check,
  which link every module), `bun run codegen` if a module's API changed, then `bun run gate`.

Measured on `vnd:bollard`: one red → green → refactor cycle takes 18 s in its own crate and 73 s through `neoc-vm`.

App work (`apps/*`) compiles no Rust: use the app's own `bun run build`, `typecheck`, `lint` and `test`. The e2e
journeys build the host once and reuse it while the Rust is unchanged.

## Logging and tracing in an app

The platform records most failures without the app doing anything. Don't log these again:

- **Page** (`bridge.js`):
  - each bus call is a span (`rpc <method>`), red with its error when it rejects;
  - `console.*` is recorded at its level;
  - an error the page doesn't handle after it has started is recorded and shown on its window.
- **Host**: each call is a span (`app.call <method>`), and a call it refuses is logged.
- **Backend**:
  - a `lib:`/`vnd:` call that waits is a span, red when it raises, even when the script catches the error;
  - a handler that raises is logged as an error, with its location;
  - `ctx:fail(message)` and an RPC error are logged as warnings;
  - a background task that fails with nothing waiting for it is logged as an error.

What only the app can say, and so must:

- **A failure it catches and turns into something for the person**: a status, a caption, a timeline entry, a fallback value, an error swallowed in a loop. Log it once, where it is caught:
  - Backend: `tracing.warn("<what could not be done>", { error = tostring(err), <what it was about> })`.
  - Page: `vos.tracing.warn(...)`. For a bug, `vos.tracing.error(err)` with the `Error` itself, which keeps its stack.
  - The message is plain words. The original error goes in `error`: the line shows it after the message, so don't paste it into the message as well.
  - Don't log it when it is answered with `ctx:fail`, or when it is a bus call's rejection on the page. Both are recorded already.
- **Background work's turning points**: things with no call around them, such as a watcher or feed that stops or comes back, or a session that ends. Log each turning point once, never on every poll.
- **Spans**: put a span around a step only when knowing which step took the time or failed helps. Name it for what is being done (`tracing.span("pull image", { image = ... })`).

Choosing the level:

- `error`: a bug, or work lost.
- `warn`: expected, and outside the app's control (Docker refused, the network is down, the person gave a bad value).
- `info`: a change of state that nobody asked for at that moment.
- `debug`: detail.

Never log a secret's value (passwords, tokens, keys, environment values). Fields named like secrets are dropped by the host, but don't depend on that.
