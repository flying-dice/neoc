#!/usr/bin/env bash
# Fails unless rust-toolchain.toml pins an exact `1.x.y` and every other site
# that names a Rust version (the Dockerfile builder image) agrees with it.
# A toolchain bump is an explicit PR that edits all sites and fixes lint fallout.
set -euo pipefail
cd "$(dirname "$0")/.."

channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
if ! [[ "$channel" =~ ^1\.[0-9]+\.[0-9]+$ ]]; then
  echo "rust-toolchain.toml: channel must be an exact 1.x.y, got '${channel}'" >&2
  exit 1
fi

status=0
image=$(sed -n 's/^FROM rust:\([^ ]*\) .*/\1/p' Dockerfile | head -n1)
if [[ "$image" != "${channel}-"* ]]; then
  echo "Dockerfile: builder image 'rust:${image}' does not match pinned ${channel}" >&2
  status=1
fi

while IFS= read -r f; do
  if grep -nE 'rust:[0-9]+(-|$)|rust:(latest|stable)|toolchain: *(stable|beta|nightly)|\+stable' "$f"; then
    echo "${f}: floating Rust toolchain reference (pin to ${channel})" >&2
    status=1
  fi
done < <(find .github -type f \( -name '*.yml' -o -name '*.yaml' \) 2>/dev/null)

[[ $status -eq 0 ]] && echo "toolchain pinned at ${channel}"
exit $status
