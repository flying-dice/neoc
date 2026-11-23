#!/usr/bin/env bash
# The neoc half of the pre-PR gate, and the scoped gate for a change confined to the neoc crates: it needs Rust,
# cargo-nextest, cargo-deny and luau-lsp (the pinned version, on PATH), and neither bun nor darklua. `bun run gate` runs
# it first, then the vos checks. Every step must pass exactly as written; the first failure stops the run.
#
# It is fast on purpose (iteration speed is a requirement): what takes minutes is not here but in
# scripts/check-slow.sh (`bun run slow`): the full hostile-input sweep, the release-build abort regressions, the bench
# bodies and the full generated-case counts.
#
# NEOC_REQUIRE_LUAU_LSP=1 is set here: a missing or other-version luau-lsp fails the Luau declaration tests instead of
# skipping them.
set -euo pipefail
cd "$(dirname "$0")/.."
export NEOC_REQUIRE_LUAU_LSP=1

step() { printf '\n==> %s\n' "$*"; }

step "cargo fmt --check"
cargo fmt --check

step "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

step "cargo test -p neoc -p neoc-vm"
cargo test -p neoc -p neoc-vm

# The suites that need a Docker daemon (sqlx_mysql, sqlx_postgres, testcontainers_modules) are left out; with Docker
# running, `neoc run 'crates/neoc/tests/**/*.test.lua' 'crates/neoc/tests/**/*.spec.lua'` runs every suite. No test
# reaches the internet (the hyper ones run against servers in the process); `cargo deny check` below needs network.
step "Lua suites (neoc run)"
cargo run -p neoc -- run \
  "crates/neoc/tests/lib/*.test.lua" \
  "crates/neoc/tests/std/*.test.lua" \
  "crates/neoc/tests/std/task.spec.lua" \
  "crates/neoc/tests/app.test.lua" \
  "crates/neoc/tests/buffer.test.lua" \
  "crates/neoc/tests/vnd/bollard.test.lua" \
  "crates/neoc/tests/vnd/hyper.test.lua" \
  "crates/neoc/tests/vnd/jsonschema.test.lua" \
  "crates/neoc/tests/vnd/keyring.test.lua" \
  "crates/neoc/tests/vnd/portable_pty.test.lua" \
  "crates/neoc/tests/vnd/quick_xml.test.lua" \
  "crates/neoc/tests/vnd/rusqlite.test.lua" \
  "crates/neoc/tests/vnd/serde_json.test.lua" \
  "crates/neoc/tests/vnd/serde_yaml.test.lua" \
  "crates/neoc/tests/vnd/toml.test.lua"

# What apps do with the modules (vos-lua's module tests), so a module change reaches them in the scoped gate too. The
# one test that builds its fixture with darklua is left out (this script needs no bun or darklua); the full gate runs
# it in `bun run test`.
step "vos-lua module tests: cargo nextest run -p vos-lua --test modules"
cargo nextest run -p vos-lua --test modules \
  -E 'not test(=an_app_built_by_the_real_pipeline_requires_the_neoc_modules)'

# The benches must keep compiling; running their bodies and the release-build abort regressions are in `bun run slow`.
step "cargo check -p neoc-vm --features bench --benches --locked"
cargo check -p neoc-vm --features bench --benches --locked

step "scripts/check-toolchain-pin.sh"
scripts/check-toolchain-pin.sh

step "cargo deny check"
cargo deny check

# The luau-lsp fixture tests (crates/neoc-vm/tests/luau_decls.rs) ran in `cargo test -p neoc -p neoc-vm` above, under
# NEOC_REQUIRE_LUAU_LSP=1; this checks that the committed types/neoc.d.luau is what the modules record.
step "Luau declarations: luau_decls --check"
cargo run -p neoc-vm --example luau_decls -- --check
step "The same declarations as a tree: luau_decls --ast --check"
cargo run -p neoc-vm --example luau_decls -- --ast --check

# types/generated/vos.d.luau and vos.d.ts are built from types/neoc.ast.json: a module change regenerates them
# (`cargo run -p vos-codegen`).
step "vos-codegen --check (generated files current against the Rust schema and types/neoc.ast.json)"
cargo run -q -p vos-codegen -- --check

# neoc and neoc-vm never depend on a vos-* crate, not even as a dev-dependency (no vos concept in the runtime).
step "no vos-* crate in the dependency tree of neoc or neoc-vm"
scripts/check-no-vos-leak.sh

step "cargo check --workspace --all-targets --locked"
cargo check --workspace --all-targets --locked

printf '\nneoc checks passed\n'
