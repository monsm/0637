# 可玩性与自主玩法（第二轮脑暴，2026-11-26）

方法：六个维度并行读仓库——**自主生活**、**跨功能组合**、**身体与物理**、**声音与听觉**、**成长与叙事**、**单机假想玩法**——产出 61 条，统一审计对照已交付的十二张游戏去重、按地基与可玩性淘汰，保留 **10 条**（其中 **7 条鸭子自主发起**）。

红线不变：不碰 `core/` 的 61 维观测与安全层；只用真实动词与 `/state` 已发布字段；单机可玩；每条都可脚本化验收。

## 保留（10 条，按可玩性/成本排序）

### 游戏卡标注今日目标（Game cards show which quest they help） · 可玩性 8/10 · small
- **玩法**：Free tier, default on. Every 玩一玩 card reads /state.quests and shows a one-line chip naming which of today's three goals its real actions move (kick rally → 做 2 个动作; mood charade → 做 2 个表情; sound simon → 拍手两次; red light → 走 2 米). The chip is a pure function of the /state.quests object — no local copy, no invented progress; no quests posted → no chip. The score stays exactly the vet-book counter.
- **自主**：none — the child plays; the card only points at the same real counter.
- **真实地基**：/state.quests (quests.rs summary: three quests with metric/target/baseline/done plus streak/best_streak) crossed with the twelve inline games (PlayMoreGames.swift, PlayMoreGames.kt), which already drive life.skills/emotions/ground_picks/metres/claps/whistles through the real intent route; the metric→counter mapping is quests.rs metric_value() itself.
- **验收**：With a skills quest posted for the day, the kick-rally card shows that quest title; after six real kicks (life.skills.kick_left +6) /state.quests progress for that quest increases and the card annotation recomputes from the new /state.quests on the next poll; with no quests posted the cards show no annotation. Both ends: the annotation string is a pure unit-tested function of a canned /state.quests object (no invented numbers).

### 编舞铺收录道具动作（Dance steps that play a prop） · 可玩性 8/10 · medium
- **玩法**：Opt-in. Recording a dance captures the five prop verbs as first-class steps. Primary-source correction: on iOS the ball/hoop/mirror/cushion prop taps already route through controller.trigger()/sit(), which call dance.record('skill',…) — so those already record while recording is on. The real gap is (a) the block's ground_pick, which records nothing, and (b) Android, where reactProp() calls core.skill()/groundPick() directly and records nothing. Playback replays the same real intent; the prop answers from the counter it already mirrors; a refused verb records nothing and the prop stays still.
- **自主**：none — recording and playback are child-driven; the duck only does the recorded real verb.
- **真实地基**：DanceStudio (DanceStudio.swift record/play, DanceStudio.kt) whose DanceStep is kind/value/label and replays through the same intent route; StageProps propagate/reactProp (StageProps.swift, StageProps.kt): ball=kick_left, hoop=roulade, block=ground_pick, cushion=sit, mirror=emotion; the prop cues already follow life.skills.kick_left/roulade, ground_picks, sit_downs, emotions (PropCues/parsePropCues, StageProps.kt cues).
- **验收**：iOS: open Dance Studio, press Record, dismiss the sheet, tap the block → a step is recorded with kind/value 'ground_pick'; press Play → /state.life.ground_picks +1 and the block-carry cue fires. Android: tap any of the five props while recording → a step appears and playback moves the matching counter. A refused verb records no step. (The ball-on-iOS case already records today, so it proves nothing and is not the assertion.)

### 道具分组积木规则（Prop-grouped block rules） · 可玩性 7/10 · medium
- **玩法**：Opt-in, shell-side only (rules.rs untouched). The block editor's action picker groups the five prop verbs under the prop icons (ball=kick_left, hoop=roulade, block=ground_pick, cushion=sit, mirror=emotion), so a child writes '拍手 → 踢球'; the rule still POSTs through the existing rules channel and executes the real verb; the prop answers from the counter increment it already mirrors. No new trigger or action word is introduced.
- **自主**：none — the duck performs the rule's real verb; the prop cue is the shell-side read of the counter it just earned.
- **真实地基**：rules.rs TRIGGERS=[clap,whistle,pet,timer] and ACTIONS=[skill,emotion,sound,light] (unchanged); StageProps PropCues (StageProps.swift) / StageProps.kt cues already react to life.skills.kick_left, roulade, ground_picks, sit_downs, emotions.
- **验收**：Core test: POST /rules {trigger:'clap',action:'skill',value:'kick_left'}, fire a clap sound reading → /state.rule_event shows the rule and life.skills.kick_left +1 (and the ball flight cue fires); a rejected value leaves the rule set unchanged. UI test both ends: the editor's prop groups post exactly these existing values, never a new trigger/action word.

### 高兴就翻个滚（A Happy Roll） · 可玩性 5/10 · small
- **玩法**：Opt-in. When care.mood crosses up through +0.8 while the label is walk|stand and the duck is free (awake, not paused, not carrying, no emotion), it performs a real roulade then a real yes. It re-arms only after mood falls below 0.7, so one happy spell is one roll. One intent per beat through the shared coordinator.
- **自主**：No tap: when it gets that happy it rolls once and nods.
- **真实地基**：/state.care.mood (care.rs target = 0.55 - 0.9*hunger - neglect/300 + 0.25*bond; /pet adds +0.12, /feed +0.1), /state.label, /state.emotion, carrying, paused; POST /skill {'name':'roulade'} → life.skills.roulade, POST /skill {'name':'yes'} → life.emotions.yes.
- **验收**：POST /care {'mood':0.75,...} then POST /pet (care.pet adds +0.12 → mood ≥ 0.87) → within 5 s life.skills.roulade +1 and life.emotions.yes +1 with no other command; then POST /care {'mood':0.5,...} and raise again → a second roll only after the re-arm (pure unit test on the arming rule: one upward crossing never fires twice).

### 自己去散步（It Walks a Route of Its Own） · 可玩性 5/10 · small
- **玩法**：Opt-in. Every ~3 min of idle with no active path and not carrying, the shell lays a real 3-5 point route from the duck's fingerprint+tick (inside PATH_MAX_POINTS=64 / PATH_MAX_LENGTH=20 m) and POSTs /path. The walking is the core's own path follower (PATH_SPEED 0.35, CMD_ALPHA 0.2). path{reached,total} and life.metres say how far it really got; a slip is reported as it is. Not the shipped Path Painter (the child draws); here nobody drew anything.
- **自主**：No tap: it sets off on a short route the app laid and really walks it.
- **真实地基**：POST /path {points} (max 64 points, 20 m), POST /path/cancel; /state.path{active,points,reached,total}, /state.life.metres, /state.life.falls, /state.pose{position,yaw}, /state.label, paused; the same guards as selfPlayDue (awake, not paused, not carrying, no emotion, idle.takeover > 0.1, idle_s ≥ 10 s).
- **验收**：Switch on, no taps: within 30 s /state.path.active==true with path.total ≥ 3 and path.reached growing; when the round ends path.active==false and life.metres rose ≥ 0.3 m; a fall is reported as life.falls +1. A second route waits for the cooldown (pure unit test: same fingerprint+tick → same route).

### 自己开一场独角戏（A One-Duck Show） · 可玩性 5/10 · medium
- **玩法**：Opt-in. On a long derived idle (≥ 240 s), once per long idle: a three-beat routine — hoop (real roulade), mirror (real curious), ball (real kick_left) — each beat waiting for the previous to finish, with /state.emotion{duration_s} as the beat clock (the same clock Mood Freeze uses). All three are standing verbs, so the show never leaves the duck seated. A refused beat is skipped and named skipped; the show never claims a move it did not get. Shares the coordinator and the 1250-tick cooldown.
- **自主**：No tap: left alone long enough, it puts on a three-beat show at its own props.
- **真实地基**：POST /skill roulade/curious/kick_left; /state.life.skills.roulade, /state.life.emotions.curious, /state.life.skills.kick_left; /state.emotion{name,duration_s}; label/paused/emotion guards; the self-play coordinator's 1250-tick cooldown (StageProps.swift).
- **验收**：Restore care.last_touch to a derived idle of 250 s with the switch on: within a minute and with no taps, life.skills.roulade +1, then life.emotions.curious +1, then life.skills.kick_left +1, in that order; a beat refused while busy is skipped (assert the next counter still arrives and the show records the skip). With the switch off the same restore moves no counter.

### 饿了会哼唧（The Hungry Grumble） · 可玩性 4/10 · small
- **玩法**：Free tier, default on. When care.hunger ≥ 0.85, awake, calm (no emotion timeline) and the derived idle span ≥ 30 s, it queues its own inquire at most once per 120 simulated seconds (cooldown on /state.tick). Sound-only: POST /sound never calls care.touch, so it complains without resetting the doze. /feed drops hunger by 0.45 and it stops. One free-sound slot, below the sleep sounds so a dozing duck never grumbles.
- **自主**：No tap: a hungry duck grumbles on its own, and goes quiet once it has been fed.
- **真实地基**：/state.care.hunger, care.asleep, /state.emotion, /state.paused, /state.tick, and the idle span derived from /state.care.last_touch + /state.tick + /state.care_time_scale (care.summary publishes last_touch, not idle_s; care.tick computes (tick-last_touch)*0.02*scale, DOZE_AFTER_S=720); POST /sound {'tag':'inquire'} → /state.last_sound; HUNGER_SECONDS=2700, feed() −0.45 hunger.
- **验收**：POST /care {'hunger':0.9,'asleep':false,'last_touch':<tick-2000>} → on the next poll /state.last_sound=='inquire'; POST /feed → /state.care.hunger ≤ 0.45. The 120 s cooldown and post-feed silence are pure-function unit tests on both ends (last_sound is single-valued, so the smoke covers the one real trigger).

### 睡前哼一声（The Last Little Sound Before It Dozes） · 可玩性 4/10 · small
- **玩法**：Free tier, default on, no switch. As its derived idle span crosses 600 s it queues its own coo once; at 690 s a second, quieter chirp; the stage dims a notch at each beat. Both go through POST /sound, which never calls care.touch, so the sound cannot postpone the doze (DOZE_AFTER_S=720 care-s) it announces. Runs on the existing 1 Hz poll; one cooldown slot in the shared idle-life coordinator. (The walk scene has no mouth joint, so this audible beat is the honest equivalent of the upstream yawn.)
- **自主**：No tap: as its own idle clock runs out it makes a settling coo, then a chirp, and still dozes on schedule.
- **真实地基**：/state.care.last_touch + /state.tick + /state.care_time_scale → the same idle span care.tick uses ((tick-last_touch)*0.02*scale); care.asleep, /state.emotion (null), paused, carrying; POST /sound {'tag':'coo'} then {'tag':'chirp'} → /state.last_sound; the doze is care.rs DOZE_AFTER_S=720. Correction: /state.care.idle_s is not a published field (care.summary publishes last_touch), and the span IS care-scaled — the candidate's 'unscaled' claim is false; the span must be derived.
- **验收**：At care_time_scale 1: POST /care {'hunger':0.4,'mood':0.3,'bond':0.4,'asleep':false,'last_touch':<tick-30000>} → derived idle 600 s, care.asleep false → on the next calm poll /state.last_sound=='coo' and care.last_touch is unchanged by the sound; then POST /config {'care_time_scale':200} → care.asleep flips true within a second (the sound did not reset the clock). Read last_sound on the first poll after the trigger.

### 练最不熟的那个把戏（It Practises Its Weakest Trick） · 可玩性 4/10 · small
- **玩法**：Opt-in. Between self-play beats the chooser reads GET /skills, keeps only the one-shots that currently load in this mode (never a missing or roller-only trick), picks the smallest real count among roulade, kick_left, kick_right (life.skills) and ground_pick (life.ground_picks), ties broken by fingerprint, and performs that verb for real. Guards: label walk|stand, not paused, no emotion, not carrying; a refusal adds nothing and is retried next beat. Shares the coordinator.
- **自主**：No tap: it works on the one trick its own vet book says it is worst at.
- **真实地基**：GET /skills; /state.life.skills{roulade,kick_left,kick_right}, /state.life.ground_picks, /state.label, paused, carrying, emotion; POST /skill {'name':...}, POST /ground_pick; a refusal returns ok:false and no counter moves; the existing self-play guards/cooldown (StageProps.swift selfPlayDue/selfPlayProp).
- **验收**：Seed POST /skill {'name':'kick_left'} three times (life.skills.kick_left==3), leave roulade at 0; switch on; within 45 s /state.life.skills.roulade +1 and kick_left still 3. The chooser is a pure unit test on both ends, including an empty /skills list (no pick, nothing posted) and missing counters read as 0.

### 困了自己回窝（It Takes Itself to the Cushion） · 可玩性 4/10 · medium
- **玩法**：Opt-in, behind the existing scene switch. At a derived idle of 660 s, awake, standing (label walk|stand) and free, the duck takes a short real step (POST /twist) onto the cushion, then a real sit (POST /posture {sit:true}) and a coo; on the next wake edge (care.asleep true→false) it posts a real stand (POST /posture {sit:false} → label 'rise'). The sit is attention, so the doze honestly moves twelve minutes later — the shell says so rather than claiming it slept on time. One intent per beat; shares the 1250-tick cooldown.
- **自主**：No tap: when it is worn out it walks onto its cushion, sits, and gets itself up after the nap.
- **真实地基**：/state.care.last_touch + tick + care_time_scale (derived idle; DOZE_AFTER_S=720), care.asleep, /state.label, /state.emotion, paused, carrying, idle.takeover; POST /twist → life.metres; POST /posture {'sit':true} → label 'sit' → life.sit_downs; POST /posture {'sit':false} → label 'rise' → life.stand_ups (runtime.rs:5058-5061); POST /pet wakes (care.pet).
- **验收**：care_time_scale 1; restore last_touch to a derived idle of 670 s with the switch on → within 30 s /state.life.metres rose ≥ 0.1 m and life.sit_downs +1 with no tap, /state.label=='sit'. Then POST /pet (a real wake) → within 10 s life.stand_ups +1 and /state.label=='rise'. The same restore with the switch off moves neither counter (only the free settling sound plays).

## 本批先做（三条自主）

- **高兴就翻个滚**：`care.mood` 上穿 0.8 时自己做一次真实 `roulade` 再点头，掉回 0.7 才重新武装。
- **饿了会哼唧**：`care.hunger ≥ 0.8` 且闲着够久时自己用 `inquire` 哼一声，喂下去（<0.6）才重新武装。
- **自己去散步**：开了「自己玩」后，自娱的节拍一半是道具、一半是**自己走一小段**（真实速度通道 2 秒）——没人画线、没人推，里程表是证据。

其余保留项（游戏卡标注今日目标、编舞铺收录道具动作、道具分组积木规则、自己开一场独角戏、睡前哼一声、困了自己回窝、练最不熟的那个把戏）留作后批。

## 淘汰与缓办（51 条）

| 名字 | 理由 |
|---|---|
| 心情灯 | Duplicate of 情绪配色脸灯（弃用） in the same candidate list — identical mechanic (emotion start edge → fixed colour via POST /flashlight), so keeping both is a dedupe failure. Independently: the flashlight is a child-owned toggle on both ends (Shell.kt toggleFlashlight / the iOS instrument row), and rules.rs ACTIONS already includes 'light', so an auto mood light would fight both the child's own torch and a child's own light rule. (Correction to the older rejection's wording: the instrument sheet ties colour words only to flashlight/battery/status, not to moods.) |
| 说梦话 | Shares the single free-sound slot with 睡前哼一声 and 饿了会哼唧, has the lowest playability of the three, and the night already answers with its own sounds; deferred behind the two kept sleep sounds. |
| 心情决定它玩什么 | Thin variant of the shipped self-play chooser (StageProps.swift selfPlayProp already picks the prop per beat); and it competes with the kept 练最不熟的那个把戏 for the same coordinator beat. Deferred. |
| 散完步点个头 | Deferred — valid and scriptable, but the yes touches care and it competes for the same coordinator beat as 自己去散步 / 自己开一场独角戏; ranked below the ten kept survivors (batch cap 10). |
| 对着镜子练表情 | Mutual duplicate of 难过之后自己照照镜子 (both end in 'walk to mirror + a mood'), and the trigger is timing-flaky: it depends on polling idle.head[2] across the 1 Hz poll and on unverified upstream glance parameters. Deferred. |
| 难过之后自己照照镜子 | Mutual duplicate of 对着镜子练表情 (both are 'mirror + mood'), and the walk 'toward the mirror' is a shell-composed path with no published mirror position (the prop is a drawing, not a /state field). Deferred. |
| 久别重逢 | Care only ages while the core ticks: a suspended app is not a duration the duck has, and the restored idle_s is the idle span at save time, not the absence. A greeting keyed to it would claim time the duck never lived; the core also already greets on boot. |
| 低电量求助 | leds.battery_percent is the BAM load-sag of the terminal voltage, not a state of charge: it dips during hard work and recovers at rest, and the core has no depletion model. A 'low battery' behaviour would fire on a heavy walk and vanish when the duck stops — an artifact of load, not a fact about the pack. |
| 心跳驱动的行为 | heart_bpm is a pure function of mood, hunger and bond (care.rs:217) - a behaviour keyed to it is the same behaviour keyed to those three fields with extra steps. The shells already draw it (HeartRipple), so it is a display, not a new signal. |
| 夜里自己走动 | Every body verb calls care.touch and wakes the duck, so the shell would have to wake it to make it move and then call that sleeping. Fake. Sound and light are the only honest night behaviours. |
| 摔倒后照照镜子 | No deterministic real fall exists in the harness - sim-drive's /push is chosen to be survived and asserts survival; the only falls are emergent slips, so the trigger cannot be scripted. The core already reacts to a fall with alarm. |
| 拍手就亮灯 | iOS already ships clap -> light as a block-rule action (rules.rs ACTIONS has 'light'; Android Companion.kt RULE_ACTIONS omitting it is a parity gap to fix, not a new idea), so a default light answer would compete with the child's own rule; the core already answers a clap with a real laugh. The duck would do nothing new. |
| 自娱频率旋钮 | A settings control over an existing core behaviour (the idle ritual already ships at 30 s), not a behaviour the duck performs. The dimension asks for life, not a slider. |
| 嘴型与呼吸 | POST /mouth is refused on the walk and roller scenes (has_mouth=false; runtime.rs:1725, mujoco.rs:1260), and a breathing body pose is either the upstream idle head's own breath or a held /body that touches care every tick and prevents the doze. A shell 'breathing' would be an animation over a command, not the duck's motion. |
| 编舞铺收录画路 | Deferred to a later batch - valid and scriptable, but largest effort in the pool; the first batch stays small (batch cap 10). |
| 给今日目标建一条规则 | Deferred - valid, real and scriptable, but ranked below the ten kept survivors (batch cap 10). |
| 编舞铺标注今日目标 | Deferred - same crossing pattern as the kept 游戏卡标注今日目标 (a surface annotating /state.quests), so it loses the dedupe tie; batch cap 10. |
| 日记里的游戏账 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| AI 复盘一场游戏 | Deferred - overlaps the kept 游戏卡标注今日目标 on the game surface and the AI surface; ranked below the ten kept survivors (batch cap 10). |
| AI 写一条积木规则 | Deferred - large effort, and it overlaps the kept 道具分组积木规则 on the rule-authoring surface; a model-output trust boundary should not be in the first batch (batch cap 10). |
| AI 朝着今日目标出招 | Deferred - valid and small, ranked below the ten kept survivors (batch cap 10). |
| 听摸摸声的规则 | Deferred - needs a core rules.rs TRIGGERS change plus a fire_rules('sound_pet') call in apply_petting (outside the protected 61-dim/safety layer, but still a core edit); ranked below the ten kept survivors (batch cap 10). |
| 今日解锁一个新把戏 | Deferred - requires a new Metric variant in core quests.rs (a core feature addition derived from the real life.unlocks field); ranked below the ten kept survivors (batch cap 10). |
| 走路时道具亮起来 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 终身记录里的今日目标 | Deferred - valid and small, ranked below the ten kept survivors (batch cap 10). |
| 连续完成徽章 | Deferred - valid and small, ranked below the ten kept survivors (batch cap 10). |
| 情绪配色脸灯（弃用） | Exact duplicate of 心情灯 in this list (emotion start edge → fixed face-light colour), and the flashlight is a child-owned toggle on both ends (Shell.kt toggleFlashlight / iOS instrument row) while rules.rs ACTIONS already has 'light' — an auto mood light would fight both the child's torch and the child's own light rules. (Correction: the instrument sheet does not currently tie colour to moods; leds.color is set only by POST /flashlight.) |
| 编舞解锁徽章（弃用） | Reskin: dance playback already flows through the real intent route, so life.skills/emotions and life.unlocks already update and the collection page already shows them; no new field crosses. |
| 低杆过鸭 | Deferred - valid and scriptable physics game, ranked below the ten kept survivors (batch cap 10). |
| 端稳了 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 顶住这阵风 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 刹车线 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 学它转头 | Deferred - valid and small, ranked below the ten kept survivors (batch cap 10). |
| 慢放推演 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 啾停站好 | Deferred - valid and small, ranked below the ten kept survivors (batch cap 10). |
| 赢过它自己 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 吊鸭过桥 | Deferred - the weaker sibling of the kept 端稳了 (both are carry games), ranked below the ten kept survivors (batch cap 10). |
| 张嘴接食 | The premise is false on the shipped scenes: has_mouth is false (runtime.rs:1725), so /mouth is refused and the mouth field never moves; the opening beak is the shell's own visualMouth, not a state change. Scoring it would score a shell animation. life.fed is real but belongs to the shipped /feed care action. |
| 轻着陆 | core/duck-mobile-core/src/vetbook.rs declares and publishes soft_landings (lines 40 and 602) but no code in the mobile core increments it (grep confirms only those two hits); only upstream robotd's vet book does. A score built on it would be a constant 0 dressed as a fact. The honest counter for a fall idea is life.falls. |
| 推积木 | /push moves only the duck's own trunk; the stage props are drawings with no bodies and no /state field. A rolling block would be invented physics. The duck's own slide is real and is what 端稳了 / 顶住这阵风 / 慢放推演 measure instead. |
| 接住坠鸭 | The only published fall facts are the latched fallen verdict and life.falls; the fall predictor's start and the limp-fall state are not in /state. A catch window could only be read from the render, so a lucky tap is indistinguishable from skill and no script can assert the catch. |
| 叫它起床 | Deferred - valid and scriptable, but it is the same sleep/wake loop as 嘘，它在睡; keep one, and it ranks below the ten kept survivors (batch cap 10). |
| 拍手走，口哨停 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 斗嘴 | Duplicates the core's shipped default sound-reaction loop (clap→laugh, whistle→curious, busy→coo/inquire, runtime.rs:2636-2655); the shell would add only a scoring wrapper over an unchanged loop. Deferred. |
| 跟它拍 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 它有话问你 | Deferred - overlaps the shipped whisper game's /heard + speech loop; ranked below the ten kept survivors (batch cap 10). |
| 倒着还给它 | Duplicates the shipped Echo game loop with only the answer order reversed (sound simon + echo game already ship); deferred. |
| 嘘，它在睡 | Deferred - the inverse of 叫它起床 (same sleep/wake loop); keep at most one, and it ranks below the ten kept survivors (batch cap 10). |
| 一声点将 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 声音暗号 | Deferred - valid and scriptable, ranked below the ten kept survivors (batch cap 10). |
| 隔空摸摸 | The candidate text is truncated, and what is visible is the loop that already ships: the core's pet-by-sound (apply_petting coos and increments life.sound_pets, /state.pet_detect publishes the edge) with the shells already staging pet_detect.onnx (PetAudio.swift/MicFeed, PetSound.kt/MicFeed). A UI around it would be a wrapper, not a new behaviour. |
