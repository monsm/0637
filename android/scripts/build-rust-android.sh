#!/usr/bin/env bash
# Build the Rust core for Android and copy the .so into jniLibs.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}/ndk/27.3.13750724}}"
export ANDROID_NDK_HOME="$NDK"

rustup target add aarch64-linux-android x86_64-linux-android >/dev/null 2>&1 || true
if ! command -v cargo-ndk >/dev/null 2>&1; then
  cargo install cargo-ndk --locked
fi

# cargo-ndk runs `cargo metadata` in the current directory, so it must run
# inside the Cargo workspace root (`core/`).
cd "$ROOT/core"

for abi in arm64-v8a x86_64; do
  export MUJOCO_INCLUDE_DIR="$ROOT/android/third_party/mujoco/$abi/include"
  export MUJOCO_LIB_DIR="$ROOT/android/third_party/mujoco/$abi/lib"
  LOG="$ROOT/android/third_party/rust-$abi.log"
  if ! cargo ndk \
    -t "$abi" \
    -o "$ROOT/android/app/src/main/jniLibs" \
    build \
    -p duck-mobile-core \
    --release \
    --no-default-features \
    --features mujoco,onnx-dynamic \
    >"$LOG" 2>&1; then
    echo "::error::cargo ndk build failed for $abi — see step summary"
    tail -n 200 "$LOG" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || cat "$LOG"
    exit 1
  fi
done

echo "Rust Android core copied into jniLibs"
