#!/usr/bin/env bash
# Build MuJoCo 3.10 as a static library for the Android ABIs the app ships.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${MUJOCO_VERSION:-3.10.0}"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}/ndk/27.3.13750724}}"
SRC="$ROOT/android/third_party/mujoco-src"
OUT="$ROOT/android/third_party/mujoco"

if [ ! -d "$SRC" ]; then
  git clone --depth 1 --branch "$VERSION" \
    https://github.com/google-deepmind/mujoco.git "$SRC"
fi

# MuJoCo hardcodes `add_library(mujoco SHARED ...)`; the mobile core links the
# library in, so build a static archive instead of a .so.
if ! grep -q "DUCK_STATIC_MUJOCO" "$SRC/CMakeLists.txt"; then
  python3 - "$SRC/CMakeLists.txt" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
needle = "add_library(mujoco SHARED ${MUJOCO_RESOURCE_FILES})"
replacement = (
    "# DUCK_STATIC_MUJOCO: link into the mobile core instead of a .so\n"
    "add_library(mujoco STATIC ${MUJOCO_RESOURCE_FILES})"
)
if needle not in text:
    raise SystemExit(f"static-library patch point not found in {path}")
open(path, "w").write(text.replace(needle, replacement, 1))
PY
fi

# bionic doesn't define _POSIX_C_SOURCE, which MuJoCo's localtime_r platform
# check requires; localtime_r itself is available on Android, so add it to the
# POSIX branch directly (flag-based fixes get overridden by the NDK toolchain).
ERRMEM="$SRC/src/engine/engine_util_errmem.c"
if ! grep -q "DUCK_ANDROID_LOCALTIME" "$ERRMEM"; then
  python3 - "$ERRMEM" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
needle = ("#if defined(_POSIX_C_SOURCE) || defined(__APPLE__) || "
          "defined(__STDC_VERSION_TIME_H__) || defined(__EMSCRIPTEN__)")
replacement = needle + " || defined(__ANDROID__)  // DUCK_ANDROID_LOCALTIME"
if needle not in text:
    raise SystemExit(f"localtime patch point not found in {path}")
open(path, "w").write(text.replace(needle, replacement, 1))
PY
fi

for abi in arm64-v8a x86_64; do
  BUILD="$OUT/build-$abi"
  if [ -f "$OUT/$abi/lib/libmujoco.a" ]; then
    echo "MuJoCo cached for $abi — skipping build"
  else
  # A restored cache may hold a pre-patch (shared/.so) configure. Start clean
  # so the patched CMakeLists and flags actually take effect.
  rm -rf "$BUILD"
  local_log="$OUT/cmake-$abi.log"
  mkdir -p "$OUT"
  if ! cmake -S "$SRC" -B "$BUILD" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$abi" \
    -DANDROID_PLATFORM=android-28 \
    -DANDROID_STL=c++_static \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=OFF \
    -DCMAKE_SKIP_INSTALL_RULES=ON \
    -DMUJOCO_BUILD_EXAMPLES=OFF \
    -DMUJOCO_BUILD_TESTS=OFF \
    -DMUJOCO_BUILD_SIMULATE=OFF \
    -DMUJOCO_BUILD_PYTHON=OFF \
    -DMUJOCO_BUILD_CSHARP=OFF \
    >"$local_log" 2>&1; then
    echo "::error::cmake configure failed for $abi — see step summary"
    tail -n 200 "$local_log" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || cat "$local_log"
    exit 1
  fi

  build_log="$OUT/build-$abi.log"
  if ! cmake --build "$BUILD" --target mujoco -j"$(nproc)" \
    >"$build_log" 2>&1; then
    echo "::error::cmake build failed for $abi — see step summary"
    tail -n 200 "$build_log" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || cat "$build_log"
    exit 1
  fi

  mkdir -p "$OUT/$abi/lib" "$OUT/$abi/include/mujoco"
  LIBRARY="$(find "$BUILD" -name 'libmujoco.a' -print -quit)"
  if [ -z "$LIBRARY" ]; then
    echo "error: libmujoco.a not found under $BUILD" >&2
    exit 1
  fi
  cp "$LIBRARY" "$OUT/$abi/lib/"
  cp "$SRC"/include/mujoco/*.h "$OUT/$abi/include/mujoco/"
  fi

  # A static MuJoCo archive does not absorb its own dependencies: copy the
  # ones it needs next to it and record the link order, dependents first.
  # Each dependency may ship under several archive names depending on how its
  # CMake reacts to BUILD_SHARED_LIBS=OFF; take the first that exists. The
  # allowlist matters — qhull also builds libqhull.a and libqhullstatic.a,
  # which would collide with the renamed qhullstatic_r MuJoCo links. This runs
  # for cached builds too: the archives are in the cached build tree.
  collect_dep() {
    local lib_dir="$1" build="$2" logical="$3"; shift 3
    for name in "$@"; do
      local found
      found="$(find "$build" -name "lib$name.a" -print -quit)"
      if [ -n "$found" ]; then
        cp "$found" "$lib_dir/"
        echo "$name" >> "$lib_dir/libs.txt"
        return 0
      fi
    done
    echo "::warning::no archive for $logical (tried: lib${*}.a) under $build"
  }
  mkdir -p "$OUT/$abi/lib"
  echo "mujoco" > "$OUT/$abi/lib/libs.txt"
  collect_dep "$OUT/$abi/lib" "$BUILD" lodepng lodepng
  collect_dep "$OUT/$abi/lib" "$BUILD" miniz miniz
  collect_dep "$OUT/$abi/lib" "$BUILD" tinyxml2 tinyxml2
  collect_dep "$OUT/$abi/lib" "$BUILD" qhull qhullstatic_r qhull_r
  collect_dep "$OUT/$abi/lib" "$BUILD" ccd ccd ccd_static
  echo "MuJoCo link order: $(tr '\n' ' ' < "$OUT/$abi/lib/libs.txt")"
done

echo "MuJoCo $VERSION built for Android"
