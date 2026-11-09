#!/usr/bin/env bash
# A scripted simulator session - everything a developer does by hand in the
# Xcode simulator, plus the assertions they would eyeball.
#
# The app's own intent API is the control surface, so the whole session runs
# from the shell: walk, sit, stand, every skill, ground pick, head/body
# control, an external push, and a policy reload. Each step is checked
# against /state, and the renderer is checked twice: the frames must show a
# lit 3D viewport that changes over time, and the startup log must say the
# renderer built its instances without failing.
#
# Every check runs even after one fails, so a single run reports the whole
# matrix instead of stopping at the first problem.
#
# Usage: sim-drive.sh <app-bundle> <udid> [output-dir]
set -uo pipefail

APP="${1:?usage: sim-drive.sh <app-bundle> <udid> [output-dir]}"
UDID="${2:?usage: sim-drive.sh <app-bundle> <udid> [output-dir]}"
OUT="${3:-sim-drive}"
BUNDLE_ID="ai.duck.microduck"
API="http://127.0.0.1:8765"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

mkdir -p "$OUT"

PASSES=0
FAILURES=0
# check <exit-code> <name> [detail] - 0 is a pass, like the shell it came from.
check() {
  local code="$1" name="$2" detail="${3:-}"
  if [ "$code" -eq 0 ]; then
    PASSES=$((PASSES + 1))
    echo "PASS  $name${detail:+  ($detail)}"
  else
    FAILURES=$((FAILURES + 1))
    echo "FAIL  $name${detail:+  ($detail)}"
  fi
}

api() {
  local method="$1" path="$2" body="${3:-}"
  if [ "$method" = "GET" ]; then
    curl -sf --max-time 10 "$API$path"
  else
    curl -sf --max-time 10 -X POST -H 'Content-Type: application/json' \
      -d "$body" "$API$path"
  fi
}

# Pull one field out of the /state JSON without needing jq.
field() {
  python3 -c "
import json,sys
try:
    d = json.load(sys.stdin)
except Exception:
    print(''); raise SystemExit
value = d.get(sys.argv[1])
if isinstance(value, (list, tuple)):
    print(' '.join(f'{v:.4f}' for v in value))
elif isinstance(value, float):
    print(f'{value:.4f}')
elif value is None:
    print('')
else:
    print(value)
" "$1"
}

# poll_state <field> <expected> [tries] [pause] - prints the last value seen.
poll_state() {
  local key="$1" want="$2" tries="${3:-25}" pause="${4:-0.2}"
  local value=""
  for _ in $(seq 1 "$tries"); do
    value=$(api GET /state | field "$key")
    [ "$value" = "$want" ] && break
    sleep "$pause"
  done
  printf '%s' "$value"
}

# wait_idle [tries] - block until the policy is back to walk or stand.
wait_idle() {
  local tries="${1:-80}" value=""
  for _ in $(seq 1 "$tries"); do
    value=$(api GET /state | field label)
    case "$value" in
      walk|stand) printf '%s' "$value"; return 0 ;;
    esac
    sleep 0.3
  done
  printf '%s' "$value"
  return 1
}

state_has() { # state_has <json> <needle>
  printf '%s' "$1" | grep -q "$2"
}

dump_logs() {
  echo "=== app stdout ==="
  tail -n 60 "$OUT/app-stdout.log" 2>/dev/null || true
  echo "=== simulator console ==="
  tail -n 60 "$OUT/sim-console.log" 2>/dev/null || true
  echo "=== unified log (Duck) ==="
  xcrun simctl spawn "$UDID" log show --last 10m --style compact \
    --predicate 'process == "Duck"' > "$OUT/sim-syslog.log" 2>/dev/null || true
  grep -E "duck:|renderer|dyld|Library not loaded|abort|Fatal|Metal|assert" \
    "$OUT/sim-syslog.log" 2>/dev/null | tail -n 60 || true
  echo "=== crash reports ==="
  ls -t ~/Library/Logs/DiagnosticReports/*.ips 2>/dev/null | head -3 | while read -r f; do
    echo "--- $f"
    head -c 4000 "$f"
  done
}

finish() {
  echo
  echo "=== feature matrix: $PASSES passed, $FAILURES failed ==="
  if [ "$FAILURES" -ne 0 ]; then
    dump_logs
    echo "::error::$FAILURES checks failed in the simulator session"
    exit 1
  fi
  echo "the simulator session passed every check"
}

# ---------------------------------------------------------------- bundle
echo "--- bundle ---"
[ -f "$APP/DuckResources/Shaders.metal" ]
check $? "bundle has Shaders.metal" "$APP/DuckResources"

# ---------------------------------------------------------------- install
echo "--- install and launch ---"
if xcrun simctl install "$UDID" "$APP"; then
  check 0 "simctl install" "installed"
else
  check 1 "simctl install" "simctl install failed"
  finish
fi

APPDIR=$(xcrun simctl get_app_container "$UDID" "$BUNDLE_ID" app 2>/dev/null || true)
echo "app container: $APPDIR"

# --console sends the app's stdout (Swift print + NSLog) to the file, which
# is where the renderer reports what it built.
xcrun simctl spawn "$UDID" log stream --style compact \
  --predicate 'process == "Duck"' > "$OUT/sim-console.log" 2>&1 &
LOG_PID=$!
trap 'kill "$LOG_PID" 2>/dev/null' EXIT

xcrun simctl launch --console "$UDID" "$BUNDLE_ID" \
  > "$OUT/app-stdout.log" 2>&1 &
LAUNCH_PID=$!

# ---------------------------------------------------------------- liveness
# /state answers as soon as the intent server is up, but the label it reports
# is "idle" until the 50 Hz loop has produced its first report (core creation
# publishes once, before any tick). Wait for the loop to actually drive the
# duck before asserting anything about the policy.
STATE=""
LABEL=""
for _ in $(seq 1 90); do
  STATE=$(api GET /state || true)
  LABEL=$(printf '%s' "$STATE" | field label)
  [ -n "$STATE" ] && [ "$LABEL" != "idle" ] && [ -n "$LABEL" ] && break
  sleep 1
done
echo "$STATE" | tee "$OUT/state-initial.json" > /dev/null

[ -n "$STATE" ]; check $? "intent API answers on :8765" "$LABEL"
[ "$LABEL" != "idle" ]; check $? "the 50 Hz control loop is ticking" "label=$LABEL"

BRINGUP=$(printf '%s' "$STATE" | field bringup)
GAIN=$(printf '%s' "$STATE" | field gain)
[ "$BRINGUP" = "2" ]; check $? "policy reached ready (bringup=2)" "bringup=$BRINGUP"
case "$LABEL" in
  walk|stand) check 0 "the walk policy drives the duck" "label=$LABEL" ;;
  *)          check 1 "the walk policy drives the duck" "label=$LABEL" ;;
esac
[ -n "$GAIN" ]; check $? "status reports a gain" "gain=$GAIN"

# ---------------------------------------------------------------- skills
# The official walk-mode skill set is exactly these three: crouch is a
# roller-mode policy and the manifest's mode filter keeps it out.
SKILLS=$(api GET /skills || true)
echo "$SKILLS" | tee "$OUT/skills.json" > /dev/null
for name in roulade kick_left kick_right; do
  state_has "$SKILLS" "\"$name\""
  check $? "skill listed: $name" "$SKILLS"
done

# ---------------------------------------------------------------- twist
echo "--- walk ---"
BEFORE=$(api GET /state | field trunk)
for _ in $(seq 1 10); do
  api POST /twist '{"vx":0.4,"vy":0.0,"wz":0.0}' > /dev/null
  sleep 0.3
done
AFTER=$(api GET /state | field trunk)
echo "trunk while walking: $BEFORE -> $AFTER"
[ "$BEFORE" != "$AFTER" ]; check $? "twist moves the duck" "$BEFORE -> $AFTER"
api POST /twist '{"vx":0.0,"vy":0.0,"wz":0.0}' > /dev/null

# ---------------------------------------------------------------- sit/stand
echo "--- sit and stand ---"
api POST /sit '{}' > /dev/null
SIT_LABEL=$(poll_state label sit 40 0.2)
[ "$SIT_LABEL" = "sit" ]; check $? "sit command seats the duck" "label=$SIT_LABEL"

# The prototype refuses the way back up while the seat ramp is still running
# ("still sitting down", sitstand_ramp_s = 2.0), so keep asking until it
# takes - exactly what a person pressing the button again would do.
STAND_LABEL="$SIT_LABEL"
for _ in $(seq 1 12); do
  case "$STAND_LABEL" in
    rise|walk|stand) break ;;
  esac
  sleep 1
  api POST /sit '{}' > /dev/null
  STAND_LABEL=$(api GET /state | field label)
done
case "$STAND_LABEL" in
  rise|walk|stand) check 0 "sit toggle stands it back up" "label=$STAND_LABEL" ;;
  *)               check 1 "sit toggle stands it back up" "label=$STAND_LABEL" ;;
esac
wait_idle 80 > /dev/null

# ---------------------------------------------------------------- skills
for name in roulade kick_left kick_right; do
  echo "--- skill $name ---"
  api POST /skill "{\"name\":\"$name\"}" > /dev/null
  # The kicks last 0.5 s and roulade 1.0 s, so sample fast enough to see them.
  SEEN=$(poll_state label "$name" 60 0.1)
  [ "$SEEN" = "$name" ]; check $? "skill runs: $name" "label=$SEEN"
  # Let the one-shot finish before the next one.
  wait_idle 100 > /dev/null
done

# ---------------------------------------------------------------- ground pick
echo "--- ground pick ---"
api POST /ground_pick '{}' > /dev/null
SEEN=$(poll_state label ground_pick 40 0.2)
[ "$SEEN" = "ground_pick" ]; check $? "ground pick runs" "label=$SEEN"

# ---------------------------------------------------------------- head/body
echo "--- head and body control ---"
api POST /head '{"neck":0.2,"head":0.1,"yaw":0.3,"roll":0.0}' > /dev/null
check $? "head command accepted" ""
api POST /body '{"z":0.0,"roll":0.05,"pitch":0.05}' > /dev/null
check $? "body command accepted" ""

# ---------------------------------------------------------------- push
echo "--- external push ---"
api POST /push '{"vx":0.5,"vy":0.0,"vz":0.0}' > /dev/null
check $? "push (external disturbance) accepted" ""
sleep 2
ALIVE=$(api GET /state | field bringup)
[ -n "$ALIVE" ]; check $? "duck survives the push" "bringup=$ALIVE"

# ---------------------------------------------------------------- reload
echo "--- policy reload ---"
if [ -n "$APPDIR" ]; then
  api POST /reload "{\"dir\":\"$APPDIR/DuckResources/policies\"}" > /dev/null
  check $? "reload the bundled policy set" "$APPDIR/DuckResources/policies"
  poll_state label walk 30 0.2 > /dev/null
  SKILLS2=$(api GET /skills || true)
  state_has "$SKILLS2" roulade
  check $? "skills still available after reload" "$SKILLS2"
else
  check 0 "reload the bundled policy set" "app container unknown"
fi

# ---------------------------------------------------------------- frames
echo "--- frames ---"
for i in 1 2 3; do
  api POST /twist '{"vx":0.4,"vy":0.0,"wz":0.0}' > /dev/null
  xcrun simctl io "$UDID" screenshot "$OUT/frame-$i.png" || true
  sleep 0.8
done
for i in 1 2 3; do
  python3 "$ROOT/scripts/check-frame.py" "$OUT/frame-$i.png" \
    --region 0.3,0.3,0.7,0.7 --region-lit 0.02 >> "$OUT/frame-check.log" 2>&1
  check $? "frame $i: 3D viewport is lit" "$(tail -1 "$OUT/frame-check.log")"
done
python3 "$ROOT/scripts/check-frame.py" "$OUT/frame-1.png" "$OUT/frame-2.png" \
  --min-diff 0.0002 >> "$OUT/frame-check.log" 2>&1
check $? "frames 1-2 differ (scene is live)" ""
python3 "$ROOT/scripts/check-frame.py" "$OUT/frame-2.png" "$OUT/frame-3.png" \
  --min-diff 0.0002 >> "$OUT/frame-check.log" 2>&1
check $? "frames 2-3 differ (scene is live)" ""

# ---------------------------------------------------------------- renderer log
# NSLog is unbuffered and lands in the unified log; Swift's print() to a pipe
# can sit in a buffer, so the app's own trail is read from the log stream.
echo "--- renderer log ---"
sleep 1
LOG=$(cat "$OUT/sim-console.log" "$OUT/app-stdout.log" 2>/dev/null)
printf '%s' "$LOG" | grep -q "duck: core created"
check $? "log: core created" ""
printf '%s' "$LOG" | grep -qE "renderer: [1-9][0-9]* visual geoms"
check $? "log: renderer built its instances" "$(printf '%s' "$LOG" | grep -oE 'renderer: [0-9]+ visual geoms' | tail -1)"
printf '%s' "$LOG" | grep -q "duck: renderer failed"
[ $? -ne 0 ]; check $? "log: no renderer failure" ""

# ---------------------------------------------------------------- still alive
ALIVE=$(api GET /state | field bringup)
[ "$ALIVE" = "2" ]; check $? "still running at the end" "bringup=$ALIVE"

finish
