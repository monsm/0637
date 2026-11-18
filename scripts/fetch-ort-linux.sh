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

# The GitHub release is the canonical source, but it disappeared on
# 2026-11-18 (the onnxruntime repository answers 404; the release CDN is
# unreachable from CI). The PyPI wheel of the same version carries the same
# shared library under onnxruntime/capi, so it is the mirror: fetch the
# wheel, take the .so out, and lay it out exactly like the release tarball
# would have. The version stays pinned; nothing about the runtime changes.
fetch_from_pypi() {
  local wheel_dir="$DEST/wheel"
  mkdir -p "$wheel_dir"
  # Read the PyPI metadata with the stdlib only: the runner's python3 may
  # not ship pip (ubuntu 24.04 is PEP 668), and all this needs is the URL.
  local url
  url="$(python3 - "$VERSION" <<'PY'
import json, sys, urllib.request
version = sys.argv[1]
with urllib.request.urlopen(
    f"https://pypi.org/pypi/onnxruntime/{version}/json", timeout=60
) as response:
    payload = json.load(response)
for entry in payload["urls"]:
    name = entry["filename"]
    if "manylinux" in name and "x86_64" in name:
        print(entry["url"])
        break
PY
)" || true
  if [ -z "$url" ]; then
    echo "error: no linux x86_64 wheel for onnxruntime==$VERSION" >&2
    return 1
  fi
  local wheel="$wheel_dir/$(basename "$url")"
  curl -L --fail --retry 3 -o "$wheel" "$url"
  rm -rf "$wheel_dir/extracted"
  unzip -q -o "$wheel" -d "$wheel_dir/extracted"
  local src
  src="$(ls "$wheel_dir"/extracted/onnxruntime/capi/libonnxruntime.so* \
    2>/dev/null | head -1)"
  if [ -z "$src" ] || [ ! -s "$src" ]; then
    echo "error: no libonnxruntime.so in $wheel" >&2
    return 1
  fi
  mkdir -p "$DEST/lib"
  cp -L "$src" "$DEST/lib/libonnxruntime.so.$VERSION"
  ln -sf "libonnxruntime.so.$VERSION" "$DEST/lib/libonnxruntime.so.1"
  ln -sf libonnxruntime.so.1 "$DEST/lib/libonnxruntime.so"
  echo "$DEST/lib/libonnxruntime.so"
}

if [ ! -f "$TARBALL" ]; then
  if ! curl -L --fail --retry 3 -o "$TARBALL" \
    "https://github.com/microsoft/onnxruntime/releases/download/v$VERSION/onnxruntime-linux-x64-$VERSION.tgz"; then
    echo "github release unavailable for $VERSION; falling back to the pypi wheel" >&2
    rm -f "$TARBALL"
    fetch_from_pypi
    exit $?
  fi
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
