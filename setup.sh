#!/usr/bin/env bash
# sqlrestore installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/sqlrestore/main/setup.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/sqlrestore/main/setup.sh | bash -s -- --prefix=$HOME/.local
#
# Env vars:
#   VERSION   pin a specific version (default: latest)
#   PREFIX    install dir (default: /usr/local; falls back to ~/.local if not writable)

set -euo pipefail

REPO="wolfsoftwaresystemsltd/sqlrestore"
VERSION="${VERSION:-latest}"
PREFIX="${PREFIX:-/usr/local}"

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix=*)  PREFIX="${1#*=}" ;;
        --prefix)    PREFIX="$2"; shift ;;
        --version=*) VERSION="${1#*=}" ;;
        --version)   VERSION="$2"; shift ;;
        -h|--help)
            sed -n '2,10p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
    shift
done

err() { echo "sqlrestore-install: $*" >&2; exit 1; }
log() { echo "sqlrestore-install: $*"; }

# --- platform detection ---
OS="$(uname -s)"
[ "$OS" = "Linux" ] || err "unsupported OS: $OS (this installer is for Linux only)"

case "$(uname -m)" in
    x86_64|amd64)        ARCH="linux-x86_64" ;;
    aarch64|arm64)       ARCH="linux-aarch64" ;;
    *) err "unsupported architecture: $(uname -m)" ;;
esac

# --- required tools ---
for cmd in curl tar uname mktemp; do
    command -v "$cmd" >/dev/null 2>&1 || err "missing required command: $cmd"
done
DL=""
if command -v curl >/dev/null 2>&1; then DL="curl -fsSL"; fi

# --- resolve version ---
if [ "$VERSION" = "latest" ]; then
    log "resolving latest release"
    TAG=$($DL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' \
        | head -n1 | sed 's/.*"\([^"]*\)"$/\1/')
    [ -n "$TAG" ] || err "could not determine latest release tag"
else
    TAG="$VERSION"
    case "$TAG" in v*) ;; *) TAG="v$TAG" ;; esac
fi
VER="${TAG#v}"
ARCHIVE="sqlrestore-${VER}-${ARCH}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"
SHA_URL="${URL}.sha256"

log "installing sqlrestore ${TAG} (${ARCH})"

# --- pick install dir ---
if [ ! -w "$PREFIX/bin" ] 2>/dev/null && [ ! -w "$PREFIX" ] 2>/dev/null; then
    if [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        log "$PREFIX not writable; falling back to $HOME/.local"
        PREFIX="$HOME/.local"
        SUDO=""
    fi
else
    SUDO=""
fi
BIN_DIR="$PREFIX/bin"

# --- download, verify, install ---
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

log "downloading $URL"
$DL "$URL"     -o "$TMP/$ARCHIVE"      || err "download failed: $URL"
$DL "$SHA_URL" -o "$TMP/$ARCHIVE.sha256" || log "no sha256 alongside release; skipping verify"

if [ -s "$TMP/$ARCHIVE.sha256" ] && command -v sha256sum >/dev/null 2>&1; then
    log "verifying sha256"
    ( cd "$TMP" && sha256sum -c "$ARCHIVE.sha256" ) || err "sha256 mismatch"
fi

tar -xzf "$TMP/$ARCHIVE" -C "$TMP"
[ -f "$TMP/sqlrestore" ] || err "binary not found in archive"
chmod +x "$TMP/sqlrestore"

$SUDO mkdir -p "$BIN_DIR"
$SUDO install -m 0755 "$TMP/sqlrestore" "$BIN_DIR/sqlrestore"

log "installed: $BIN_DIR/sqlrestore"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) log "note: $BIN_DIR is not in PATH — add: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac

"$BIN_DIR/sqlrestore" --help | head -n 3 || true
log "done."
