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

# Take the lib/ directory out of the archive, leaving the version out of
# the path, so the destination is the same whatever version is fetched.
#
# tar may report an error while doing this — in the archive the plain name
# is a symlink to libonnxruntime.so.1, which is itself a symlink to the
# versioned library, and a link whose target has not been written yet
# cannot be created everywhere. The libraries themselves are written
# either way; the links are repaired below.
tar -xzf "$TARBALL" -C "$DEST" --strip-components=1 \
  "onnxruntime-linux-x64-$VERSION/lib" || true

LIBDIR="$DEST/lib"
real="$(ls "$LIBDIR"/libonnxruntime.so.1.* 2>/dev/null | head -1 || true)"
if [ -z "$real" ]; then
  echo "error: no libonnxruntime.so.1.* in $TARBALL" >&2
  exit 1
fi
if [ ! -s "$real" ]; then
  echo "error: $real is empty" >&2
  exit 1
fi

# Point the two names a loader might look for at the library that is here.
ln -sf "$(basename "$real")" "$LIBDIR/libonnxruntime.so.1"
ln -sf libonnxruntime.so.1 "$LIBDIR/libonnxruntime.so"

LIB="$LIBDIR/libonnxruntime.so"
if [ ! -f "$LIB" ]; then
  echo "error: expected $LIB" >&2
  exit 1
fi
echo "ONNX Runtime $VERSION -> $LIB" >&2
echo "$LIB"
