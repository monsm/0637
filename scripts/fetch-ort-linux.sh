#!/usr/bin/env bash
# Fetch the ONNX Runtime shared library for a Linux host.
#
# The core loads the official ONNX policies through `ort`'s dynamic loader,
# so a host that wants to run the policy — the desktop harness, or the
# core's own test suite — needs libonnxruntime. The mobile builds carry
# their own slices (the iOS XCFramework, the Android AAR); this is the
# Linux x64 one, and it is the same version they use.
#
# Usage: scripts/fetch-ort-linux.sh [destination]
# Prints the path of the shared library on stdout.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${ORT_LINUX_VERSION:-1.24.2}"
DEST="${1:-$ROOT/work/ort-linux}"
TARBALL="$DEST/onnxruntime-linux-x64-$VERSION.tgz"

mkdir -p "$DEST"
if [ ! -f "$TARBALL" ]; then
  curl -L --fail --retry 3 -o "$TARBALL" \
    "https://github.com/microsoft/onnxruntime/releases/download/v$VERSION/onnxruntime-linux-x64-$VERSION.tgz"
fi

# The archive holds one directory with the version in its name; take the
# library out of it and leave the version out of the path, so the
# destination is the same whatever version is fetched.
tar -xzf "$TARBALL" -C "$DEST" --strip-components=1 \
  "onnxruntime-linux-x64-$VERSION/lib/libonnxruntime.so"

LIB="$DEST/lib/libonnxruntime.so"
if [ ! -f "$LIB" ]; then
  echo "error: expected $LIB" >&2
  exit 1
fi
echo "ONNX Runtime $VERSION -> $LIB" >&2
echo "$LIB"
