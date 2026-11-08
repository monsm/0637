#!/usr/bin/env bash
# Stage the official MJCF, STL meshes and ONNX policies into the Android assets.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="$ROOT/upstream/microduck_rl/src/mjlab_microduck/robot/microduck"
DST="$ROOT/android/app/src/main/assets"

if [ ! -d "$SRC" ]; then
  echo "error: missing $SRC" >&2
  exit 1
fi
if [ ! -d "$ROOT/policies" ]; then
  echo "error: missing $ROOT/policies" >&2
  exit 1
fi

rm -rf "$DST"
mkdir -p "$DST/robot/microduck/assets" "$DST/policies"
cp "$SRC"/*.xml "$DST/robot/microduck/"
cp "$SRC"/assets/*.stl "$DST/robot/microduck/assets/"
cp "$ROOT"/policies/*.onnx "$DST/policies/"
[ -f "$ROOT/policies/manifest.json" ] && cp "$ROOT/policies/manifest.json" "$DST/policies/"

echo "staged Android assets"
du -sh "$DST"
