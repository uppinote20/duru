#!/usr/bin/env bash
# duru installer — https://github.com/uppinote20/duru
# Usage: curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/uppinote20/duru/main/install.sh | bash

set -euo pipefail

REPO="uppinote20/duru"
BINARY_NAME="duru"
INSTALL_DIR="${DURU_INSTALL_DIR:-$HOME/.local/bin}"
TMP_DIR=""

err() {
    echo "Error: $1" >&2
    exit 1
}

detect_platform() {
    local os arch
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Darwin)
            case "$arch" in
                arm64)  TARGET="aarch64-apple-darwin" ;;
                x86_64) TARGET="x86_64-apple-darwin" ;;
                *)      err "Unsupported macOS architecture: $arch" ;;
            esac
            ;;
        Linux)
            case "$arch" in
                x86_64)        TARGET="x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
                *)             err "Unsupported Linux architecture: $arch" ;;
            esac
            ;;
        *)
            err "Unsupported OS: $os (download Windows binaries from GitHub Releases)"
            ;;
    esac

    echo "Detected platform: $TARGET"
}

get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')

    if [ -z "$VERSION" ]; then
        err "Failed to fetch latest version"
    fi

    echo "Latest version: $VERSION"
}

cleanup() {
    if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}

download_and_install() {
    local archive url sha256_url

    archive="${BINARY_NAME}-${TARGET}.tar.gz"
    url="https://github.com/$REPO/releases/download/$VERSION/$archive"
    sha256_url="${url}.sha256"

    TMP_DIR=$(mktemp -d)
    trap cleanup EXIT

    echo "Downloading $archive..."
    curl -fsSL "$url" -o "$TMP_DIR/$archive"
    curl -fsSL "$sha256_url" -o "$TMP_DIR/$archive.sha256"

    echo "Verifying checksum..."
    cd "$TMP_DIR"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c "$archive.sha256"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -c "$archive.sha256"
    else
        echo "Warning: no sha256sum or shasum found, skipping verification"
    fi
    cd - >/dev/null

    echo "Extracting..."
    tar xzf "$TMP_DIR/$archive" -C "$TMP_DIR"

    mkdir -p "$INSTALL_DIR"
    mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
}

check_path() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo
            echo "NOTE: $INSTALL_DIR is not in your PATH."
            echo "Add it by running:"
            echo
            echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
            echo
            echo "Or add the line above to your shell profile (~/.bashrc, ~/.zshrc, etc.)"
            ;;
    esac
}

main() {
    echo "Installing duru..."
    echo

    detect_platform
    get_latest_version
    download_and_install

    echo
    echo "duru $VERSION installed to $INSTALL_DIR/$BINARY_NAME"

    check_path
}

main
