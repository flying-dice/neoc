#!/usr/bin/env sh
set -eu

VERSION="__VERSION__"
INSTALL_DIR="${NEOC_INSTALL_DIR:-$HOME/.neoc/bin}"
BASE_URL="https://gitlab.beluga-sirius.ts.net/api/v4/projects/13/packages/generic/neoc/${VERSION}"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}/${ARCH}" in
  Linux/x86_64)
    BINARY="neoc-x86_64-unknown-linux-gnu"
    ;;
  *)
    echo "Unsupported platform: ${OS}/${ARCH}" >&2
    echo "Download manually from the releases page:" >&2
    echo "  https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/releases/${VERSION}" >&2
    exit 1
    ;;
esac

mkdir -p "${INSTALL_DIR}"
curl -fsSL "${BASE_URL}/${BINARY}" -o "${INSTALL_DIR}/neoc"
chmod +x "${INSTALL_DIR}/neoc"

echo "Installed neoc ${VERSION} -> ${INSTALL_DIR}/neoc"
echo ""
echo "Add ${INSTALL_DIR} to your PATH if it is not already there:"
echo "  export PATH=\"\$PATH:${INSTALL_DIR}\""
