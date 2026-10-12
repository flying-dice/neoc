#!/usr/bin/env bash
# Gates the demo packages under packages/*:
#   1. Freshness: `neoc build` in every package with a neoc.toml, then fail if
#      the committed src/auto output differs (modified or untracked files).
#   2. Host: run every package's `start` script and fail on a non-zero exit,
#      so a broken `require` of the generated module turns the job red.
# Env: NEOC — path to the neoc binary (default: target/debug/neoc).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

neoc=${NEOC:-$root/target/debug/neoc}
[[ -x "$neoc" ]] || { echo "check-packages: neoc binary not found at $neoc (set NEOC)" >&2; exit 1; }

shopt -s nullglob
manifests=(packages/*/neoc.toml)
[[ ${#manifests[@]} -gt 0 ]] || { echo "check-packages: no packages/*/neoc.toml found" >&2; exit 1; }

status=0
for manifest in "${manifests[@]}"; do
  pkg=$(dirname "$manifest")
  echo "== $pkg: neoc build"
  (cd "$pkg" && "$neoc" build) || { echo "$pkg: neoc build failed" >&2; status=1; continue; }

  if [[ -n $(git status --porcelain -- "$pkg/src/auto") ]]; then
    echo "$pkg: committed src/auto is stale; run \`neoc build\` in $pkg and commit the result" >&2
    git --no-pager diff -- "$pkg/src/auto" >&2 || true
    git status --porcelain -- "$pkg/src/auto" >&2
    status=1
  fi

  start=
  [[ -f "$pkg/package.json" ]] && start=$(jq -r '.scripts.start // empty' "$pkg/package.json")
  if [[ -n "$start" ]]; then
    echo "== $pkg: $start"
    (cd "$pkg" && bash -c "$start" >/dev/null) || { echo "$pkg: start host failed: $start" >&2; status=1; }
  fi
done
exit $status
