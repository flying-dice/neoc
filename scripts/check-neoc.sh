#!/usr/bin/env bash
# The neoc half of the pre-PR gate, and the scoped gate for a change confined to the neoc crates: it needs Rust,
# cargo-nextest, cargo-deny and luau-lsp (the pinned version, on PATH), and neither bun nor darklua. `bun run gate` runs
# it first, then the vos checks. Every step must pass exactly as written; the first failure stops the run.
#
# It is fast on purpose (iteration speed is a requirement): what takes minutes is not here but in
# scripts/check-slow.sh (`bun run slow`).
#
# NEOC_REQUIRE_LUAU_LSP=1 is set here: a missing luau-lsp fails the strict-mode check of the VM's test scripts
# (crates/neoc-vm/tests/it/strict_luau.rs) instead of skipping it.
set -euo pipefail
cd "$(dirname "$0")/.."
export NEOC_REQUIRE_LUAU_LSP=1

step() { printf '\n==> %s\n' "$*"; }

step "cargo fmt --check"
cargo fmt --check

step "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

# `neoc-*` is the runtime: neoc-vm, neoc-vm-core, neoc-vm-std and the neoc-vm-lib-* and neoc-vm-vnd-* module crates (and neoc-agui).
step "cargo test -p neoc -p 'neoc-*'"
cargo test -p neoc -p 'neoc-*'

# No test reaches the internet (the hyper ones run against servers in the process); `cargo deny check` below needs
# network.

# What apps do with the modules (vos-lua's module tests), so a module change reaches them in the scoped gate too. The
# one test that builds its fixture with darklua is left out (this script needs no bun or darklua); the full gate runs
# it in `bun run test`.
step "vos-lua module tests: cargo nextest run -p vos-lua --test it (the modules:: tests)"
cargo nextest run -p vos-lua --test it \
  -E 'test(/^modules::/) & not test(=modules::an_app_built_by_the_real_pipeline_requires_the_neoc_modules)'

step "scripts/check-toolchain-pin.sh"
scripts/check-toolchain-pin.sh

step "cargo deny check"
cargo deny check

# types/generated/vos.d.luau and vos.d.ts are written from the schema of neoc-vm's modules (and of the `app` API):
# a module change regenerates them (`cargo run -p vos-codegen`).
step "vos-codegen --check (generated files current against the Rust schema of the modules)"
cargo run -q -p vos-codegen -- --check

# neoc and the VM crates never depend on a vos-* crate, not even as a dev-dependency (no vos concept in the runtime).
step "no vos-* crate in the dependency tree of neoc or the VM crates"
scripts/check-no-vos-leak.sh

step "cargo check --workspace --all-targets --locked"
cargo check --workspace --all-targets --locked

printf '\nneoc checks passed\n'
