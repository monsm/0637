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

# forward: host port -> device port. `adb reverse` is the opposite
# direction (device -> host) and would never reach the app's server.
adb forward tcp:8765 tcp:8765
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
SKILLS=$(curl -sf --max-time 5 http://127.0.0.1:8765/skills || true)
echo "$SKILLS" | tee intent-skills.json > /dev/null
# The verbs a client can discover: the manifest's one-shots and the loop's
# own. A verb `/skills` does not name is one nobody can find.
for name in sit stand ground_pick curious angry laugh yes; do
  if ! echo "$SKILLS" | grep -q "\"$name\""; then
    echo "::error::/skills does not list $name"
    exit 1
  fi
done
# An emotion is a scripted head/beak timeline with its own voice; /state
# publishes which one the loop is playing, so a dead loader cannot pass.
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"name":"curious"}' http://127.0.0.1:8765/skill > /dev/null || true
sleep 0.4
STATE_EMOTION=$(curl -sf --max-time 5 http://127.0.0.1:8765/state || true)
if ! echo "$STATE_EMOTION" | grep -q '"name":"curious"'; then
  echo "::error::/state.emotion does not report the curious timeline"
  exit 1
fi
if ! echo "$STATE_EMOTION" | grep -q '"last_emotion_sound":"emotion:curious"'; then
  echo "::error::the emotion's official wav never reached the sound queue"
  exit 1
fi
# The duck's own senses reach the API, not just its commands.
# The staged bundle must carry all 13 official timelines, not just publish
# their names: an AssetStager that skipped the directory would otherwise
# pass the earlier checks and fail only when a user taps an emotion.
STAGED=$(find android/app/src/main/assets/emotions -maxdepth 1 -name '*.json' 2>/dev/null | wc -l)
if [ "$STAGED" -lt 13 ]; then
  echo "::error::only $STAGED emotion timelines staged in the app bundle"
  exit 1
fi
echo "emotions staged: $STAGED"

for needle in '"proprioception"' '"positions"' '"imu_ready"' '"mouth"' '"performance"'; do
  if ! echo "$STATE" | grep -q "$needle"; then
    echo "::error::/state does not publish $needle"
    exit 1
  fi
done
# The platform's own measurements: after a few seconds the loop has ticked
# enough for the once-a-second report to be non-zero. A zero here means the
# counters are not wired, which is exactly what this check exists to catch.
sleep 2
PERF=$(curl -sf --max-time 5 http://127.0.0.1:8765/state || true)
TPS=$(echo "$PERF" | grep -oE '"ticks_per_second":[0-9.]+' | head -1 | cut -d: -f2)
case "$TPS" in
  ''|0|0.0)
    echo "::error::/state.performance.ticks_per_second is $TPS — the counters are not wired"
    exit 1
    ;;
esac
echo "performance: $TPS ticks/s"
# A still duck must actually glance. An emotion just ran and fades the idle
# spell out, so it needs up to ~3.5 s of stillness to come back; a single
# sample at a fixed 4 s races the runner. Poll for a real measurement.
IDLE="0 0"
for _ in $(seq 1 20); do
  IDLE=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try:
    d=json.load(sys.stdin).get('idle') or {}
    print(d.get('takeover',0), max(abs(x) for x in d.get('head',[0]) or [0]))
except Exception: print('0 0')
" 2>/dev/null || echo "0 0")
  GAIN=${IDLE%% *}; HEAD=${IDLE##* }
  python3 -c "
import sys
sys.exit(0 if float('${GAIN:-0}') > 0.8 and float('${HEAD:-0}') > 0.001 else 1)
" 2>/dev/null && break
  sleep 1
done
GAIN=${IDLE%% *}; HEAD=${IDLE##* }
python3 -c "
import sys
sys.exit(0 if float('${GAIN:-0}') > 0.8 and float('${HEAD:-0}') > 0.001 else 1)
" || { echo "::error::a still duck never looks around (idle=$IDLE)"; exit 1; }
echo "idle: takeover=$GAIN head=$HEAD"
# Refresh the published artifact: the first /state above can legitimately
# predate the once-a-second report and would leave a 0.0 in ci-logs.
echo "$PERF" > intent-state.json
# A one-shot answers with the tick's verdict: an unknown skill is refused in
# words rather than accepted and dropped.
REFUSED=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"name":"fly"}' http://127.0.0.1:8765/skill || true)
echo "$REFUSED" | tee intent-skill-refused.json > /dev/null
if ! echo "$REFUSED" | grep -q '"ok":false'; then
  echo "::error::an unknown skill was accepted: $REFUSED"
  exit 1
fi

# The build proves which scene, meshes and policies it is running.
FINGERPRINT=$(curl -sf --max-time 5 http://127.0.0.1:8765/fingerprint || true)
echo "$FINGERPRINT" | tee intent-fingerprint.json > /dev/null
if ! echo "$FINGERPRINT" | grep -qE '"fingerprint":"[0-9a-f]{64}"'; then
  echo "::error::the build fingerprint is missing or malformed"
  exit 1
fi
echo "fingerprint: $(echo "$FINGERPRINT" | grep -oE '"fingerprint":"[0-9a-f]{64}"')"

# The voice is the robot's own synth in the core: a sound request is answered
# by the tick, and an unknown tag is refused rather than silently dropped.
SOUND=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"tag":"chirp"}' http://127.0.0.1:8765/sound || true)
echo "$SOUND" | tee intent-sound.json > /dev/null
if ! echo "$SOUND" | grep -q '"ok":true'; then
  echo "::error::the duck cannot be asked to chirp: $SOUND"
  exit 1
fi
if curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"tag":"bark"}' http://127.0.0.1:8765/sound | grep -q '"ok":true'; then
  echo "::error::an unknown sound was accepted"
  exit 1
fi

# The API answering is not the same as the loop driving: the label is "idle"
# until the first tick, and a core that loaded but never ticks would answer
# every route while standing still forever.
LABEL=""
for _ in $(seq 1 60); do
  LABEL=$(curl -sf --max-time 5 http://127.0.0.1:8765/state     | grep -oE '"label":"[^"]*"' | head -1 | cut -d'"' -f4 || true)
  [ -n "$LABEL" ] && [ "$LABEL" != "idle" ] && break
  sleep 1
done
if [ -z "$LABEL" ] || [ "$LABEL" = "idle" ]; then
  echo "::error::the control loop never drove the duck: label=$LABEL"
  exit 1
fi

# The duck's own odometry: an estimate from its legs and IMU, not the
# simulator's ground truth.
if ! echo "$STATE" | grep -q '"pose"'; then
  echo "::error::/state does not publish the duck's pose"
  exit 1
fi

# A drive mode is a whole-bundle switch: roller loads roller.onnx as the
# locomotion network and drops the standing network. The state says which
# bundle is loaded; the roller scene ships beside the walking one.
MODE=$(echo "$STATE" | grep -oE '"mode":"[^"]*"' || true)
if [ "$MODE" != '"mode":"walk"' ]; then
  echo "::error::the duck did not start in walk mode: $MODE"
  exit 1
fi
ROLLER=$(curl -sf --max-time 20 -X POST -H 'Content-Type: application/json'   -d '{"mode":"roller"}' http://127.0.0.1:8765/mode || true)
echo "$ROLLER" | tee intent-mode-roller.json > /dev/null
if ! echo "$ROLLER" | grep -q '"ok":true'; then
  echo "::error::the duck could not switch to roller: $ROLLER"
  exit 1
fi
sleep 3
if ! curl -sf --max-time 5 http://127.0.0.1:8765/state | grep -q '"mode":"roller"'; then
  echo "::error::the state does not report roller mode"
  exit 1
fi
curl -sf --max-time 20 -X POST -H 'Content-Type: application/json'   -d '{"mode":"walk"}' http://127.0.0.1:8765/mode > /dev/null || true
sleep 3

# Drive the duck through its own intent API, the way an external agent
# would: the API holds the duck for a lease, so keep the twist coming.
for _ in $(seq 1 8); do
  curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
    -d '{"vx":0.4,"vy":0.0,"wz":0.0}' http://127.0.0.1:8765/twist > /dev/null || true
  sleep 0.3
done
curl -sf --max-time 5 http://127.0.0.1:8765/state | tee intent-state-walking.json || true

# A black screen is the failure this app is prone to (the core load used to
# block the main thread). Catch it in CI instead of on the user's device.
# Two frames, taken while the duck walks: they must not be blank, and they
# must not be identical — a renderer that never started draws the same
# picture forever.
adb exec-out screencap -p > android-screen.png || true
adb exec-out screencap -p > android-screen-2.png || true
if [ -s android-screen.png ]; then
  # The scene region must be lit: SwiftUI/Compose chrome alone can light up
  # enough of the screen to pass a whole-frame check while the 3D view is
  # still black.
  python3 scripts/check-frame.py android-screen.png \
    --region 0.3,0.3,0.7,0.7 --region-lit 0.02     --expect-colour 255,140,50,110,150     --expect-shell 120,600 || exit 1
fi
if [ -s android-screen.png ] && [ -s android-screen-2.png ]; then
  python3 scripts/check-frame.py android-screen.png android-screen-2.png \
    --min-diff 0.0002 || exit 1
fi
echo "smoke test passed"
