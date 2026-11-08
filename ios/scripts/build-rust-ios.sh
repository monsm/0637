#!/usr/bin/env bash
# Build the Rust core as an iOS dylib and copy it into ios/Frameworks for Xcode.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET="${1:-aarch64-apple-ios-sim}"

case "$TARGET" in
  *-sim) MUJOCO="$ROOT/ios/third_party/mujoco-sim" ;;
  *)     MUJOCO="$ROOT/ios/third_party/mujoco-device" ;;
esac

export MUJOCO_INCLUDE_DIR="$MUJOCO/include"
export MUJOCO_LIB_DIR="$MUJOCO/lib"
export ORT_IOS_XCFWK_LOCATION="$ROOT/ios/third_party/onnxruntime.xcframework"
export ORT_CXX_STDLIB=libc++
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-install_name,@rpath/libduck_mobile_core.dylib"

rustup target add "$TARGET" >/dev/null 2>&1 || true

cargo build \
  --manifest-path "$ROOT/core/Cargo.toml" \
  -p duck-mobile-core \
  --lib \
  --release \
  --no-default-features \
  --features mujoco,onnx-static \
  --target "$TARGET"

mkdir -p "$ROOT/ios/Frameworks"
cp "$ROOT/core/target/$TARGET/release/libduck_mobile_core.dylib" "$ROOT/ios/Frameworks/"
echo "built $TARGET -> ios/Frameworks/libduck_mobile_core.dylib"
