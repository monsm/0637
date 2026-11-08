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

# MuJoCo hardcodes `add_library(mujoco SHARED ...)`; the mobile core links the
# library in, so build a static archive instead of a dylib.
if ! grep -q "DUCK_STATIC_MUJOCO" "$SRC/CMakeLists.txt"; then
  python3 - "$SRC/CMakeLists.txt" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
needle = "add_library(mujoco SHARED ${MUJOCO_RESOURCE_FILES})"
replacement = (
    "# DUCK_STATIC_MUJOCO: link into the mobile core instead of a dylib\n"
    "add_library(mujoco STATIC ${MUJOCO_RESOURCE_FILES})"
)
if needle not in text:
    raise SystemExit(f"static-library patch point not found in {path}")
open(path, "w").write(text.replace(needle, replacement, 1))
PY
fi

# MuJoCo adds -mmacosx-version-min=11 whenever APPLE is true; clang rejects
# that flag on iOS targets (-mios-simulator-version-min conflict). The flags
# are used both by enforce_mujoco_macosx_min_version() and directly by the
# mujoco target, so patch the variable definitions: iOS takes the empty else()
# branch instead.
MACOS_CMAKE="$SRC/cmake/MujocoMacOS.cmake"
if ! grep -q "DUCK_IOS_NO_MACOS_MIN_VARS" "$MACOS_CMAKE"; then
  if ! python3 - "$MACOS_CMAKE" >"$SRC/patch.log" 2>&1 <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
needle = "if(APPLE)\n  # Target the oldest version of macOS"
replacement = (
    "if(APPLE AND NOT CMAKE_SYSTEM_NAME STREQUAL \"iOS\")\n"
    "  # DUCK_IOS_NO_MACOS_MIN_VARS: -mmacosx-version-min conflicts with iOS targets\n"
    "  # Target the oldest version of macOS"
)
if needle not in text:
    raise SystemExit(f"patch point not found in {path}")
open(path, "w").write(text.replace(needle, replacement, 1))
PY
  then
    echo "::error::MujocoMacOS patch failed — see ci-logs/patch.log"
    exit 1
  fi
fi

build() {
  local sysroot="$1"
  local out="$2"
  local build="$out/build"

  # Cached from a previous run — nothing to build, but the dependency
  # archives still have to be collected below.
  if [ -f "$out/lib/libmujoco.a" ]; then
    echo "MuJoCo cached at $out — skipping build"
  else
  # A restored cache may hold a pre-patch (shared/dylib) configure. Start clean
  # so the patched CMakeLists and flags actually take effect.
  rm -rf "$build"

  local name
  name="$(basename "$out")"
  local log="$out/$name-cmake.log"
  mkdir -p "$(dirname "$log")"
  if ! cmake -S "$SRC" -B "$build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_SYSROOT="$sysroot" \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=17.0 \
    -DCMAKE_MACOSX_BUNDLE=OFF \
    -DCMAKE_PROJECT_INCLUDE="$ROOT/ios/scripts/no-bundle.cmake" \
    -DCMAKE_SKIP_INSTALL_RULES=ON \
    -DBUILD_SHARED_LIBS=OFF \
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

  local blog="$out/$name-build.log"
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
  fi

  # A static MuJoCo archive does not absorb its own dependencies: copy the
  # ones it needs next to it and record the link order, dependents first.
  # The allowlist matters — qhull also builds libqhull.a and libqhullstatic.a,
  # which would collide with the renamed libqhull_r.a MuJoCo links. This runs
  # for cached builds too: the archives are in the cached build tree.
  mkdir -p "$out/lib"
  echo "mujoco" > "$out/lib/libs.txt"
  for dep in lodepng miniz tinyxml2 qhull_r; do
    dep_found="$(find "$build" -name "lib$dep.a" -print -quit)"
    if [ -z "$dep_found" ]; then
      echo "::warning::lib$dep.a not found under $build"
      continue
    fi
    cp "$dep_found" "$out/lib/"
    echo "$dep" >> "$out/lib/libs.txt"
  done
  echo "MuJoCo link order: $(tr '\n' ' ' < "$out/lib/libs.txt")"
}

build iphonesimulator "$ROOT/ios/third_party/mujoco-sim"
if [ "${BUILD_DEVICE:-0}" = "1" ]; then
  build iphoneos "$ROOT/ios/third_party/mujoco-device"
  echo "MuJoCo $VERSION built for simulator and device"
else
  echo "MuJoCo $VERSION built for simulator (set BUILD_DEVICE=1 for device)"
fi
