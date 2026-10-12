#!/usr/bin/env bash
# Fails unless rust-toolchain.toml pins an exact `1.x.y` and every other site
# that names a Rust version agrees with it: the Dockerfile builder image,
# and any `.github` workflow. A toolchain bump
# is an explicit PR that edits all sites and fixes lint fallout.
set -euo pipefail
cd "$(dirname "$0")/.."

channel=$(sed -nE "s/^channel[[:space:]]*=[[:space:]]*[\"']([^\"']*)[\"'].*/\1/p" rust-toolchain.toml)
if ! [[ "$channel" =~ ^1\.[0-9]+\.[0-9]+$ ]]; then
  echo "rust-toolchain.toml: channel must be an exact 1.x.y, got '${channel}'" >&2
  exit 1
fi

status=0
files=(Dockerfile)
while IFS= read -r f; do files+=("$f"); done < <(find .github -type f \( -name '*.yml' -o -name '*.yaml' \) 2>/dev/null)

for f in "${files[@]}"; do
  # Every `rust:<tag>` image reference must be `<channel>` or `<channel>-<variant>`.
  while IFS= read -r tag; do
    if [[ "$tag" != "$channel" && "$tag" != "${channel}-"* ]]; then
      echo "${f}: image 'rust:${tag}' does not match pinned ${channel}" >&2
      status=1
    fi
  done < <(grep -oE '(^|[[:space:]"'"'"'])rust:[A-Za-z0-9._-]+' "$f" | sed -E 's/.*rust://')
  # Every `toolchain:` value and `dtolnay/rust-toolchain@<ref>` must be the channel.
  while IFS= read -r v; do
    if [[ "$v" != "$channel" ]]; then
      echo "${f}: toolchain '${v}' does not match pinned ${channel}" >&2
      status=1
    fi
  done < <({ grep -oE 'toolchain:[[:space:]]*["'"'"']?[A-Za-z0-9._-]+' "$f" | sed -E "s/toolchain:[[:space:]]*[\"']?//"
             grep -oE 'dtolnay/rust-toolchain@[A-Za-z0-9._-]+' "$f" | sed 's/.*@//'; } || true)
  if grep -nE '\+(stable|beta|nightly)' "$f" >&2; then
    echo "${f}: floating '+channel' override" >&2
    status=1
  fi
done

if [[ -z "$(grep -ohE '(^|[[:space:]])rust:[A-Za-z0-9._-]+' Dockerfile)" ]]; then
  echo "Dockerfile: no 'rust:<tag>' builder image found" >&2
  status=1
fi

[[ $status -eq 0 ]] && echo "toolchain pinned at ${channel}"
exit $status
