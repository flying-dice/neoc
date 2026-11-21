#!/usr/bin/env bash
# The slow suites: everything that is kept out of `bun run gate` because it costs minutes, not seconds. Run it
# (`bun run slow`) before a PR that changes what these suites guard, and when you want the full sweep; it is not
# part of the pre-PR gate, a review or a fix round (CLAUDE.md, "The gate": iteration speed is a requirement).
#
# One mechanism for every slow suite: a test that is too slow for the gate is `#[ignore = "slow: ..."]`, and this
# script runs those tests and no other ignored one. It finds them by that reason: the function after each
# `#[ignore = "slow:` attribute under `crates/`. An ignored test with another reason (`vos-lua`'s `baseline.rs`, a
# measurement run on demand) is not a slow suite and does not run here. The `neoc` crate is left out: its cucumber
# harness takes no libtest flags and has no ignored test. The two things that are not tests of their own, a release
# build of the abort regressions and the bench bodies, are steps below. Add a new slow test with `#[ignore = "slow:
# run with bun run slow"]` within three lines above its `fn`; nothing here changes.
#
# What runs:
#   - hostile_inputs: the full sweep (about two million hostile calls over every module) and the 60 000-macro
#     ring (a wall-clock bound in a debug build);
#   - vos-domain: the full case counts of the generated-tree diff properties (the gate runs a fifth of them);
#   - abort_regressions in a release build: proves a default 2 MiB thread survives (the debug run is in the gate);
#   - the bench bodies, each once (the gate only compiles them).
#
# The first failure stops the run. Needs the pinned Rust toolchain, cargo-nextest and git; not bun, darklua or
# luau-lsp (none of the steps builds an app or checks the Luau declarations).
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }

# The `slow:` tests by name: the `fn` within three lines after each `#[ignore = "slow:` attribute. The count check
# catches an attribute too far from its `fn` (no name) and a window that reaches the next test (two names).
attributes=$(git grep -c '#\[ignore = "slow:' -- 'crates/*.rs' | awk -F: '{ n += $NF } END { print n + 0 }')
names=$(git grep -h -A3 '#\[ignore = "slow:' -- 'crates/*.rs' \
  | sed -n -E 's/^[[:space:]]*(pub )?(async )?fn ([A-Za-z0-9_]+).*/\3/p' | sort -u)
count=$(printf '%s\n' "$names" | grep -c . || true)
if [ "$count" -eq 0 ] || [ "$count" -ne "$attributes" ]; then
  echo "check-slow: found $attributes slow: attributes but $count test names; keep each attribute within three lines above its fn" >&2
  exit 1
fi
filter=$(printf '%s\n' "$names" | awk '{ printf "%stest(/(^|::)%s$/)", (NR > 1 ? " | " : ""), $0 }')

step "slow tests ($count): cargo nextest run --workspace --exclude neoc --run-ignored only -E '$filter'"
cargo nextest run --workspace --exclude neoc --run-ignored only --no-fail-fast -E "$filter"

step "cargo test --release -p neoc-vm --test abort_regressions"
cargo test --release -p neoc-vm --test abort_regressions

step "bench bodies, each once"
cargo bench -p neoc-vm --features bench --bench lib --bench vnd -- --test

printf '\nslow suites passed\n'
