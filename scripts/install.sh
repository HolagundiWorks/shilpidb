#!/bin/sh
# ShilpiDB installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/HolagundiWorks/shilpidb/main/scripts/install.sh | sh
#
# Downloads the shilpid + shilpi binaries for this OS/arch from the latest
# (or SHILPIDB_VERSION-pinned) GitHub Release and installs them to
# SHILPIDB_INSTALL_DIR (default: $HOME/.local/bin).
#
# Env vars:
#   SHILPIDB_VERSION      release tag to install, e.g. "v0.1.1" (default: latest)
#   SHILPIDB_INSTALL_DIR  install directory (default: "$HOME/.local/bin")

set -eu

REPO="HolagundiWorks/shilpidb"
INSTALL_DIR="${SHILPIDB_INSTALL_DIR:-$HOME/.local/bin}"

log() { printf '%s\n' "$*" >&2; }
die() {
    log "error: $*"
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found on PATH"
}

need uname
need tar
need mkdir
need mktemp

fetch() {
    # fetch URL > stdout, trying curl then wget.
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$1"
    else
        die "need either curl or wget to download"
    fi
}

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
    Linux)
        case "$arch" in
            x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
            *) die "unsupported Linux architecture: $arch (only x86_64 is published today)" ;;
        esac
        ;;
    Darwin)
        case "$arch" in
            x86_64) target="x86_64-apple-darwin" ;;
            arm64) target="aarch64-apple-darwin" ;;
            *) die "unsupported macOS architecture: $arch" ;;
        esac
        ;;
    *)
        die "unsupported OS: $os (this script covers Linux and macOS; see scripts/install.ps1 for Windows)"
        ;;
esac

version="${SHILPIDB_VERSION:-}"
if [ -z "$version" ]; then
    log "Looking up the latest release..."
    api_response="$(fetch "https://api.github.com/repos/$REPO/releases/latest")"
    version="$(printf '%s' "$api_response" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
    [ -n "$version" ] || die "could not determine the latest release tag from the GitHub API"
fi

archive="shilpidb-${version}-${target}.tar.gz"
url="https://github.com/$REPO/releases/download/${version}/${archive}"

log "Installing ShilpiDB ${version} (${target}) to ${INSTALL_DIR}..."

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

fetch "$url" >"$tmp_dir/$archive" || die "download failed: $url"
tar xzf "$tmp_dir/$archive" -C "$tmp_dir"

extracted_dir="$tmp_dir/shilpidb-${version}-${target}"
[ -f "$extracted_dir/shilpid" ] || die "archive didn't contain shilpid as expected"
[ -f "$extracted_dir/shilpi" ] || die "archive didn't contain shilpi as expected"

mkdir -p "$INSTALL_DIR"
cp "$extracted_dir/shilpid" "$INSTALL_DIR/shilpid"
cp "$extracted_dir/shilpi" "$INSTALL_DIR/shilpi"
chmod +x "$INSTALL_DIR/shilpid" "$INSTALL_DIR/shilpi"

log ""
log "Installed:"
log "  $INSTALL_DIR/shilpid"
log "  $INSTALL_DIR/shilpi"
log ""

case ":$PATH:" in
    *":$INSTALL_DIR:"*)
        "$INSTALL_DIR/shilpid" --version
        "$INSTALL_DIR/shilpi" --version
        ;;
    *)
        log "Note: $INSTALL_DIR is not on your PATH. Add it, e.g.:"
        log "  export PATH=\"$INSTALL_DIR:\$PATH\""
        log "(add that line to your shell profile to make it permanent)"
        ;;
esac

log ""
log "Run 'shilpid --help' to start the server and 'shilpi --help' for the shell."
