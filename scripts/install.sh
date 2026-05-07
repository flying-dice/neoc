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
    local os arch target

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os="unknown-linux-gnu" ;;
        Darwin) os="apple-darwin" ;;
        *)
            echo "error: unsupported OS: $os" >&2
            echo "       use the PowerShell script on Windows" >&2
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)   arch="aarch64" ;;
        *)
            echo "error: unsupported architecture: $arch" >&2
            exit 1
            ;;
    esac

    target="${arch}-${os}"
    echo "$target"
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
    local target version artifact_name download_url tmp_dir

    echo "Installing neoc..."

    target="$(detect_platform)"
    version="$(resolve_version)"
    artifact_name="neoc-${target}"

    echo "  Platform: $target"
    echo "  Version:  $version"
    echo "  Install:  $INSTALL_DIR"

    download_url="https://github.com/$REPO/releases/download/${version}/${artifact_name}.tar.gz"

    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    echo "  Downloading $download_url ..."
    if ! curl -fsSL "$download_url" -o "$tmp_dir/archive.tar.gz"; then
        echo "error: download failed — check that version '$version' exists" >&2
        echo "       and has a release asset for '$target'" >&2
        exit 1
    fi

    tar -xzf "$tmp_dir/archive.tar.gz" -C "$tmp_dir"

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
