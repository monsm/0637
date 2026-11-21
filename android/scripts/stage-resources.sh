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
mkdir -p "$DST/robot/microduck/assets" "$DST/policies" "$DST/models"
cp "$SRC"/*.xml "$DST/robot/microduck/"
cp "$SRC"/assets/*.stl "$DST/robot/microduck/assets/"
cp "$ROOT"/policies/*.onnx "$DST/policies/"
[ -f "$ROOT/policies/manifest.json" ] && cp "$ROOT/policies/manifest.json" "$DST/policies/"
# The pickup classifier is the one model that is not a policy: robotd ships it
# with duck-control, not with the policy set.
PICKUP="$ROOT/upstream/microduck/duck-control/models/pickup_detector.onnx"
[ -f "$PICKUP" ] && cp "$PICKUP" "$DST/models/pickup_detector.onnx"
# The official scratch-sound classifier for POST /sensor pcm16 (J3/J1). The
# core ignores the stream when the model is absent, so staging it is the
# shell's whole half of the contract.
PET="$ROOT/upstream/microduck/pet-detect/models/pet_detect.onnx"
if [ -f "$PET" ]; then
  mkdir -p "$DST/pet-detect"
  cp "$PET" "$DST/pet-detect/pet_detect.onnx"
fi
# `robotd/emotions/`: the real duck's thirteen keyframe timelines and their
# 48 kHz voices. `robot.do curious` is a skill, so the app needs the data.
EMOTIONS="$ROOT/upstream/microduck/robotd/emotions"
if [ -d "$EMOTIONS" ]; then
  mkdir -p "$DST/emotions"
  cp "$EMOTIONS"/*.json "$EMOTIONS"/*.wav "$DST/emotions/"
fi

echo "staged Android assets"
du -sh "$DST"
