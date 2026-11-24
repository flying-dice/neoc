#!/usr/bin/env bash
# The slow suites: everything that is kept out of `bun run gate` because it costs minutes, not seconds. Run it
# (`bun run slow`) before a PR that changes what these suites guard, and when you want the full sweep; it is not
# part of the pre-PR gate, a review or a fix round (iteration speed is a requirement).
#
# One mechanism for every slow suite: a test that is too slow for the gate is `#[ignore = "slow: ..."]`, and this
# script runs those tests and no other ignored one. It finds them by that reason: the function after each
# `ignore = "slow:` under `crates/` (any spacing, plain or inside a `cfg_attr`, tracked by git or not), in the test
# binary its file compiles into, and runs each by binary id and full test path. The `neoc` crate is left out: its
# cucumber harness takes no libtest flags and has no ignored test. Add a new slow test with
# `#[ignore = "slow: run with bun run slow"]` within three lines above its `fn`; nothing here changes.
#
# It fails closed. It checks its selection against `cargo nextest list --run-ignored only`: every ignored test must be
# one of the `slow:` tests found above or on NOT_SLOW below (a test ignored for another reason: the `vos-lua`
# baseline, a measurement run on demand, and the VM's tests that need Docker, a database server or the credential
# store), and every `slow:` test must be one nextest ignores. Anything else (another spelling of the attribute, an
# attribute too far from its `fn`, a new test ignored without a reason) stops the script with a message. `--list`
# does only that check and prints the selection (a dry run: no slow suite runs).
#
# What runs today: lib:template's 60 000-macro ring (a wall-clock bound in a debug build).
#
# The first failure stops the run. Needs the pinned Rust toolchain and cargo-nextest; not bun, darklua or
# luau-lsp (no step builds an app or type-checks a script).
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
  # need a running Docker daemon
  "neoc-vm modules::vnd::bollard::tests::a_real_daemon_answers"
  "neoc-vm modules::vnd::bollard::tests::a_real_daemon_runs_a_container_from_pull_to_removal"
  "neoc-vm modules::vnd::sqlx_mysql::tests::against_a_container"
  "neoc-vm modules::vnd::sqlx_postgres::tests::against_a_container"
  "neoc-vm modules::vnd::testcontainers_modules::tests::generic_containers_start_publish_their_ports_and_stop"
  "neoc-vm modules::vnd::testcontainers_modules::tests::mysql_and_postgres_containers_answer_their_drivers"
  # need a server: NEOC_TEST_MYSQL_URL, NEOC_TEST_POSTGRES_URL
  "neoc-vm modules::vnd::sqlx_mysql::tests::against_a_server"
  "neoc-vm modules::vnd::sqlx_postgres::tests::against_a_server"
  # writes to the credential store of whoever runs it
  "neoc-vm modules::vnd::keyring::tests::the_platform_store_keeps_a_credential"
)

# The nextest binary id of the test target a source file compiles into: `crates/<dir>/tests/<name>.rs` (or
# `tests/<name>/main.rs`) is `<package>::<name>`, a file under `src/bin/<name>` is `<package>::bin/<name>`, and any other
# file under `src/` is the library, `<package>`. Prints nothing for any other place (a module shared by test targets).
binary_id() {
  local file=$1 dir rel package
  dir=${file#crates/} && dir=${dir%%/*}
  rel=${file#crates/"$dir"/}
  package=$(sed -n -E 's/^name[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "crates/$dir/Cargo.toml" | head -n 1)
  [ -n "$package" ] || return 0
  case $rel in
    tests/*/main.rs) rel=${rel#tests/} && printf '%s::%s\n' "$package" "${rel%/main.rs}" ;;
    tests/*/*) ;;
    tests/*.rs) rel=${rel#tests/} && printf '%s::%s\n' "$package" "${rel%.rs}" ;;
    src/bin/*) rel=${rel#src/bin/} && rel=${rel%%/*} && printf '%s::bin/%s\n' "$package" "${rel%.rs}" ;;
    src/*) printf '%s\n' "$package" ;;
  esac
}

# The `slow:` tests as `<binary id> <fn>`: the `fn` within three lines after each `ignore = "slow:`, in the test binary
# of its file. The count check catches an attribute too far from its `fn` (no name), a window that reaches the next
# test (two names) and two slow tests of the same name in one binary (one entry).
pattern='ignore[[:space:]]*=[[:space:]]*"slow:'
attributes=$(grep -rEc --include='*.rs' "$pattern" crates | awk -F: '{ n += $NF } END { print n + 0 }')
entries=()
while IFS= read -r file; do
  binary=$(binary_id "$file")
  [ -n "$binary" ] || fail "$file has a slow: test, but this script cannot name its test binary (see binary_id)"
  while IFS= read -r name; do entries+=("$binary $name"); done < <(grep -EA3 "$pattern" "$file" |
    sed -n -E 's/^[[:space:]]*(pub )?(async )?fn ([A-Za-z0-9_]+).*/\3/p')
done < <(grep -rlE --include='*.rs' "$pattern" crates | sort)
slow=$(printf '%s\n' ${entries[@]+"${entries[@]}"} | sort -u)
count=$(printf '%s\n' "$slow" | grep -c . || true)
[ "$count" -gt 0 ] || fail "found no slow: test under crates/"
[ "$count" -eq "$attributes" ] ||
  fail "found $attributes slow: attributes but $count test names; keep each attribute within three lines above its fn, and give each slow test in a binary its own name"

# Every ignored test is a slow: test or on NOT_SLOW, and every slow: test is ignored. An ignored test is a slow: one
# when its binary id and the last segment of its path are a slow: entry; it is then selected by binary id and full
# path, so an ignored test of the same name in another binary is not run as slow. Two ignored tests of one binary
# that end in the same slow: name stop the script instead of guessing which one it is.
ignored=$(cargo nextest list --workspace --exclude neoc --run-ignored only --message-format oneline) ||
  fail "cargo nextest list failed"
selected=() unexpected=() seen=()
while IFS= read -r line; do
  [ -n "$line" ] || continue
  binary=${line%% *} path=${line#* }
  key="$binary ${path##*::}"
  if printf '%s\n' "$slow" | grep -qxF -- "$key"; then
    if printf '%s\n' ${seen[@]+"${seen[@]}"} | grep -qxF -- "$key"; then
      fail "two ignored tests in $binary end in ${path##*::}; give the slow: one a name of its own"
    fi
    seen+=("$key") selected+=("$line")
  else
    known=no
    for entry in "${NOT_SLOW[@]}"; do [ "$line" = "$entry" ] && known=yes; done
    [ "$known" = yes ] || unexpected+=("$line")
  fi
done <<<"$ignored"
[ "${#unexpected[@]}" -eq 0 ] ||
  fail "ignored tests that are neither slow: (as this script reads the attribute) nor on NOT_SLOW: ${unexpected[*]-}. Spell the attribute #[ignore = \"slow: ...\"] within three lines above the fn, or add the test to NOT_SLOW with its reason"
missing=()
while IFS= read -r key; do
  printf '%s\n' ${seen[@]+"${seen[@]}"} | grep -qxF -- "$key" || missing+=("$key")
done <<<"$slow"
[ "${#missing[@]}" -eq 0 ] || fail "slow: tests that nextest does not list as ignored: ${missing[*]-}"

filter=$(printf '%s\n' "${selected[@]}" | awk '{ printf "%s(binary_id(=%s) & test(=%s))", (NR > 1 ? " | " : ""), $1, $2 }')
if [ "$mode" = list ]; then
  printf 'slow tests (%s):\n' "$count"
  printf '%s\n' "${selected[@]}"
  printf 'filter: %s\n' "$filter"
  exit 0
fi

step "slow tests ($count): cargo nextest run --workspace --exclude neoc --run-ignored only -E '$filter'"
cargo nextest run --workspace --exclude neoc --run-ignored only --no-fail-fast -E "$filter"

printf '\nslow suites passed\n'
