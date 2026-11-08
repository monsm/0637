#!/usr/bin/env bash
# Build the Rust core for Android and copy the .so into jniLibs.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}/ndk/27.3.13750724}}"
export ANDROID_NDK_HOME="$NDK"

export RUSTUP_TOOLCHAIN=1.99.1
rustup target add --toolchain 1.99.1 aarch64-linux-android x86_64-linux-android
rustup target list --toolchain 1.99.1 --installed
if ! command -v cargo-ndk >/dev/null 2>&1; then
  cargo install cargo-ndk --locked
fi

# cargo-ndk runs `cargo metadata` in the current directory, so it must run
# inside the Cargo workspace root (`core/`).
cd "$ROOT/core"

for abi in arm64-v8a x86_64; do
  export MUJOCO_INCLUDE_DIR="$ROOT/android/third_party/mujoco/$abi/include"
  export MUJOCO_LIB_DIR="$ROOT/android/third_party/mujoco/$abi/lib"
  TRIPLE=$([ "$abi" = "arm64-v8a" ] && echo aarch64-linux-android || echo x86_64-linux-android)
  # The NDK's libdl.a is bionic's static-executable shim: besides the dl
  # entry points it carries malloc/locale/errno/atexit objects whose globals
  # are never initialised in a shared library (scudo divides by zero on the
  # first malloc). Rebuild the archive with only the dl member and put it
  # first on the search path.
  TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
  STUB="$ROOT/core/target/android-libdl-stub/$abi"
  mkdir -p "$STUB"
  rm -f "$STUB"/*.o "$STUB/libdl.a"
  (
    cd "$STUB"
    "$TOOLCHAIN/bin/llvm-ar" x "$TOOLCHAIN/sysroot/usr/lib/$TRIPLE/libdl.a"
    "$TOOLCHAIN/bin/llvm-ar" rcs libdl.a $(ls | grep libdl_static)
  )
  echo "libdl stub for $abi:"
  "$TOOLCHAIN/bin/llvm-ar" t "$STUB/libdl.a"
  ls -la "$STUB"
  export DUCK_LIBDL_STUB_DIR="$STUB"
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-Map=$ROOT/android/third_party/link-$abi.map"
  LOG="$ROOT/android/third_party/rust-$abi.log"
  # --lib only: the package's sim binaries are desktop tools and drag in
  # link requirements the app does not need.
  # --platform must match minSdk (29): the default API 21 stubs lack libc
  # symbols that libc++abi references (pthread_atfork), and API 28 lacks ELF
  if ! cargo ndk \
    -t "$abi" \
    --platform 29 \
    -o "$ROOT/android/app/src/main/jniLibs" \
    build \
    -v \
    -p duck-mobile-core \
    --lib \
    --release \
    --no-default-features \
    --features mujoco,onnx-dynamic \
    >"$LOG" 2>&1; then
    echo "::error::cargo ndk build failed for $abi"
    # Print the tail to the job log too: the step summary is not reachable
    # from the API, the job log is.
    tail -n 120 "$LOG" || cat "$LOG"
    tail -n 200 "$LOG" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || true
    exit 1
  fi
done

echo "Rust Android core copied into jniLibs"
