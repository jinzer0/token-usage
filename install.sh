#!/bin/sh
set -eu

REPO="jinzer0/token-usage"
BIN="token-usage"
INSTALL_DIR="${TOKEN_USAGE_INSTALL_DIR:-$HOME/.local/bin}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

need curl
need tar
need uname

os=$(uname -s)
arch=$(uname -m)

case "$os:$arch" in
  Linux:x86_64) target="x86_64-unknown-linux-gnu" ;;
  Darwin:arm64) target="aarch64-apple-darwin" ;;
  Darwin:x86_64) target="x86_64-apple-darwin" ;;
  *)
    echo "error: unsupported platform: $os $arch" >&2
    echo "supported: Linux x86_64, macOS arm64, macOS x86_64" >&2
    exit 1
    ;;
esac

api="https://api.github.com/repos/$REPO/releases/latest"
tag=$(curl -fsSL "$api" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
if [ -z "$tag" ]; then
  echo "error: could not determine latest release for $REPO" >&2
  exit 1
fi

archive="$BIN-$tag-$target.tar.gz"
base="https://github.com/$REPO/releases/download/$tag"
tmp=$(mktemp -d)
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT INT TERM

curl -fsSL "$base/$archive" -o "$tmp/$archive"

if curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"; then
  expected=$(grep "  $archive$" "$tmp/SHA256SUMS" | awk '{print $1}')
  if [ -n "$expected" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      actual=$(sha256sum "$tmp/$archive" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
      actual=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
    else
      actual=""
      echo "warning: SHA256SUMS found but neither sha256sum nor shasum is installed; skipping verification" >&2
    fi
    if [ -n "$actual" ] && [ "$expected" != "$actual" ]; then
      echo "error: checksum verification failed for $archive" >&2
      exit 1
    fi
  fi
fi

mkdir -p "$INSTALL_DIR"
tar -xzf "$tmp/$archive" -C "$tmp"
if [ ! -x "$tmp/$BIN" ]; then
  echo "error: archive did not contain executable $BIN" >&2
  exit 1
fi

mv "$tmp/$BIN" "$INSTALL_DIR/$BIN"
chmod +x "$INSTALL_DIR/$BIN"

if command -v "$INSTALL_DIR/$BIN" >/dev/null 2>&1; then
  "$INSTALL_DIR/$BIN" --version
else
  echo "installed $BIN to $INSTALL_DIR"
  echo "add $INSTALL_DIR to PATH to run: $BIN --version"
fi
