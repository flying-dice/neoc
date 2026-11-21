#!/usr/bin/env bash
# The slow suites: everything that is kept out of `bun run gate` because it costs minutes, not seconds. Run it
# (`bun run slow`) before a PR that changes what these suites guard, and when you want the full sweep; it is not
# part of the pre-PR gate, a review or a fix round (CLAUDE.md, "The gate": iteration speed is a requirement).
#
# One mechanism for every slow suite: a test that is too slow for the gate is `#[ignore = "slow: ..."]`, and this
# script runs every ignored test of the workspace (the `neoc` crate is left out: its cucumber harness takes no
# libtest flags and has no ignored test). The two things that are not tests of their own, a release build of the
# abort regressions and the bench bodies, are steps below. Add a new slow test with `#[ignore = "slow: run with
# bun run slow"]`; nothing here changes.
#
# What runs:
#   - hostile_inputs: the full sweep (about two million hostile calls over every module) and the 60 000-macro
#     ring (a wall-clock bound in a debug build);
#   - vos-domain: the full case counts of the generated-tree diff properties (the gate runs a fifth of them);
#   - abort_regressions in a release build: proves a default 2 MiB thread survives (the debug run is in the gate);
#   - the bench bodies, each once (the gate only compiles them).
#
# The first failure stops the run. Needs the same tools as `bun run gate:neoc`, plus cargo-nextest.
set -euo pipefail
cd "$(dirname "$0")/.."
export NEOC_REQUIRE_LUAU_LSP=1

step() { printf '\n==> %s\n' "$*"; }

step "ignored tests: cargo nextest run --workspace --exclude neoc --run-ignored only"
cargo nextest run --workspace --exclude neoc --run-ignored only --no-fail-fast

step "cargo test --release -p neoc-vm --test abort_regressions"
cargo test --release -p neoc-vm --test abort_regressions

step "bench bodies, each once"
cargo bench -p neoc-vm --features bench --bench lib --bench vnd -- --test

printf '\nslow suites passed\n'
