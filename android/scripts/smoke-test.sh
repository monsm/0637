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
# Pin the room's hour: the frame checks below assert on colours and must not
# change with the CI runner's wall clock (the stage tints by local time).
adb shell am start -n ai.duck.microduck/.MainActivity --ef daylight_hour 12 | tee am-start.log

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
# 站起来 on a standing duck must be a no-op, not a toggle into the seat.
if ! curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"text":"站起来"}' http://127.0.0.1:8765/heard | grep -q '"did":false'; then
  echo "::error::站起来 seated a standing duck"
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
CARE_DOZE_DEBUG=$(curl -sf --max-time 5 http://127.0.0.1:8765/care || true)
echo "care doze state: $CARE_DOZE_DEBUG"
if ! echo "$CARE_DOZE_DEBUG" | grep -q '"asleep":true'; then
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
# The chosen night: good night puts the duck to sleep on purpose, and the
# state says so. A hand is a real hand: a pet ends it, and the vet book
# names that ending rather than "morning".
echo "--- night ---"
if ! curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"on":true}' http://127.0.0.1:8765/night | grep -q '"ok":true'; then
  echo "::error::the duck refused to go to sleep on purpose"
  exit 1
fi
NIGHT=""
for _ in $(seq 1 20); do
  NIGHT=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try:
    d=json.load(sys.stdin)
    print(1 if (d.get('night') or {}).get('active') and (d.get('care') or {}).get('asleep') else 0)
except Exception: print(0)
" 2>/dev/null || echo 0)
  [ "$NIGHT" = "1" ] && break
  sleep 0.2
done
if [ "$NIGHT" != "1" ]; then
  echo "::error::/night did not put the duck to sleep"
  exit 1
fi
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' -d '{}' \
  http://127.0.0.1:8765/pet > /dev/null || true
NIGHT_AWAKE=""
for _ in $(seq 1 20); do
  NIGHT_AWAKE=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try: print(1 if not (json.load(sys.stdin).get('night') or {}).get('active') else 0)
except Exception: print(0)
" 2>/dev/null || echo 0)
  [ "$NIGHT_AWAKE" = "1" ] && curl -sf --max-time 5 http://127.0.0.1:8765/life | grep -q 'woken_by_touch' && break
  sleep 0.2
done
if [ "$NIGHT_AWAKE" != "1" ] || ! curl -sf --max-time 5 http://127.0.0.1:8765/life | grep -q 'woken_by_touch'; then
  echo "::error::a stroke did not end the night with woken_by_touch"
  exit 1
fi
# The morning verb on an awake duck is a no-op that still answers: the
# good-morning chip must never leave the row stuck in night.
if ! curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"on":false}' http://127.0.0.1:8765/night | grep -q '"ok":true'; then
  echo "::error::the morning verb refused an awake duck"
  exit 1
fi
NIGHT_OFF=""
for _ in $(seq 1 20); do
  NIGHT_OFF=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try: print(1 if not (json.load(sys.stdin).get('night') or {}).get('active') else 0)
except Exception: print(0)
" 2>/dev/null || echo 0)
  [ "$NIGHT_OFF" = "1" ] && break
  sleep 0.2
done
if [ "$NIGHT_OFF" != "1" ]; then
  echo "::error::the night is still active after the morning verb"
  exit 1
fi
echo "night: on -> asleep and quiet, pet -> woken_by_touch, off ok"
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
# peer list. Two emulators are not available here, so the host plays the
# second duck: its datagrams reach the app through the emulator's UDP
# redirect, and the app's own unicast replies and gifts come back out the
# same way. adb forwards only TCP, so the console adds the UDP side.
echo "--- neighbours ---"
if ! curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"name":"CI Android"}' http://127.0.0.1:8765/peers/start | grep -q '"ok":true'; then
  echo "::error::the duck could not open its LAN peer socket"
  exit 1
fi
for _ in $(seq 1 30); do
  PEERS=$(curl -sf --max-time 5 http://127.0.0.1:8765/peers || true)
  echo "$PEERS" | grep -q '"name":"CI Android"' && break
  sleep 0.2
done
if ! echo "$PEERS" | grep -q '"name":"CI Android"'; then
  echo "::error::the LAN socket never announced the duck"
  exit 1
fi
if ! echo "$PEERS" | grep -q '"peers"'; then
  echo "::error::/peers does not publish the peer list"
  exit 1
fi
adb emu redir add udp:8766:8766 > /dev/null 2>&1 || true

# Discovery and a greeting, the same protocol two phones use. The duck must
# discover Sunny, answer the greet with a hello, and keep the peer.
PEER_REPLY=$(python3 - <<'PY'
import json, socket, time
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(("127.0.0.1", 8799))
sock.settimeout(3.0)
announce = {"v": 1, "type": "announce", "from": "peerci01", "name": "Sunny"}
greet = {"v": 1, "type": "greet", "from": "peerci01", "name": "Sunny", "to": "all"}
sock.sendto(json.dumps(announce).encode(), ("127.0.0.1", 8766))
time.sleep(0.15)
sock.sendto(json.dumps(greet).encode(), ("127.0.0.1", 8766))
try:
    data, _ = sock.recvfrom(2048)
    message = json.loads(data)
    print(message.get("type", ""), message.get("to", ""))
except Exception:
    print("timeout", "")
PY
)
if [ "${PEER_REPLY%% *}" != "hello" ]; then
  echo "::error::the duck did not answer another duck's greet: reply=$PEER_REPLY"
  exit 1
fi
echo "neighbours: the host peer is greeted with hello"
PEER_FOUND=""
for _ in $(seq 1 20); do
  PEER_FOUND=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try:
    peers=(json.load(sys.stdin).get('peers') or {}).get('peers') or []
    print(1 if any(p.get('name') == 'Sunny' for p in peers) else 0)
except Exception: print(0)
" 2>/dev/null || echo 0)
  [ "$PEER_FOUND" = "1" ] && break
  sleep 0.2
done
if [ "$PEER_FOUND" != "1" ]; then
  echo "::error::the host peer never reached /state.peers"
  exit 1
fi
APP_PEER_ID=$(curl -sf --max-time 5 http://127.0.0.1:8765/peers | python3 -c "
import json,sys
try: print(json.load(sys.stdin).get('id',''))
except Exception: print('')
" 2>/dev/null || echo "")
if [ -z "$APP_PEER_ID" ]; then
  echo "::error::/peers does not publish the duck's own id"
  exit 1
fi

# A gift through the real receiver path: the official laugh timeline runs
# and the vet book keeps the gift. The announce keeps the peer inside its
# 8 s TTL while the packet lands.
python3 - "$APP_PEER_ID" <<'PY'
import json, socket, sys, time
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
announce = {"v": 1, "type": "announce", "from": "peerci01", "name": "Sunny"}
gift = {"v": 1, "type": "gift", "from": "peerci01", "name": "Sunny",
        "to": sys.argv[1], "gift": "emotion", "value": "laugh"}
sock.sendto(json.dumps(announce).encode(), ("127.0.0.1", 8766))
time.sleep(0.15)
sock.sendto(json.dumps(gift).encode(), ("127.0.0.1", 8766))
PY
GIFT=""
for _ in $(seq 1 20); do
  GIFT=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try:
    e=json.load(sys.stdin).get('gift_event') or {}
    print(e.get('kind',''), e.get('from_name',''), e.get('value',''))
except Exception: print('')
" 2>/dev/null || echo "")
  [ "$GIFT" = "emotion Sunny laugh" ] && break
  sleep 0.2
done
if [ "$GIFT" != "emotion Sunny laugh" ]; then
  echo "::error::a neighbour's gift never reached the duck: gift_event=$GIFT"
  exit 1
fi
if ! curl -sf --max-time 5 http://127.0.0.1:8765/life | grep -q 'gift:emotion:laugh'; then
  echo "::error::the vet book did not record the neighbour's gift"
  exit 1
fi
echo "neighbours: gift emotion laugh received and recorded"

# The app's own sender path: one treat to the known peer is queued, not
# refused, and the packet really leaves the app. The listener IS the socket
# that announces: the app stores a peer's address from the announce's
# source, so a reply can only come back to the exact port that spoke.
python3 - <<'PY' > /tmp/duck-gift-ack.txt &
import json, socket, time
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(("127.0.0.1", 8799))
sock.settimeout(8.0)
announce = {"v": 1, "type": "announce", "from": "peerci01", "name": "Sunny"}
sock.sendto(json.dumps(announce).encode(), ("127.0.0.1", 8766))
try:
    data, _ = sock.recvfrom(2048)
    message = json.loads(data)
    print(message.get("type", ""), message.get("gift", ""))
except Exception:
    print("timeout", "")
PY
ACK_PID=$!
sleep 0.5
GIFT_SENT=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"id":"peerci01","gift":"treat"}' http://127.0.0.1:8765/peers/gift || true)
if ! echo "$GIFT_SENT" | grep -q '"queued":true'; then
  echo "::error::the duck refused to send a neighbour a treat: $GIFT_SENT"
  exit 1
fi
wait "$ACK_PID" 2>/dev/null || true
GIFT_ACK=$(cat /tmp/duck-gift-ack.txt 2>/dev/null || echo "")
rm -f /tmp/duck-gift-ack.txt
if [ "$GIFT_ACK" != "gift treat" ]; then
  echo "::error::the queued treat never left the app as a gift packet: ack=$GIFT_ACK"
  exit 1
fi
echo "neighbours: treat sent to the host peer and the packet left the app"
echo "neighbours: LAN socket, peer list and gifts ok"

# The room's chorus: a host packet from the known peer makes the duck join,
# perform the official laugh and keep the moment in its lifetime record.
python3 - <<'PY' > /tmp/duck-chorus.txt
import json, socket, time
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(("127.0.0.1", 8799))
announce = {"v": 1, "type": "announce", "from": "peerci01", "name": "Sunny"}
song = {"v": 1, "type": "chorus", "from": "peerci01", "name": "Sunny",
        "to": "all", "id": "ci-chorus-1", "moves": ["laugh"]}
sock.sendto(json.dumps(announce).encode(), ("127.0.0.1", 8766))
time.sleep(0.15)
sock.sendto(json.dumps(song).encode(), ("127.0.0.1", 8766))
PY
CHORUS=""
for _ in $(seq 1 25); do
  CHORUS=$(curl -sf --max-time 5 http://127.0.0.1:8765/state | python3 -c "
import json,sys
try:
    c=json.load(sys.stdin).get('chorus') or {}
    print('yes' if c.get('active') and c.get('role') == 'join' else 'no')
except Exception: print('no')
" 2>/dev/null || echo "no")
  [ "$CHORUS" = "yes" ] && break
  sleep 0.2
done
if [ "$CHORUS" != "yes" ]; then
  echo "::error::the duck never joined the room's chorus: $CHORUS"
  exit 1
fi
CHORUS_NOTE=""
for _ in $(seq 1 40); do
  CHORUS_NOTE=$(curl -sf --max-time 5 http://127.0.0.1:8765/life | python3 -c "
import json,sys
try: print('yes' if 'chorus' in (json.load(sys.stdin).get('events') or []) else 'no')
except Exception: print('no')
" 2>/dev/null || echo "no")
  [ "$CHORUS_NOTE" = "yes" ] && break
  sleep 0.2
done
if [ "$CHORUS_NOTE" != "yes" ]; then
  echo "::error::the vet book never named the chorus"
  exit 1
fi
# The lead's own route: a move this build does not have is refused, and a
# real one is queued. Both are answered before any broadcast.
BAD_CHORUS=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"moves":["moonwalk"]}' http://127.0.0.1:8765/chorus || true)
if ! echo "$BAD_CHORUS" | grep -q '"ok":false'; then
  echo "::error::an unknown chorus move was accepted: $BAD_CHORUS"
  exit 1
fi
GOOD_CHORUS=$(curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d '{"moves":["laugh"]}' http://127.0.0.1:8765/chorus || true)
if ! echo "$GOOD_CHORUS" | grep -q '"queued":true'; then
  echo "::error::the duck refused to lead a chorus: $GOOD_CHORUS"
  exit 1
fi
echo "neighbours: chorus joined, performed and led ok"
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
# Daily quests: calendar day in, three measurable goals out.
echo "--- daily quests ---"
QUEST_DAY=$(python3 -c 'import datetime; print(datetime.date.today().isoformat())')
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"day\":\"$QUEST_DAY\",\"board\":{}}" http://127.0.0.1:8765/quests/day > /dev/null || true
QUESTS=$(curl -sf --max-time 5 http://127.0.0.1:8765/quests || true)
if ! echo "$QUESTS" | python3 -c "
import json,sys
d=json.load(sys.stdin)
q=d.get('quests',[])
sys.exit(0 if d.get('day') == '$QUEST_DAY' and len(q)==3 and all('title_en' in x and 'progress' in x for x in q) else 1)
" 2>/dev/null; then
  echo "::error::the daily quest board was not created: $QUESTS"
  exit 1
fi
echo "daily quests: 3 goals for $QUEST_DAY"
# A real pet feeds the board if today's quest measures pets; the generic
# assertion is that the board stays valid and its progress never goes back
# while the life counters only grow.
QUEST_SUM_BEFORE=$(echo "$QUESTS" | python3 -c "
import json,sys
print(sum(x.get('progress',0) for x in json.load(sys.stdin).get('quests',[])))
" 2>/dev/null || echo 0)
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' -d '{}' \
  http://127.0.0.1:8765/pet > /dev/null || true
sleep 0.4
QUEST_SUM_AFTER=$(curl -sf --max-time 5 http://127.0.0.1:8765/quests | python3 -c "
import json,sys
print(sum(x.get('progress',0) for x in json.load(sys.stdin).get('quests',[])))
" 2>/dev/null || echo 0)
python3 -c "
import sys
sys.exit(0 if float('${QUEST_SUM_AFTER:-0}') >= float('${QUEST_SUM_BEFORE:-0}') else 1)
" 2>/dev/null || { echo "::error::the quest board went backwards after a pet"; exit 1; }
echo "daily quests: progress stays honest after an action"
# The calendar is real: a board completed on February 28th keeps its streak
# on March 1st (February 30th does not exist), and a summary-shaped board —
# what the shell actually saves — restores with its progress intact.
MONTH_BOARD='{"day":"2026-02-28","quests":[{"id":"pet","target":3,"done":true,"progress":1.0},{"id":"roll","target":2,"done":true,"progress":1.0},{"id":"walk","target":2,"done":true,"progress":1.0}],"streak":3,"best_streak":4,"completed_today":3,"last_completed_day":"2026-02-28"}'
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"day\":\"2026-03-01\",\"board\":$MONTH_BOARD}" http://127.0.0.1:8765/quests/day > /dev/null || true
sleep 0.5
MONTH_STREAK=$(curl -sf --max-time 5 http://127.0.0.1:8765/quests | python3 -c "
import json,sys
try: print(json.load(sys.stdin).get('streak'))
except Exception: print('0')
" 2>/dev/null || echo 0)
if [ "$MONTH_STREAK" != "3" ]; then
  echo "::error::a February 28th board lost its streak on March 1st: streak=$MONTH_STREAK"
  exit 1
fi
echo "daily quests: a February streak survives March 1st (streak=$MONTH_STREAK)"
# The same-day relaunch: the board's own day is today, so the core keeps
# it instead of rolling a fresh board, and the shell's summary shape must
# come back with the progress the child already earned.
YDAY=$(python3 -c 'import datetime; print((datetime.date.today()-datetime.timedelta(days=1)).isoformat())')
SAVE_BOARD="{\"day\":\"$QUEST_DAY\",\"quests\":[{\"id\":\"pet\",\"target\":3,\"done\":false,\"progress\":0.5},{\"id\":\"roll\",\"target\":2,\"done\":false,\"progress\":0.0},{\"id\":\"walk\",\"target\":2,\"done\":false,\"progress\":0.0}],\"streak\":1,\"best_streak\":1,\"completed_today\":0,\"last_completed_day\":\"$YDAY\"}"
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"day\":\"$QUEST_DAY\",\"board\":$SAVE_BOARD}" http://127.0.0.1:8765/quests/day > /dev/null || true
sleep 0.4
PET_PROGRESS=$(curl -sf --max-time 5 http://127.0.0.1:8765/quests | python3 -c "
import json,sys
try:
    q=[x for x in json.load(sys.stdin).get('quests',[]) if x.get('id')=='pet']
    print(round(q[0].get('progress',0),2) if q else 0)
except Exception: print('0')
" 2>/dev/null || echo 0)
python3 -c "
import sys
sys.exit(0 if 0.3 <= float('${PET_PROGRESS:-0}') <= 1.0 else 1)
" 2>/dev/null || { echo "::error::a summary-shaped saved board lost its progress on restore (pet=$PET_PROGRESS)"; exit 1; }
echo "daily quests: the shell's saved board restores with its progress (pet=$PET_PROGRESS)"
# Hand the day back so the rest of the session measures the real calendar.
curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
  -d "{\"day\":\"$QUEST_DAY\",\"board\":{}}" http://127.0.0.1:8765/quests/day > /dev/null || true

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
# The vet book counts the distance the duck's own odometry estimates.
WALKED=$(curl -sf --max-time 5 http://127.0.0.1:8765/life | python3 -c "
import json,sys
print(json.load(sys.stdin).get('metres', 0))
" 2>/dev/null || echo 0)
python3 -c "
import sys
sys.exit(0 if float('${WALKED:-0}') > 0.01 else 1)
" 2>/dev/null || { echo "::error::the vet book never counted the walk: metres=$WALKED"; exit 1; }
echo "walk counted: $WALKED m"

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
# The shell saves every three seconds; let a full cadence (plus its state
# fetch) see the new numbers before the process is killed.
sleep 8
CARE_NOW=$(curl -sf --max-time 5 http://127.0.0.1:8765/care | python3 -c "
import json,sys
c=json.load(sys.stdin).get('care') or {}
print(c.get('hunger'), c.get('bond'), c.get('asleep'))
" 2>/dev/null || echo "?")
echo "care before restart: $CARE_NOW"
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
" || { echo "::error::the companion state did not survive a restart: $RESTORED_CARE"; adb logcat -d | grep -E "duck-care" | tail -40; exit 1; }
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
tap_text "Director" || tap_text "取景" || {
  echo "::error::no Director switch in the shell"
  grep -o 'text="[^"]*"' ui.xml 2>/dev/null | sort -u | head -60 || true
  exit 1
}
SHUTTER_OK=0
for _ in $(seq 1 10); do
  if tap_text "Photo" || tap_text "拍照" || tap_text "Take photo"; then SHUTTER_OK=1; break; fi
  sleep 1
done
[ "$SHUTTER_OK" = "1" ] || {
  echo "::error::no shutter in Director"
  grep -o 'text="[^"]*"' ui.xml 2>/dev/null | sort -u | head -60 || true
  exit 1
}
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
[ "$PHOTO_OK" = "1" ] || {
  echo "::error::the Director photo never reached its preview"
  grep -o 'text="[^"]*"' ui.xml 2>/dev/null | sort -u | head -80 || true
  adb logcat -d | grep -E "duck-camera|AndroidRuntime|photo" | tail -30 || true
  exit 1
}
echo "Director photo reached the preview with Save and Share"

# ------------------------------------------------------------------ B1 screen
# The first screen is a promise, not a debug console. Assert it in the real
# UI, in Chinese, in portrait: no wire skill name (`_`), no raw tick
# (`t 302`), no percentage pill, and no engine/port text. Then take the
# acceptance screenshot from that exact screen.
echo "--- first screen speaks human ---"
adb shell am force-stop ai.duck.microduck > /dev/null 2>&1 || true
sleep 1
adb shell am start -n ai.duck.microduck/.MainActivity --ef daylight_hour 12 > /dev/null 2>&1 || true
for _ in $(seq 1 90); do
  SCREEN_STATE=$(curl -sf --max-time 5 http://127.0.0.1:8765/state || true)
  [ -n "$SCREEN_STATE" ] && break
  sleep 2
done
if [ -z "${SCREEN_STATE:-}" ]; then
  echo "::error::the core did not come back for the first-screen check"
  exit 1
fi
# Let the first frame settle, then switch the shell to Chinese through its
# own menu: the acceptance screen is the one a Chinese user sees.
sleep 3
tap_text "⋯" > /dev/null 2>&1 || true
sleep 1
# The menu shows the language it is IN. "EN" switches to Chinese; if it says
# "中文" the shell is already Chinese, and tapping the anchor again dismisses
# the menu without changing the language.
if ! tap_text "EN" > /dev/null 2>&1; then
  tap_text "⋯" > /dev/null 2>&1 || true
fi
sleep 2

# A fresh dump or a clear failure: a swallowed dump that falls back to a
# previous run's ui.xml would assert on stale text and "pass" by accident.
rm -f ui.xml
if ! adb shell uiautomator dump /sdcard/ui.xml > /dev/null 2>&1; then
  echo "::error::uiautomator could not dump the first screen"
  exit 1
fi
adb exec-out cat /sdcard/ui.xml > ui.xml 2>/dev/null || true
[ -s ui.xml ] || { echo "::error::the first-screen UI dump is empty"; exit 1; }
python3 - <<'PY' || exit 1
import re
import sys
import xml.etree.ElementTree as ET

try:
    root = ET.parse('ui.xml').getroot()
except Exception as exc:
    raise SystemExit(f"::error::cannot read the UI dump: {exc}")
texts = []
for node in root.iter('node'):
    if node.get('package') != 'ai.duck.microduck':
        continue
    for attr in ('text', 'content-desc'):
        value = node.get(attr) or ''
        if value:
            texts.append(value)

problems = []
problems += [f"raw token on screen: {t!r}" for t in texts if '_' in t]
problems += [f"raw tick on screen: {t!r}" for t in texts if re.fullmatch(r't \d+', t)]
problems += [f"percentage on the first screen: {t!r}" for t in texts if '%' in t]
problems += [f"engine/port text on the first screen: {t!r}" for t in texts
             if re.search(r'MuJoCo|ONNX|8765', t, re.IGNORECASE)]
if problems:
    for problem in problems:
        print(f"::error::{problem}")
    sys.exit(1)

joined = ' '.join(texts)
if not any(word in joined for word in ('技能', '时间', '心情不错', '有点饿了', '待着', '散步中', '站着')):
    raise SystemExit(f"::error::the shell did not switch to Chinese: {texts[:20]}")
print("first screen: no raw tokens, ticks, percentages or engine names")
PY

# The idle Live screen itself, portrait, no skill line on it.
adb exec-out screencap -p > ui-b1-android.png || true
python3 - <<'PY' || exit 1
import struct
try:
    with open('ui-b1-android.png', 'rb') as handle:
        header = handle.read(24)
except Exception as exc:
    raise SystemExit(f"::error::no first-screen screenshot: {exc}")
if header[:8] != b'\x89PNG\r\n\x1a\n':
    raise SystemExit("::error::the first-screen screenshot is not a PNG")
width, height = struct.unpack('>II', header[16:24])
if height <= width:
    raise SystemExit(f"::error::the acceptance screenshot is not portrait: {width}x{height}")
print(f"portrait screenshot: {width}x{height}")
PY

# A real wire skill with an underscore: its verdict in the top bar must read
# human too. This runs after the screenshot so the idle frame stays clean.
tap_text "技能" > /dev/null 2>&1 || tap_text "Skills" > /dev/null 2>&1 || true
sleep 1
tap_text "左踢" > /dev/null 2>&1 || tap_text "kick left" > /dev/null 2>&1 || true
sleep 2
rm -f ui.xml
if ! adb shell uiautomator dump /sdcard/ui.xml > /dev/null 2>&1; then
  echo "::error::uiautomator could not dump the screen after the skill"
  exit 1
fi
adb exec-out cat /sdcard/ui.xml > ui.xml 2>/dev/null || true
[ -s ui.xml ] || { echo "::error::the post-skill UI dump is empty"; exit 1; }
python3 - <<'PY' || exit 1
import sys
import xml.etree.ElementTree as ET

try:
    root = ET.parse('ui.xml').getroot()
except Exception as exc:
    raise SystemExit(f"::error::cannot read the post-skill UI dump: {exc}")
texts = [
    node.get(attr) or ''
    for node in root.iter('node')
    if node.get('package') == 'ai.duck.microduck'
    for attr in ('text', 'content-desc')
]
raw = [t for t in texts if '_' in t]
if raw:
    raise SystemExit(f"::error::a raw skill name reached the top bar: {raw[:5]}")
if not any(word in ' '.join(texts) for word in ('左踢', 'kick left')):
    raise SystemExit(f"::error::the kick verdict did not reach the top bar: {texts[:20]}")
print("skill verdict is human: no underscores")
PY

echo "smoke test passed"
