#!/bin/bash
# Install script for neoc.
# Usage: curl -fsSL https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.sh | bash
#
# Environment variables:
#   NEOC_INSTALL_DIR  — where to install (default: ~/.neoc/bin)
#   NEOC_VERSION      — version tag to install (default: latest)

set -euo pipefail

REPO="flying-dice/neoc"
BINARY="neoc"
INSTALL_DIR="${NEOC_INSTALL_DIR:-$HOME/.neoc/bin}"
VERSION="${NEOC_VERSION:-latest}"

detect_platform() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) ;;
        *)
            echo "error: unsupported OS: $os" >&2
            echo "       releases ship Linux x86_64 and Windows x86_64 binaries" >&2
            echo "       (use install.ps1 on Windows)" >&2
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64|amd64) ;;
        *)
            echo "error: unsupported architecture: $arch" >&2
            exit 1
            ;;
    esac

    echo "x86_64-unknown-linux-gnu"
}

resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
            | grep '"tag_name"' \
            | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
        if [ -z "$VERSION" ]; then
            echo "error: could not resolve latest version" >&2
            exit 1
        fi
    fi
    echo "$VERSION"
}

main() {
    local target version artifact_name base_url tmp_dir expected actual

    echo "Installing neoc..."

    target="$(detect_platform)"
    version="$(resolve_version)"
    artifact_name="neoc-${target}"

    echo "  Platform: $target"
    echo "  Version:  $version"
    echo "  Install:  $INSTALL_DIR"

    base_url="https://github.com/$REPO/releases/download/${version}"

    tmp_dir="$(mktemp -d)"
    trap "rm -rf '$tmp_dir'" EXIT

    echo "  Downloading $base_url/$artifact_name ..."
    if ! curl -fsSL "$base_url/$artifact_name" -o "$tmp_dir/$BINARY" \
        || ! curl -fsSL "$base_url/SHA256SUMS" -o "$tmp_dir/SHA256SUMS"; then
        echo "error: download failed — check that version '$version' exists" >&2
        echo "       and has a release asset for '$target'" >&2
        exit 1
    fi

    expected="$(awk -v f="$artifact_name" '$2 == f {print $1}' "$tmp_dir/SHA256SUMS")"
    actual="$(sha256sum "$tmp_dir/$BINARY" | awk '{print $1}')"
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "error: checksum mismatch for $artifact_name" >&2
        exit 1
    fi

    mkdir -p "$INSTALL_DIR"
    mv "$tmp_dir/$BINARY" "$INSTALL_DIR/$BINARY"
    chmod +x "$INSTALL_DIR/$BINARY"

    echo ""
    echo "neoc $version installed to $INSTALL_DIR/$BINARY"

    # Check if install dir is on PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo ""
            echo "Add to your PATH:"
            echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
            echo ""
            echo "Or add to your shell profile:"
            shell_name="$(basename "$SHELL")"
            case "$shell_name" in
                zsh)  echo "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.zshrc" ;;
                bash) echo "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.bashrc" ;;
                fish) echo "  fish_add_path $INSTALL_DIR" ;;
                *)    echo "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.\${SHELL}rc" ;;
            esac
            ;;
    esac
}

main
