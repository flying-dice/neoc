# Agents

## Short change → test loops

Every runtime module is its own crate, so build and test the smallest crate that holds the change, never the
workspace:

- `std:*` is `neoc-vm-std`; a `lib:<m>` module is `neoc-lib-<m>` (`crates/neoc-lib/<m>`); a `vnd:<m>` module is
  `neoc-vnd-<m>` (`crates/neoc-vnd/<m>`); the engine, lifecycle and host are `neoc-vm-core`.
- While iterating: `cargo check -p <crate> --all-targets`, then `cargo test -p <crate> <test name>` for the one test.
- Before calling it done: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo test -p <crate>`.
- Once, at the end, the cross-module checks: `cargo test -p neoc-vm` (the scripts suite and the strict-Luau check,
  which link every module), `bun run codegen` if a module's API changed, then `bun run gate`.

Measured on `vnd:bollard`: one red → green → refactor cycle takes 18 s in its own crate and 73 s through `neoc-vm`.

App work (`apps/*`) compiles no Rust: use the app's own `bun run build`, `typecheck`, `lint` and `test`. The e2e
journeys build the host once and reuse it while the Rust is unchanged.
