#!/usr/bin/env bash
# Fetch the official policy set into policies/.
#
# The files are not in the repository: they are the released MicroDuck
# policies, published by Pollen/Hugging Face, and the manifest references
# all of them. A missing file silently removes a feature (crouch needs
# roller_crouch, the stand policy is the fallback walk slot), so the whole
# set is fetched every time, with the manifest last.
#
# Usage: scripts/fetch-policies.sh [destination]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/policies}"
BASE="${DUCK_POLICY_BASE:-https://huggingface.co/pollen-robotics/microduck-policies/resolve/main}"

mkdir -p "$DEST"

for f in velstand.onnx alpha_walking.onnx alpha_stand.onnx \
         roller.onnx alpha_sitstand.onnx alpha_ground_pick.onnx \
         roller_crouch.onnx roulade.onnx \
         ball_kick_left.onnx ball_kick_right.onnx manifest.json; do
  if [ -s "$DEST/$f" ]; then
    continue
  fi
  curl -L --fail --retry 3 -o "$DEST/$f" "$BASE/$f"
done

echo "official policies in $DEST:"
ls -la "$DEST"
