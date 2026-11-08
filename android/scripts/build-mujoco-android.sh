#!/usr/bin/env bash
# Build MuJoCo 3.10 as a static library for the Android ABIs the app ships.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${MUJOCO_VERSION:-3.10.0}"
NDK="${ANDROID_NDK_HOME:-${ANDROID_HOME:-$HOME/Android/Sdk}/ndk/27.2.12479018}"
SRC="$ROOT/android/third_party/mujoco-src"
OUT="$ROOT/android/third_party/mujoco"

if [ ! -d "$SRC" ]; then
  git clone --depth 1 --branch "$VERSION" \
    https://github.com/google-deepmind/mujoco.git "$SRC"
fi

for abi in arm64-v8a x86_64; do
  BUILD="$OUT/build-$abi"
  cmake -S "$SRC" -B "$BUILD" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$abi" \
    -DANDROID_PLATFORM=android-26 \
    -DANDROID_STL=c++_static \
    -DCMAKE_BUILD_TYPE=Release \
    -DMUJOCO_BUILD_EXAMPLES=OFF \
    -DMUJOCO_BUILD_TESTS=OFF \
    -DMUJOCO_BUILD_SIMULATE=OFF \
    -DMUJOCO_BUILD_PYTHON=OFF \
    -DMUJOCO_BUILD_CSHARP=OFF

  cmake --build "$BUILD" --target mujoco -j"$(nproc)"

  mkdir -p "$OUT/$abi/lib" "$OUT/$abi/include/mujoco"
  LIBRARY="$(find "$BUILD" -name 'libmujoco.a' -print -quit)"
  if [ -z "$LIBRARY" ]; then
    echo "error: libmujoco.a not found under $BUILD" >&2
    exit 1
  fi
  cp "$LIBRARY" "$OUT/$abi/lib/"
  cp "$SRC"/include/mujoco/*.h "$OUT/$abi/include/mujoco/"
done

echo "MuJoCo $VERSION built for Android"
