import io

p = 'ios/scripts/sim-drive.sh'
s = io.open(p, encoding='utf-8').read()

anchor = '''# ---------------------------------------------------------------- frames'''
block = '''# ---------------------------------------------------------------- time machine
# The time machine is only worth having if it really holds the duck still,
# really advances one tick at a time, and really replays a rewind. Each of
# those is a claim this section checks against /state.
echo "--- time machine ---"

TICK_BEFORE=$(api GET /state | field tick)
api POST /time '{"paused":true}' > /dev/null
sleep 1.2
TICK_PAUSED=$(api GET /state | field tick)
[ "$TICK_BEFORE" = "$TICK_PAUSED" ]; check $? "pause freezes the simulation clock" "$TICK_BEFORE -> $TICK_PAUSED"

PAUSED_FLAG=$(api GET /state | field paused)
[ "$PAUSED_FLAG" = "True" ] || [ "$PAUSED_FLAG" = "true" ]
check $? "state reports the duck is paused" "paused=$PAUSED_FLAG"

api POST /step '{"ticks":10}' > /dev/null
sleep 0.6
TICK_STEPPED=$(api GET /state | field tick)
[ "$TICK_STEPPED" = "$((TICK_PAUSED + 10))" ]
check $? "step advances exactly the ticks asked for" "$TICK_PAUSED -> $TICK_STEPPED (want +10)"

api POST /time '{"speed":0.5}' > /dev/null
api POST /time '{"paused":false}' > /dev/null
sleep 2
TICK_SLOW=$(api GET /state | field tick)
ELAPSED=$((TICK_SLOW - TICK_STEPPED))
# 2 s at 0.5x is about 50 ticks; allow scheduling slack either way.
[ "$ELAPSED" -ge 25 ] && [ "$ELAPSED" -le 75 ]
check $? "half speed advances about half as fast" "$ELAPSED ticks in 2 s"
api POST /time '{"speed":1.0}' > /dev/null

# A checkpoint exists for the ticks that have run, and a rewind lands on one.
TIMELINE=$(api GET /timeline || true)
echo "$TIMELINE" | tee "$OUT/timeline.json" > /dev/null
printf '%s' "$TIMELINE" | grep -q '"newest"'
check $? "the timeline lists checkpoints" "$(printf '%s' "$TIMELINE" | head -c 120)"

api POST /checkpoint '{}' > /dev/null
sleep 0.4
BEFORE_SEEK=$(api GET /state | field tick)
api POST /seek '{"ticks":20}' > /dev/null
sleep 0.6
AFTER_SEEK=$(api GET /state | field tick)
[ -n "$AFTER_SEEK" ] && [ "$AFTER_SEEK" -lt "$BEFORE_SEEK" ]
check $? "seek rewinds the duck" "$BEFORE_SEEK -> $AFTER_SEEK"

# A rewind must resume the same duck, not a reset one: it is still ready and
# still being driven by the policy, with the body where the checkpoint left it.
SEEK_LABEL=$(api GET /state | field label)
SEEK_BRINGUP=$(api GET /state | field bringup)
case "$SEEK_LABEL" in
  walk|stand|sit|rise|idle|held) check 0 "the rewound duck is still driving" "label=$SEEK_LABEL" ;;
  *) check 1 "the rewound duck is still driving" "label=$SEEK_LABEL" ;;
esac
[ "$SEEK_BRINGUP" = "2" ]; check $? "the rewound duck is still ready" "bringup=$SEEK_BRINGUP"

api POST /time '{"paused":false}' > /dev/null
api POST /time '{"speed":1.0}' > /dev/null

# ---------------------------------------------------------------- frames'''

assert anchor in s
s = s.replace(anchor, block, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
