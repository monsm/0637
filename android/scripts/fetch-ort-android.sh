#!/usr/bin/env bash
# Fetch the Android ONNX Runtime AAR and copy its shared libraries into jniLibs.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${ORT_ANDROID_VERSION:-1.24.2}"
DEST="$ROOT/android/third_party"
AAR="$DEST/onnxruntime-android-$VERSION.aar"
JNILIBS="$ROOT/android/app/src/main/jniLibs"

mkdir -p "$DEST" "$JNILIBS"
if [ ! -f "$AAR" ]; then
  curl -L --fail --retry 3 -o "$AAR" \
    "https://repo1.maven.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/$VERSION/onnxruntime-android-$VERSION.aar"
fi

rm -rf "$DEST/ort-android"
unzip -q "$AAR" -d "$DEST/ort-android"
for abi in arm64-v8a x86_64; do
  if [ -d "$DEST/ort-android/jni/$abi" ]; then
    mkdir -p "$JNILIBS/$abi"
    cp "$DEST"/ort-android/jni/$abi/*.so "$JNILIBS/$abi/"
  fi
done
echo "ONNX Runtime $VERSION copied into jniLibs"
