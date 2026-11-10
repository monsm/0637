#!/usr/bin/env bash
# Stage the official MJCF, STL meshes and ONNX policies into the iOS bundle.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="$ROOT/upstream/microduck_rl/src/mjlab_microduck/robot/microduck"
DST="$ROOT/ios/DuckApp/DuckResources"

if [ ! -d "$SRC" ]; then
  echo "error: missing $SRC — clone microduck_rl into upstream/ first" >&2
  exit 1
fi
if [ ! -d "$ROOT/policies" ]; then
  echo "error: missing $ROOT/policies — download the official ONNX set first" >&2
  exit 1
fi

rm -rf "$DST"
mkdir -p "$DST/robot/microduck/assets" "$DST/policies" "$DST/models"
cp "$SRC"/*.xml "$DST/robot/microduck/"
cp "$SRC"/assets/*.stl "$DST/robot/microduck/assets/"
cp "$ROOT"/policies/*.onnx "$DST/policies/"
[ -f "$ROOT/policies/manifest.json" ] && cp "$ROOT/policies/manifest.json" "$DST/policies/"
# The pickup classifier is the one model that is not a policy: robotd ships it
# with duck-control, not with the policy set.
PICKUP="$ROOT/upstream/microduck/duck-control/models/pickup_detector.onnx"
[ -f "$PICKUP" ] && cp "$PICKUP" "$DST/models/pickup_detector.onnx"
# `robotd/emotions/`: the real duck's thirteen keyframe timelines and their
# 48 kHz voices. `robot.do curious` is a skill, so the app needs the data.
EMOTIONS="$ROOT/upstream/microduck/robotd/emotions"
if [ -d "$EMOTIONS" ]; then
  mkdir -p "$DST/emotions"
  cp "$EMOTIONS"/*.json "$EMOTIONS"/*.wav "$DST/emotions/"
fi

# The shader lives OUTSIDE $DST on purpose: the rm -rf above used to delete
# it, so the shipped bundle had no Metal library, the renderer returned nil,
# and the 3D view was pure black on every build. Copy it back in and fail
# loudly if it is gone.
SHADER="$ROOT/ios/DuckApp/Shaders/Shaders.metal"
if [ ! -f "$SHADER" ]; then
  echo "error: missing $SHADER - the app would ship without a Metal library and render a black view" >&2
  exit 1
fi
cp "$SHADER" "$DST/Shaders.metal"
test -f "$DST/Shaders.metal" || { echo "error: Shaders.metal did not land in $DST" >&2; exit 1; }

echo "staged resources into $DST"
du -sh "$DST"
