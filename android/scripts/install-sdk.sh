#!/usr/bin/env bash
# Install the Android SDK packages the build needs, with public diagnostics.
set -uo pipefail

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/usr/local/lib/android/sdk}}"
SDKM="$SDK_ROOT/cmdline-tools/latest/bin/sdkmanager"
LOG="$PWD/sdk-packages.log"

echo "SDK_ROOT=$SDK_ROOT"
echo "sdkmanager present: $(test -x "$SDKM" && echo yes || echo no)"

# The GitHub-hosted image ships cmdline-tools; bootstrap only if absent.
if [ ! -x "$SDKM" ]; then
  echo "bootstrapping Android cmdline-tools"
  mkdir -p "$SDK_ROOT/cmdline-tools"
  curl -L --fail --retry 3 -o "$SDK_ROOT/tools.zip" \
    "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip"
  unzip -q -o "$SDK_ROOT/tools.zip" -d "$SDK_ROOT/cmdline-tools"
  rm -rf "$SDK_ROOT/cmdline-tools/latest"
  mv "$SDK_ROOT/cmdline-tools/cmdline-tools" "$SDK_ROOT/cmdline-tools/latest"
  SDKM="$SDK_ROOT/cmdline-tools/latest/bin/sdkmanager"
fi

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
