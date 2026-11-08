#!/usr/bin/env bash
# Install the Android SDK packages the build needs, with public diagnostics.
set -uo pipefail

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
SDKM="$SDK_ROOT/cmdline-tools/latest/bin/sdkmanager"
LOG="$PWD/sdk-packages.log"

echo "SDK_ROOT=$SDK_ROOT"
echo "sdkmanager present: $(test -x "$SDKM" && echo yes || echo no)"
ls "$SDK_ROOT/cmdline-tools/latest/bin" 2>&1 | head -n 20 || true

yes | "$SDKM" --sdk_root="$SDK_ROOT" --licenses > /dev/null 2>&1 || true

if ! "$SDKM" --sdk_root="$SDK_ROOT" "platform-tools" "platforms;android-35" \
  "build-tools;35.0.0" "ndk;27.2.12479018" "cmake;3.22.1" \
  >"$LOG" 2>&1; then
  echo "::error::sdkmanager failed — see step summary"
  tail -n 200 "$LOG" >> "${GITHUB_STEP_SUMMARY:-/dev/null}" 2>/dev/null || cat "$LOG"
  exit 1
fi

sudo apt-get update -qq
sudo apt-get install -y -qq ninja-build cmake
echo "Android SDK packages installed"
