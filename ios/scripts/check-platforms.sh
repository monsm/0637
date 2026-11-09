#!/usr/bin/env bash
# Assert every Mach-O inside an app bundle targets the expected platform.
#
# Why this exists: a device build that embeds a simulator slice installs fine
# and then shows a black screen — dyld refuses to load the wrong-platform
# image and the app dies before SwiftUI ever draws a frame. The simulator
# cannot reproduce it, because the simulator build is a different slice, and
# `lipo -info` cannot tell the two apart either (both are arm64). Only the
# LC_BUILD_VERSION platform field says for sure which world a binary belongs
# to.
#
# Usage: check-platforms.sh <app-bundle> <IOS|IOSSIMULATOR>
set -uo pipefail

APP="${1:?usage: check-platforms.sh <app-bundle> <IOS|IOSSIMULATOR>}"
EXPECTED="${2:?usage: check-platforms.sh <app-bundle> <IOS|IOSSIMULATOR>}"

case "$EXPECTED" in
  IOS)          WANT_PLATFORM=2 ;;
  IOSSIMULATOR) WANT_PLATFORM=7 ;;
  *) echo "::error::unknown platform '$EXPECTED' (use IOS or IOSSIMULATOR)"; exit 2 ;;
esac

if [ ! -d "$APP" ]; then
  echo "::error::app bundle not found: $APP"
  exit 1
fi

# platform numbers from <mach-o/loader.h>
describe_platform() {
  case "$1" in
    1) echo "macOS" ;;
    2) echo "iOS" ;;
    3) echo "tvOS" ;;
    4) echo "watchOS" ;;
    5) echo "bridgeOS" ;;
    6) echo "macCatalyst" ;;
    7) echo "iOS-Simulator" ;;
    8) echo "tvOS-Simulator" ;;
    9) echo "watchOS-Simulator" ;;
    *) echo "unknown($1)" ;;
  esac
}

# The platform of a Mach-O, from LC_BUILD_VERSION. Older images carry
# LC_VERSION_MIN_IPHONEOS instead, which means iOS proper.
platform_of() {
  local binary="$1"
  local platform
  platform=$(otool -l "$binary" 2>/dev/null \
    | awk '/LC_BUILD_VERSION/{seen=1} seen && /^[[:space:]]*platform[[:space:]]+[0-9]+/{print $2; exit}')
  if [ -n "$platform" ]; then
    echo "$platform"
    return
  fi
  if otool -l "$binary" 2>/dev/null | grep -q LC_VERSION_MIN_IPHONEOS; then
    echo 2
    return
  fi
  echo ""
}

FAILED=0
CHECKED=0

while IFS= read -r -d '' binary; do
  # Only look at real Mach-O images; the bundle also holds meshes, policies
  # and ONNX weights. The magics cover thin images (both endiannesses) and
  # fat archives.
  case "$(head -c 4 "$binary" | od -An -tx1 | tr -d ' \n')" in
    cffaedfe|cefaedfe|feedfacf|feedface|cafebabe|bebafeca) ;;
    *) continue ;;
  esac

  platform=$(platform_of "$binary")
  CHECKED=$((CHECKED + 1))
  if [ -z "$platform" ]; then
    echo "::error::no LC_BUILD_VERSION in $(basename "$binary") — cannot verify its platform"
    FAILED=1
    continue
  fi
  if [ "$platform" != "$WANT_PLATFORM" ]; then
    echo "::error::$(basename "$binary") is built for $(describe_platform "$platform"), expected $(describe_platform "$WANT_PLATFORM")"
    echo "::error::  path: ${binary#"$APP"/}"
    FAILED=1
  else
    echo "ok  $(describe_platform "$platform")  ${binary#"$APP"/}"
  fi
done < <(find "$APP" -type f -print0 2>/dev/null)

if [ "$CHECKED" -eq 0 ]; then
  echo "::error::no Mach-O binaries found in $APP"
  exit 1
fi

if [ "$FAILED" -ne 0 ]; then
  echo "::error::$CHECKED binaries checked, at least one has the wrong platform"
  echo "::error::the app would install and then show a black screen on the device"
  exit 1
fi
echo "all $CHECKED binaries in $APP target $(describe_platform "$WANT_PLATFORM")"
