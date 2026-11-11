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

for needle in '"proprioception"' '"positions"' '"imu_ready"' '"mouth"' '"performance"' '"leds"'; do
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
# The words-to-verbs table is the core's, so a Chinese sentence must run a
# skill even when no recognizer is present (CI has no microphone).
HEARD=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"text":"翻滚"}' http://127.0.0.1:8765/heard || true)
if ! echo "$HEARD" | grep -q '"skill":"roulade"'; then
  echo "::error::Chinese speech did not map to roulade: $HEARD"
  exit 1
fi
# The words are published for a user-supplied model, not only acted on.
if ! curl -sf --max-time 5 http://127.0.0.1:8765/heard | grep -q '翻滚'; then
  echo "::error::GET /heard does not publish the last words"
  exit 1
fi
# The gaze solver: aiming right must yaw the head right.
LOOK=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"x":1.0,"y":-0.5,"z":0.15}' http://127.0.0.1:8765/look || true)
if ! echo "$LOOK" | grep -q '"ok":true'; then
  echo "::error::look did not answer: $LOOK"
  exit 1
fi
# robotd's face LEDs, as state: status, gauge, seven-colour flashlight.
FLASH=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{"color":"cyan"}' http://127.0.0.1:8765/flashlight || true)
if ! echo "$FLASH" | grep -q '"color":"cyan"'; then
  echo "::error::flashlight did not set cyan: $FLASH"
  exit 1
fi
FLASH2=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json'   -d '{}' http://127.0.0.1:8765/flashlight || true)
if ! echo "$FLASH2" | grep -q '"flashlight":0'; then
  echo "::error::flashlight did not toggle off: $FLASH2"
  exit 1
fi
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

# The companion layer: hunger, mood and bond live in the core, age on its
# simulated clock, and are shown by both shells. A treat fills the tummy, a
# stroke warms the bond, neglect puts it to sleep, and a hand wakes it.
if ! curl -sf --max-time 5 http://127.0.0.1:8765/care | grep -q '"hunger"'; then
  echo "::error::/care does not publish the companion state"
  exit 1
fi
CARE_HUNGER_BEFORE=$(curl -sf --max-time 5 http://127.0.0.1:8765/care | python3 -c "
import json,sys
print((json.load(sys.stdin).get('care') or {}).get('hunger',1))
" 2>/dev/null || echo 1)
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:8765/feed > /dev/null || true
sleep 1
CARE_HUNGER_AFTER=$(curl -sf --max-time 5 http://127.0.0.1:8765/care | python3 -c "
import json,sys
print((json.load(sys.stdin).get('care') or {}).get('hunger',1))
" 2>/dev/null || echo 1)
python3 -c "
import sys
sys.exit(0 if float('${CARE_HUNGER_BEFORE}') > float('${CARE_HUNGER_AFTER}') else 1)
" || { echo "::error::a treat did not lower hunger: $CARE_HUNGER_BEFORE -> $CARE_HUNGER_AFTER"; exit 1; }
CARE_BOND_BEFORE=$(curl -sf --max-time 5 http://127.0.0.1:8765/care | python3 -c "
import json,sys
print((json.load(sys.stdin).get('care') or {}).get('bond',0))
" 2>/dev/null || echo 0)
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:8765/pet > /dev/null || true
sleep 0.4
CARE_BOND_AFTER=$(curl -sf --max-time 5 http://127.0.0.1:8765/care | python3 -c "
import json,sys
print((json.load(sys.stdin).get('care') or {}).get('bond',0))
" 2>/dev/null || echo 0)
python3 -c "
import sys
sys.exit(0 if float('${CARE_BOND_AFTER}') > float('${CARE_BOND_BEFORE}') else 1)
" || { echo "::error::a stroke did not warm the bond: $CARE_BOND_BEFORE -> $CARE_BOND_AFTER"; exit 1; }
# Twelve minutes of neglect, with the care clock at 200x: 8 simulated seconds
# since the last touch is 1600 care-seconds, so no test waits twelve minutes.
CARE_TICK=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | grep -oE '"tick":[0-9]+' | head -1 | cut -d: -f2)
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"care_time_scale":200}' http://127.0.0.1:8765/config > /dev/null || true
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"hunger\":0.9,\"mood\":0.2,\"bond\":0.4,\"asleep\":false,\"last_touch\":$((CARE_TICK - 400))}" \
  http://127.0.0.1:8765/care > /dev/null || true
sleep 0.5
if ! curl -sf --max-time 5 http://127.0.0.1:8765/care | grep -q '"asleep":true'; then
  echo "::error::a long neglect did not put the duck to sleep"
  exit 1
fi
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:8765/pet > /dev/null || true
sleep 0.4
if ! curl -sf --max-time 5 http://127.0.0.1:8765/care | grep -q '"asleep":false'; then
  echo "::error::a stroke did not wake the duck"
  exit 1
fi
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"care_time_scale":1}' http://127.0.0.1:8765/config > /dev/null || true
echo "care: hunger $CARE_HUNGER_BEFORE -> $CARE_HUNGER_AFTER, bond $CARE_BOND_BEFORE -> $CARE_BOND_AFTER, doze and wake ok"
# The microphone channel, classified: clap and whistle reach the duck's own
# reaction and the published event. The RMS is the room, never a sense.
sound_tick() { python3 -c 'import time; print(int(time.time()*1000))'; }
ST=$(sound_tick)
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.02,\"peak_hz\":300},\"tick_ms\":$ST}" \
  http://127.0.0.1:8765/sensor > /dev/null || true
sleep 0.15
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.7,\"peak_hz\":2800},\"tick_ms\":$((ST + 60))}" \
  http://127.0.0.1:8765/sensor > /dev/null || true
sleep 0.5
if ! curl -sf --max-time 5 http://127.0.0.1:8765/state | grep -q '"kind":"clap"'; then
  echo "::error::a clap was not classified as a clap"
  exit 1
fi
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.0,\"peak_hz\":0},\"tick_ms\":$((ST + 250))}" \
  http://127.0.0.1:8765/sensor > /dev/null || true
sleep 0.2
WT=$(sound_tick)
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.4,\"peak_hz\":900},\"tick_ms\":$WT}" \
  http://127.0.0.1:8765/sensor > /dev/null || true
sleep 0.45
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.4,\"peak_hz\":950},\"tick_ms\":$((WT + 500))}" \
  http://127.0.0.1:8765/sensor > /dev/null || true
sleep 0.5
if ! curl -sf --max-time 5 http://127.0.0.1:8765/state | grep -q '"kind":"whistle"'; then
  echo "::error::a sustained tone was not classified as a whistle"
  exit 1
fi
echo "room sounds: clap and whistle ok"

# A child's block rule, end to end: program "when clap, roll over", make the
# clap, and the duck's real roulade must run and be counted.
echo "--- block rules ---"
if ! curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"rules":[{"trigger":"clap","action":"skill","value":"roulade"}]}' \
  http://127.0.0.1:8765/rules | grep -q '"ok":true'; then
  echo "::error::the core refused a valid block rule"
  exit 1
fi
if ! curl -sf --max-time 5 http://127.0.0.1:8765/rules | grep -q '"trigger":"clap"'; then
  echo "::error::the block rule was not read back"
  exit 1
fi
sleep 4
rolls_of() {
  curl -sf --max-time 5 http://127.0.0.1:8765/life | python3 -c "
import json,sys
print(json.load(sys.stdin).get('skills',{}).get('roulade',0))
" 2>/dev/null || echo 0
}
ROLLS_BEFORE=$(rolls_of)
ROLLS_AFTER=$ROLLS_BEFORE
for attempt in 1 2 3 4; do
  RT=$(sound_tick)
  curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
    -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.02,\"peak_hz\":300},\"tick_ms\":$RT}" \
    http://127.0.0.1:8765/sensor > /dev/null || true
  sleep 0.15
  curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
    -d "{\"sensor\":\"microphone\",\"mode\":\"twin\",\"value\":{\"rms\":0.7,\"peak_hz\":2800},\"tick_ms\":$((RT + 60))}" \
    http://127.0.0.1:8765/sensor > /dev/null || true
  for _ in $(seq 1 20); do
    ROLLS_AFTER=$(rolls_of)
    [ "$ROLLS_AFTER" -gt "$ROLLS_BEFORE" ] && break
    sleep 0.25
  done
  [ "$ROLLS_AFTER" -gt "$ROLLS_BEFORE" ] && break
  sleep 1.5
done
if [ "$ROLLS_AFTER" -le "$ROLLS_BEFORE" ]; then
  echo "::error::a clap did not run the programmed roulade: $ROLLS_BEFORE -> $ROLLS_AFTER"
  exit 1
fi
if ! curl -sf --max-time 5 http://127.0.0.1:8765/state | grep -q '"value":"roulade"'; then
  echo "::error::/state does not publish the block that fired"
  exit 1
fi
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' -d '{"rules":[]}' \
  http://127.0.0.1:8765/rules > /dev/null || true
echo "block rules: clap -> roulade ok ($ROLLS_BEFORE -> $ROLLS_AFTER)"

# The LAN social layer starts on demand and publishes its own identity and
# an (empty in CI) peer list. Two emulators are not available here, so the
# protocol itself is exercised by the iOS host-peer test and the unit tests.
echo "--- neighbours ---"
if ! curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"name":"CI Android"}' http://127.0.0.1:8765/peers/start | grep -q '"ok":true'; then
  echo "::error::the duck could not open its LAN peer socket"
  exit 1
fi
PEERS=$(curl -sf --max-time 5 http://127.0.0.1:8765/peers || true)
echo "$PEERS" | grep -q '"name":"CI Android"'
if ! echo "$PEERS" | grep -q '"peers"'; then
  echo "::error::/peers does not publish the peer list"
  exit 1
fi
echo "neighbours: LAN socket and peer list ok"
# Leave a calm, awake duck for the walk and screenshots.
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"hunger":0.15,"mood":0.35,"bond":0.2,"asleep":false}' \
  http://127.0.0.1:8765/care > /dev/null || true

# The lifetime record as a shareable document: the core writes it, the sheet
# saves it to cache and hands it to the chooser.
if ! curl -sf --max-time 5 http://127.0.0.1:8765/export/vetbook.md | grep -q "walked"; then
  echo "::error::the vet book Markdown export is missing or empty"
  exit 1
fi
if ! curl -sf --max-time 5 http://127.0.0.1:8765/export/vetbook.json | grep -q '"format":"microduck-vetbook/1"'; then
  echo "::error::the vet book JSON export has no format tag"
  exit 1
fi
echo "vet book export: markdown and json ok"

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
# A block rule may have just run a real skill; a mode switch requires the
# duck to be standing still, so let the scheduled move finish first.
sleep 3
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

# The companion state must survive the process being killed by Android. Set
# a distinctive value, background the app so onPause writes the prefs, kill
# it, relaunch, and read back what the core accepted. A default-valued duck
# (hunger 0.15, bond 0.1) means the hand-back never ran.
echo "--- companion survives a restart ---"
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"hunger":0.42,"mood":0.6,"bond":0.33,"asleep":false}' \
  http://127.0.0.1:8765/care > /dev/null || true
# The shell saves every three seconds; let one cadence see the new numbers
# before the process is killed.
sleep 5
adb shell input keyevent KEYCODE_HOME > /dev/null 2>&1 || true
sleep 3
adb shell am force-stop ai.duck.microduck > /dev/null 2>&1 || true
sleep 1
adb shell am start -n ai.duck.microduck/.MainActivity > /dev/null 2>&1 || true
RESTORED_CARE=""
for _ in $(seq 1 180); do
  RESTORED_CARE=$(curl -sf --max-time 5 http://127.0.0.1:8765/care 2>/dev/null | python3 -c "
import json,sys
c=json.load(sys.stdin).get('care') or {}
h=c.get('hunger'); b=c.get('bond')
print(f'{h:.3f} {b:.3f}' if h is not None and b is not None else '')
" 2>/dev/null || true)
  if [ -n "$RESTORED_CARE" ]; then
    python3 -c "
import sys
h,b=map(float,'$RESTORED_CARE'.split())
sys.exit(0 if 0.38 <= h <= 0.50 and b >= 0.28 else 1)
" 2>/dev/null && break
  fi
  sleep 1
done
python3 -c "
import sys
h,b=map(float,'${RESTORED_CARE:-0 0}'.split())
sys.exit(0 if 0.38 <= h <= 0.50 and b >= 0.28 else 1)
" || { echo "::error::the companion state did not survive a restart: $RESTORED_CARE"; exit 1; }
echo "care survived restart: hunger/bond=$RESTORED_CARE"

# Director's shutter, end to end: enter Director, tap the shutter, and the
# photo preview must appear. The emulator has no camera, so the frame is the
# duck over the studio — the same path a denied-camera device takes. The
# preview, Save and Share are Compose controls; nothing here is a fake button.
echo "--- Director photo ---"
adb shell pm grant ai.duck.microduck android.permission.CAMERA > /dev/null 2>&1 || true
tap_text() {
  adb shell uiautomator dump /sdcard/ui.xml > /dev/null 2>&1 || return 1
  adb exec-out cat /sdcard/ui.xml > ui.xml 2>/dev/null || return 1
  coords=$(python3 - "$1" <<'PY'
import sys, xml.etree.ElementTree as ET
want = sys.argv[1]
try:
    root = ET.parse('ui.xml').getroot()
except Exception:
    raise SystemExit(1)
for node in root.iter('node'):
    if node.get('text') == want:
        bounds = node.get('bounds', '')
        parts = bounds.replace('][', ',').strip('[]').split(',')
        if len(parts) == 4:
            x = (int(parts[0]) + int(parts[2])) // 2
            y = (int(parts[1]) + int(parts[3])) // 2
            print(x, y)
            raise SystemExit(0)
raise SystemExit(1)
PY
)
  [ -n "$coords" ] || return 1
  adb shell input tap $coords > /dev/null 2>&1
}
tap_text "Director" || tap_text "取景" || { echo "::error::no Director switch in the shell"; exit 1; }
SHUTTER_OK=0
for _ in $(seq 1 10); do
  if tap_text "Take photo" || tap_text "拍照"; then SHUTTER_OK=1; break; fi
  sleep 1
done
[ "$SHUTTER_OK" = "1" ] || { echo "::error::no shutter in Director"; exit 1; }
PHOTO_OK=0
for _ in $(seq 1 30); do
  adb shell uiautomator dump /sdcard/ui.xml > /dev/null 2>&1 || true
  adb exec-out cat /sdcard/ui.xml > ui.xml 2>/dev/null || true
  if python3 - <<'PY'
import sys, xml.etree.ElementTree as ET
try:
    root = ET.parse('ui.xml').getroot()
except Exception:
    raise SystemExit(1)
texts = [n.get('text') or '' for n in root.iter('node')]
titles = {'Your duck, in your room', '你的鸭子，在你房间'}
save = {'Save', '保存'}
share = {'Share', '分享'}
raise SystemExit(0 if titles.intersection(texts) and save.intersection(texts) and share.intersection(texts) else 1)
PY
  then
    PHOTO_OK=1
    break
  fi
  sleep 1
done
[ "$PHOTO_OK" = "1" ] || { echo "::error::the Director photo never reached its preview"; exit 1; }
echo "Director photo reached the preview with Save and Share"
echo "smoke test passed"
