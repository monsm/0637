#!/usr/bin/env bash
# Download Microsoft's ONNX Runtime iOS XCFramework (CocoaPods archive).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${ORT_IOS_VERSION:-1.24.2}"
DEST="$ROOT/ios/third_party"
FRAMEWORK="$DEST/onnxruntime.xcframework"

mkdir -p "$DEST"
if [ ! -d "$FRAMEWORK" ]; then
  ARCHIVE="$DEST/onnxruntime-ios-$VERSION.zip"
  echo "downloading ONNX Runtime $VERSION iOS xcframework..."
  curl -L --fail --retry 3 -o "$ARCHIVE" \
    "https://download.onnxruntime.ai/pod-archive-onnxruntime-c-$VERSION.zip"
  unzip -q "$ARCHIVE" -d "$DEST"
fi

if [ ! -d "$FRAMEWORK" ]; then
  echo "error: expected $FRAMEWORK" >&2
  exit 1
fi
echo "$FRAMEWORK"
