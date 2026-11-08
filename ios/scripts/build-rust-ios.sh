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
# ort-sys passes this value straight to `-l`; it must be the link name (`c++`),
# not the file name — `libc++` would turn into `-llibc++`.
export ORT_CXX_STDLIB=c++

# rustc links with -nodefaultlibs, which also suppresses the driver's SDK
# setup: without an explicit sysroot, ld searches no SDK and `-lc++` fails.
case "$TARGET" in
  *-sim) SDK_NAME=iphonesimulator ;;
  *)     SDK_NAME=iphoneos ;;
esac
SDK="$(xcrun --sdk "$SDK_NAME" --show-sdk-path)"
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-isysroot -C link-arg=$SDK -C link-arg=-Wl,-install_name,@rpath/libduck_mobile_core.dylib"

# rustup picks the toolchain from the working directory, and
# core/rust-toolchain.toml is what pins 1.99.1 — so the build has to run from
# inside core/. RUSTUP_TOOLCHAIN pins it regardless of where cargo is called.
export RUSTUP_TOOLCHAIN=1.99.1
rustup target add --toolchain 1.99.1 "$TARGET"
rustup target list --toolchain 1.99.1 --installed

LOG="$ROOT/ios/third_party/rust-$TARGET.log"
mkdir -p "$(dirname "$LOG")"
cd "$ROOT/core"
if ! cargo build \
  -p duck-mobile-core \
  --lib \
  --release \
  --no-default-features \
  --features mujoco,onnx-static \
  --target "$TARGET" \
  >"$LOG" 2>&1; then
  echo "::error::cargo build failed"
  # Print the tail to the job log too: the step summary is not reachable
  # from the API, the job log is.
  tail -n 120 "$LOG" || cat "$LOG"
  tail -n 200 "$LOG" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || true
  exit 1
fi

mkdir -p "$ROOT/ios/Frameworks"
cp "$ROOT/core/target/$TARGET/release/libduck_mobile_core.dylib" "$ROOT/ios/Frameworks/"
echo "built $TARGET -> ios/Frameworks/libduck_mobile_core.dylib"
