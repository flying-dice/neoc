#!/usr/bin/env bash
# The slow suites: everything that is kept out of `bun run gate` because it costs minutes, not seconds. Run it
# (`bun run slow`) before a PR that changes what these suites guard, and when you want the full sweep; it is not
# part of the pre-PR gate, a review or a fix round (CLAUDE.md, "The gate": iteration speed is a requirement).
#
# One mechanism for every slow suite: a test that is too slow for the gate is `#[ignore = "slow: ..."]`, and this
# script runs those tests and no other ignored one. It finds them by that reason: the function after each
# `ignore = "slow:` under `crates/` (any spacing, plain or inside a `cfg_attr`, tracked by git or not). The `neoc`
# crate is left out: its cucumber harness takes no libtest flags and has no ignored test. The two things that are not
# tests of their own, a release build of the abort regressions and the bench bodies, are steps below. Add a new slow
# test with `#[ignore = "slow: run with bun run slow"]` within three lines above its `fn`; nothing here changes.
#
# It fails closed. It checks its selection against `cargo nextest list --run-ignored only`: every ignored test must be
# one of the `slow:` tests found above or on NOT_SLOW below (a test ignored for another reason: today the `vos-lua`
# baseline, a measurement run on demand), and every `slow:` test must be one nextest ignores. Anything else (another
# spelling of the attribute, an attribute too far from its `fn`, a new test ignored without a reason) stops the
# script with a message. `--list` does only that check and prints the selection (a dry run: no slow suite runs).
#
# What runs:
#   - hostile_inputs: the full sweep (about two million hostile calls over every module) and the 60 000-macro
#     ring (a wall-clock bound in a debug build);
#   - vos-domain: the full case counts of the generated-tree diff properties (the gate runs a fifth of them);
#   - abort_regressions in a release build: proves a default 2 MiB thread survives (the debug run is in the gate);
#   - the bench bodies, each once (the gate only compiles them).
#
# The first failure stops the run. Needs the pinned Rust toolchain and cargo-nextest; not bun, darklua or
# luau-lsp (none of the steps builds an app or checks the Luau declarations).
set -euo pipefail
cd "$(dirname "$0")/.."

mode=run
case "${1:-}" in
  "") ;;
  --list) mode=list ;;
  *) echo "usage: bash scripts/check-slow.sh [--list]" >&2; exit 2 ;;
esac

step() { printf '\n==> %s\n' "$*"; }
fail() { printf 'check-slow: %s\n' "$*" >&2; exit 1; }

# Ignored for another reason than slow, as `cargo nextest list --message-format oneline` prints them.
NOT_SLOW=(
  "vos-lua::baseline calculator_vm_create_time_and_memory" # a measurement, run on demand (AGOS-53)
)

# The `slow:` tests by name: the `fn` within three lines after each `ignore = "slow:`. The count check catches an
# attribute too far from its `fn` (no name), a window that reaches the next test (two names) and two slow tests of
# the same name (one name).
pattern='ignore[[:space:]]*=[[:space:]]*"slow:'
attributes=$(grep -rEc --include='*.rs' "$pattern" crates | awk -F: '{ n += $NF } END { print n + 0 }')
names=$(grep -rEh --include='*.rs' -A3 "$pattern" crates \
  | sed -n -E 's/^[[:space:]]*(pub )?(async )?fn ([A-Za-z0-9_]+).*/\3/p' | sort -u)
count=$(printf '%s\n' "$names" | grep -c . || true)
[ "$count" -gt 0 ] || fail "found no slow: test under crates/"
[ "$count" -eq "$attributes" ] ||
  fail "found $attributes slow: attributes but $count test names; keep each attribute within three lines above its fn, and give each slow test its own name"

# Every ignored test is a slow: test or on NOT_SLOW, and every slow: test is ignored.
ignored=$(cargo nextest list --workspace --exclude neoc --run-ignored only --message-format oneline) ||
  fail "cargo nextest list failed"
unexpected=() seen=()
while IFS= read -r line; do
  [ -n "$line" ] || continue
  test_name=${line##* }
  if printf '%s\n' "$names" | grep -qxF -- "${test_name##*::}"; then
    seen+=("${test_name##*::}")
  else
    known=no
    for entry in "${NOT_SLOW[@]}"; do [ "$line" = "$entry" ] && known=yes; done
    [ "$known" = yes ] || unexpected+=("$line")
  fi
done <<<"$ignored"
[ "${#unexpected[@]}" -eq 0 ] ||
  fail "ignored tests that are neither slow: (as this script reads the attribute) nor on NOT_SLOW: ${unexpected[*]-}. Spell the attribute #[ignore = \"slow: ...\"] within three lines above the fn, or add the test to NOT_SLOW with its reason"
missing=()
while IFS= read -r name; do
  printf '%s\n' ${seen[@]+"${seen[@]}"} | grep -qxF -- "$name" || missing+=("$name")
done <<<"$names"
[ "${#missing[@]}" -eq 0 ] || fail "slow: tests that nextest does not list as ignored: ${missing[*]-}"

filter=$(printf '%s\n' "$names" | awk '{ printf "%stest(/(^|::)%s$/)", (NR > 1 ? " | " : ""), $0 }')
if [ "$mode" = list ]; then
  printf 'slow tests (%s):\n%s\nfilter: %s\n' "$count" "$names" "$filter"
  exit 0
fi

step "slow tests ($count): cargo nextest run --workspace --exclude neoc --run-ignored only -E '$filter'"
cargo nextest run --workspace --exclude neoc --run-ignored only --no-fail-fast -E "$filter"

step "cargo test --release -p neoc-vm --test abort_regressions"
cargo test --release -p neoc-vm --test abort_regressions

step "bench bodies, each once"
cargo bench -p neoc-vm --features bench --bench lib --bench vnd -- --test

printf '\nslow suites passed\n'
