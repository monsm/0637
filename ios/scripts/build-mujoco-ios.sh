#!/usr/bin/env bash
# Build MuJoCo 3.10.0 as a static library for the iOS simulator and device.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${MUJOCO_VERSION:-3.10.0}"
SRC="$ROOT/ios/third_party/mujoco-src"

if [ ! -d "$SRC" ]; then
  git clone --depth 1 --branch "$VERSION" \
    https://github.com/google-deepmind/mujoco.git "$SRC"
fi

build() {
  local sysroot="$1"
  local out="$2"
  local build="$out/build"

  # Cached from a previous run — nothing to do.
  if [ -f "$out/lib/libmujoco.a" ]; then
    echo "MuJoCo cached at $out — skipping build"
    return 0
  fi

  local log="$build-cmake.log"
  mkdir -p "$(dirname "$log")"
  if ! cmake -S "$SRC" -B "$build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_SYSROOT="$sysroot" \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=17.0 \
    -DMUJOCO_BUILD_EXAMPLES=OFF \
    -DMUJOCO_BUILD_TESTS=OFF \
    -DMUJOCO_BUILD_SIMULATE=OFF \
    -DMUJOCO_BUILD_PYTHON=OFF \
    -DMUJOCO_BUILD_CSHARP=OFF \
    >"$log" 2>&1; then
    echo "::error::cmake configure failed — see step summary"
    tail -n 200 "$log" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || cat "$log"
    exit 1
  fi

  local blog="$build-build.log"
  if ! cmake --build "$build" --target mujoco -j"$(sysctl -n hw.ncpu)" \
    >"$blog" 2>&1; then
    echo "::error::cmake build failed — see step summary"
    tail -n 200 "$blog" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || cat "$blog"
    exit 1
  fi

  mkdir -p "$out/lib" "$out/include/mujoco"
  local library
  library="$(find "$build" -name 'libmujoco.a' -print -quit)"
  if [ -z "$library" ]; then
    echo "error: libmujoco.a not found under $build" >&2
    exit 1
  fi
  cp "$library" "$out/lib/"
  cp "$SRC"/include/mujoco/*.h "$out/include/mujoco/"
}

build iphonesimulator "$ROOT/ios/third_party/mujoco-sim"
build iphoneos "$ROOT/ios/third_party/mujoco-device"
echo "MuJoCo $VERSION built for simulator and device"
