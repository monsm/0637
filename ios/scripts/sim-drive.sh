#!/usr/bin/env bash
# A scripted simulator session — the Xcode-simulator equivalent of what a
# developer does by hand: install, launch, watch the log, drive the app, look
# at the frames.
#
# The app's own intent API is the control surface, so the whole session can be
# driven from the shell: a twist makes the duck walk, a skill makes it roll,
# and /state reports what the policy is doing. Everything the script asserts
# is a claim the simulator can really make, so a failure here means the app is
# broken on the simulator, not that the test is flaky.
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

step() { echo "--- $* ---"; }
fail() {
  echo "::error::$*"
  dump_logs
  exit 1
}

api() {
  local method="$1" path="$2" body="${3:-}"
  if [ "$method" = "GET" ]; then
    curl -sf --max-time 5 "$API$path"
  else
    curl -sf --max-time 5 -X POST -H 'Content-Type: application/json' \
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
key = sys.argv[1]
value = d.get(key)
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

dump_logs() {
  echo "=== simulator console ==="
  tail -n 80 "$OUT/sim-console.log" 2>/dev/null || true
  echo "=== unified log (Duck) ==="
  xcrun simctl spawn "$UDID" log show --last 10m --style compact \
    --predicate 'process == "Duck"' > "$OUT/sim-syslog.log" 2>/dev/null || true
  grep -E "duck:|dyld|Library not loaded|abort|Fatal|crash|Metal|assert" \
    "$OUT/sim-syslog.log" 2>/dev/null | tail -n 60 || true
  echo "=== crash reports ==="
  ls -t ~/Library/Logs/DiagnosticReports/*.ips 2>/dev/null | head -3 | while read -r f; do
    echo "--- $f"
    head -c 4000 "$f"
  done
  echo "=== frames ==="
  ls -la "$OUT"/*.png 2>/dev/null || true
}

# ---------------------------------------------------------------- install
step "installing $APP"
xcrun simctl install "$UDID" "$APP" || fail "simctl install failed"

# ---------------------------------------------------------------- launch
step "launching $BUNDLE_ID"
xcrun simctl spawn "$UDID" log stream --style compact \
  --predicate 'process == "Duck"' > "$OUT/sim-console.log" 2>&1 &
LOG_PID=$!
trap 'kill "$LOG_PID" 2>/dev/null' EXIT

xcrun simctl launch "$UDID" "$BUNDLE_ID" | tee "$OUT/sim-launch.log" \
  || fail "simctl launch failed"

# ---------------------------------------------------------------- liveness
# /state only answers once MuJoCo loaded the scene, the ONNX policies loaded
# and the 50 Hz loop is ticking.
step "waiting for the intent API"
STATE=""
for _ in $(seq 1 60); do
  STATE=$(api GET /state || true)
  [ -n "$STATE" ] && break
  sleep 3
done
[ -n "$STATE" ] || fail "the intent API never answered — the core is not ticking"
echo "$STATE" | tee "$OUT/state-initial.json"

BRINGUP=$(printf '%s' "$STATE" | field bringup)
LABEL=$(printf '%s' "$STATE" | field label)
echo "bringup=$BRINGUP label=$LABEL"

# bringup 2 means the walk policy is in control: the ONNX policy really ran.
[ "$BRINGUP" = "2" ] \
  || fail "the policy never reached the ready state (bringup=$BRINGUP, want 2)"

SKILLS=$(api GET /skills || true)
echo "$SKILLS" | tee "$OUT/skills.json"

# ---------------------------------------------------------------- drive it
step "driving the duck: walk forward"
# Keep the twist coming: the API holds the duck for a lease, and the app
# takes the joystick back when the API goes quiet.
for _ in $(seq 1 12); do
  api POST /twist '{"vx":0.4,"vy":0.0,"wz":0.0}' > /dev/null \
    || fail "the twist command was rejected"
  sleep 0.3
done
STATE2=$(api GET /state || true)
[ -n "$STATE2" ] || fail "the intent API stopped answering while walking"
echo "$STATE2" | tee "$OUT/state-walking.json"

TRUNK_A=$(printf '%s' "$STATE" | field trunk)
TRUNK_B=$(printf '%s' "$STATE2" | field trunk)
echo "trunk $TRUNK_A -> $TRUNK_B"
if [ "$TRUNK_A" = "$TRUNK_B" ]; then
  echo "::warning::the trunk did not move over 4 s of walking — the duck may be stuck"
fi

step "driving the duck: sit"
api POST /sit '{}' > /dev/null || fail "the sit command was rejected"
sleep 3
api GET /state | tee "$OUT/state-sitting.json" > /dev/null

step "driving the duck: roll"
api POST /skill '{"name":"roulade"}' > /dev/null || fail "the skill command was rejected"
sleep 3
api GET /state | tee "$OUT/state-rolling.json" > /dev/null

# ---------------------------------------------------------------- frames
# Three frames a moment apart. They must not be blank, and they must not be
# identical: a frozen renderer (or a Metal pipeline that never came up) is the
# other way this app can look broken.
step "capturing frames while the duck walks"
for i in 1 2 3; do
  api POST /twist '{"vx":0.4,"vy":0.0,"wz":0.0}' > /dev/null || fail "the twist was rejected"
  xcrun simctl io "$UDID" screenshot "$OUT/frame-$i.png" || fail "screenshot $i failed"
  sleep 0.8
done

for i in 1 2 3; do
  python3 "$ROOT/scripts/check-frame.py" "$OUT/frame-$i.png" \
    || fail "frame $i is blank — the app is showing a black screen"
done
python3 "$ROOT/scripts/check-frame.py" "$OUT/frame-1.png" "$OUT/frame-2.png" \
  --min-diff 0.0002 \
  || fail "frames 1 and 2 are identical — the renderer is frozen"
python3 "$ROOT/scripts/check-frame.py" "$OUT/frame-2.png" "$OUT/frame-3.png" \
  --min-diff 0.0002 \
  || fail "frames 2 and 3 are identical — the renderer is frozen"

step "the simulator session passed"
echo "intent API: $STATE"
echo "skills:     $SKILLS"
