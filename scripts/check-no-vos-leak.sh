#!/usr/bin/env bash
# neoc and neoc-vm2 never depend on a vos-* crate, not even as a dev-dependency (no vos concept in the runtime).
# Run by scripts/check-neoc.sh; tested in scripts/check-no-vos-leak.test.ts with a stub `cargo`.
#
# The tree is captured first: a failing `cargo tree` must fail this check, not read as "no leak" (a pipeline inside an
# `if` would). The output must also list neoc-vm2 itself, or an empty tree would pass.
set -euo pipefail

tree=$(cargo tree -p neoc -p neoc-vm2 -e all --prefix none) || {
  echo "cargo tree failed: cannot check for a vos-* dependency" >&2
  exit 1
}
if ! grep -qE '^neoc-vm2 ' <<<"$tree"; then
  echo "cargo tree did not list neoc-vm2: cannot check for a vos-* dependency" >&2
  exit 1
fi
if grep -E '^vos-' <<<"$tree"; then
  echo "neoc and neoc-vm2 must not depend on a vos-* crate" >&2
  exit 1
fi
