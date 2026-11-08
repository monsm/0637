#!/usr/bin/env bash
# Emulator smoke test: install the APK, launch it, and verify the process
# survives and the core's intent API answers. Runs on the host (adb only).
#
# The API answering on :8765 proves the whole native stack came up: MuJoCo
# loaded the scene, the ONNX policies loaded, and the 50 Hz loop is ticking.
set -uo pipefail

APK="android/app/build/outputs/apk/debug/app-debug.apk"

# The emulator may report boot before system_server is fully up; installs
# fail with "Can't find service: activity" until then.
adb wait-for-device
for _ in $(seq 1 60); do
  [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '
')" = "1" ] && break
  sleep 5
done

INSTALL_OK=0
for _ in $(seq 1 3); do
  if adb install -r "$APK"; then
    INSTALL_OK=1
    break
  fi
  sleep 10
done
[ "$INSTALL_OK" = "1" ] || { echo "::error::apk install failed"; exit 1; }
adb logcat -c
adb shell am start -n ai.duck.microduck/.MainActivity | tee am-start.log

# Wait for the process to appear (ActivityManager may take a while), then
# give the core time to load MuJoCo and the policies.
PID=""
for _ in $(seq 1 30); do
  PID=$(adb shell pidof ai.duck.microduck | tr -d '\r')
  [ -n "$PID" ] && break
  sleep 2
done
sleep 15

if [ -z "$PID" ]; then
  adb logcat -d > android-logcat.log || true
  echo "::error::app process is not running — it crashed on open"
  grep -nE "FATAL|AndroidRuntime|signal |Abort message" android-logcat.log | head -40 || true
  exit 1
fi
if ! adb shell pidof ai.duck.microduck | tr -d '\r' | grep -q .; then
  adb logcat -d > android-logcat.log || true
  echo "::error::app died during startup"
  grep -nE "FATAL|AndroidRuntime|signal |Abort message" android-logcat.log | head -40 || true
  exit 1
fi
echo "app alive with pid $PID"

adb reverse tcp:8765 tcp:8765
STATE=""
for _ in $(seq 1 120); do
  STATE=$(curl -sf --max-time 5 http://127.0.0.1:8765/state || true)
  [ -n "$STATE" ] && break
  sleep 5
done

adb logcat -d > android-logcat.log || true
if [ -z "$STATE" ]; then
  echo "::error::intent API did not answer — the core is not ticking"
  grep -nE "duck-rust|FATAL|AndroidRuntime|signal |onnx|mujoco|policy" android-logcat.log | tail -40 || true
  echo "--- core stages ---"
  grep -E "duck-rust" android-logcat.log | tail -20 || true
  grep -nE "ai.duck.microduck" android-logcat.log | grep -viE "zygote|ActivityManager|WindowManager|SplashScreen|AppsFilter|MediaProvider|Avrcp" | tail -20 || true
  exit 1
fi
echo "$STATE" | tee intent-state.json
curl -sf --max-time 5 http://127.0.0.1:8765/skills | tee intent-skills.json || true
echo "smoke test passed"
