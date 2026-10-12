#!/bin/bash
# Stamp a release version into the workspace Cargo.toml.
# Usage: scripts/stamp-version.sh v1.2.3[-pre]
#
# Accepts tags of the form v<major>.<minor>.<patch> with an optional
# -<pre> suffix, rewrites [workspace.package] version in place, and prints
# the bare version (v1.2.3 -> 1.2.3). Used by the release workflow on its
# CI checkout only; nothing is committed back.

set -euo pipefail

tag="${1:-}"
re='^v([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?)$'
if [[ ! "$tag" =~ $re ]]; then
    echo "error: '$tag' is not a release tag (expected vMAJOR.MINOR.PATCH[-pre])" >&2
    exit 1
fi
version="${BASH_REMATCH[1]}"

manifest="$(cd "$(dirname "$0")/.." && pwd)/Cargo.toml"
sed -i -E '0,/^version = ".*"$/s//version = "'"$version"'"/' "$manifest"

if [ "$(grep -c "^version = \"$version\"\$" "$manifest")" -ne 1 ]; then
    echo "error: could not stamp version into $manifest" >&2
    exit 1
fi

echo "$version"
