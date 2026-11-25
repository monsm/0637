# 全维度玩法清单（2026-11-25 脑暴 + 审计）

方法：四个维度并行读仓库——**身体/触摸**、**声音/情绪/灯**、**陪伴/成长/记录/时间**、**社交/AI/规则/外部**——产出 56 条；再统一审计，按红线去重、按可玩性与地基淘汰，净留 **12 条**。

原则不变：不碰 `core/` 的 61 维观测与安全层；每条只挂在真实动词与 `/state` 已发布字段上；每条都要可脚本化验收；单机可玩（需要第二台设备的一律淘汰）。

## 保留（12 条，按可玩性/成本排序）

### 声音指挥官（Room-Sound Simon） · 可玩性 5/5 · small
- **玩法**：The app calls a target - 'make it laugh' or 'make it curious' - and the child makes the matching room sound into the phone's mic (clap or whistle). The duck's own classifier hears it and answers with its real default reaction: clap -> official laugh, whistle -> official curious. The score is the duck's own events; if the child has a matching clap block rule, the rule owns the moment and the game shows that instead of pretending.
- **真实地基**：POST /sensor {"sensor":"microphone","value":{"rms":...,"peak_hz":...}} -> Sensors/SoundWatch -> runtime.sound_event(SoundKind); GET /state.sound_event{kind:"clap"|"whistle",tick}; /state.life.claps / .whistles; the runtime's default reactions (clap->laugh, whistle->curious) and fire_rules precedence (a matching block owns the moment and the default stands down).
- **验收**：With the mic on and the duck free, POST a synthetic clap reading to /sensor (the short loud high transient the sensors test uses) -> GET /state.sound_event.kind=="clap", life.claps +1, emotion.name=="laugh"; the synthetic whistle -> kind=="whistle", life.whistles +1, emotion.name=="curious". With a clap rule installed, /state.rule_event shows the rule and no default laugh is awarded.

### 表情木头人（Mood Freeze） · 可玩性 5/5 · small
- **玩法**：The duck is the caller: it performs a real mood; while it is excited the children dance, and when it switches to angry or no they freeze. The duck's own timeline length is the round clock, published, so there are no arguments about the beat.
- **真实地基**：POST /skill {"name":<mood>}; GET /state.emotion{name,elapsed_s,duration_s} - duration_s is that mood's real timeline (excited 3.3 s, angry 2.5 s, mock 2.5 s, devastated 8.5 s, yes/yes_fast 1.4 s); /state.last_emotion_sound.
- **验收**：POST /skill {"name":"excited"} -> /state.emotion.name=="excited" with duration_s==3.3; the banner follows emotion.name exactly and shows 'freeze' on the poll where emotion==null; a round with "angry" shows duration_s==2.5. A refused mood is shown as refused, not counted as a round.

### 绿灯行红灯停（Red Light, Green Light） · 可玩性 5/5 · medium
- **玩法**：The child is the traffic light. On green the app keeps sending a real forward twist through the same drive channel the stick uses; on red it sends zero every tick and the duck must stop dead. A fall ends the round, and the whole round is judged on the duck's real poses, metres and falls.
- **真实地基**：POST /twist {"vx":>0,"vy":0,"wz":0} on green and {"vx":0,"vy":0,"wz":0} on red (the stick's own channel); GET /state.pose.position, /state.pose.yaw, /state.tick, /state.life.metres, /state.life.falls.
- **验收**：After a red is posted (zero twist every tick, so the stop is commanded and not a deadman artifact), let the duck settle 0.2 s, then sample pose.position at tick t and t+25 (0.5 s); assert displacement <= 0.10 m. After 3 s of green assert life.metres rose >= 0.3. Assert life.falls is unchanged across the round.

### 照我做（Posture Mirror） · 可玩性 5/5 · medium
- **玩法**：A page game like Echo and RPS, not the stage's emotion mirror. It shows one of the duck's real body poses at a time - sit, stand, ground_pick (the crouch), roulade - and the child must make the duck do that same real verb before the page moves on. An optional head-turn pose uses /head yaw and is judged as a UI assertion. The score is the duck's own vet-book counters.
- **真实地基**：POST /posture {"sit":true|false} (the loop's sit_toggle; answers 'unchanged' when already in that posture; refused when paused or while the policy is not driving), POST /ground_pick, POST /skill {"name":"roulade"}, POST /head {"yaw":...}; GET /state.life.sit_downs / .stand_ups (counted on the 'sit'/'rise' label edges), .ground_picks (the crouch's rising edge), .skills.roulade.
- **验收**：Complete a four-pose sequence (sit, stand, ground_pick, roulade); assert life.sit_downs, life.stand_ups, life.ground_picks and life.skills.roulade each rose by 1. A pose the duck is already in answers 'unchanged' and adds nothing. The optional head pose passes on a UI assertion that the rendered head yaw matches the shown side.

### 快进的一天（Fast-Forward Day） · 可玩性 5/5 · medium
- **玩法**：The child opens a time-lapse toy that runs the duck's own tummy clock at 60x (the same real knob CI uses at 200x). In about a minute a whole care-day happens for real: hunger climbs, mood sags, it gets sad and dozes; the child feeds, pets and finally tucks it in. Turning the toy off returns to 1x. The speed is a session setting, and the fast clock does not shorten a night: /state.night.quiet_s stays tick-based.
- **真实地基**：POST /config {"care_time_scale":N} (core clamps 0..10000; HUNGER_SECONDS=2700 s, DOZE_AFTER_S=720 s care-seconds); GET /state.care{hunger,mood,asleep}, /state.care_time_scale; POST /feed, /pet, /night {"on":true|false}; /state.night.quiet_s = tick delta * 0.02, deliberately not care-scaled.
- **验收**：Real /state change: iOS sim-drive sets care_time_scale=60, polls /state.care.hunger from ~0.15 to >0.5 (bounded ~20 s), POSTs /feed, asserts hunger falls by >=0.4 and mood rises, and asserts care_time_scale==60 in the mode and 1 after exit. UI assertion (XCUITest): the fast-forward badge shows only in the mode; a night started in the mode reports a quiet_s measured in ticks, not multiplied by 60.

### 左右开弓（Kick Rally） · 可玩性 4/5 · small
- **玩法**：A beat ticks and the child taps the left pad on one beat and the right pad on the next, alternating. Each on-beat tap fires the duck's real ball-kick skill with that foot (kick_left / kick_right); an off-beat tap sends nothing. The beat speeds up, a missed beat ends the streak, and the score is the duck's own vet-book kick counters, not an app tally.
- **真实地基**：POST /skill {"name":"kick_left"|"kick_right"} (manifest skills ball_kick_left.onnx / ball_kick_right.onnx, 0.5 s, non-chaining); GET /state.life.skills.kick_left / .kick_right (Book::skill increments life.skills[name]); /state.tick (20 ms/tick) for the beat; /state.emotion.name for the duck's reaction.
- **验收**：At a beat of >=0.7 s (so a 0.5 s kick is never still busy), fire four alternating on-beat kicks; poll /state and assert life.skills.kick_left rose by 2 and life.skills.kick_right rose by 2. A tap outside the beat window posts nothing and leaves both counters unchanged.

### 情绪哑剧（Emotion Charades） · 可玩性 4/5 · small
- **玩法**：The duck performs one of its 13 official moods with the real body timeline and the official wav while the game hides the name. The child watches the face and body and picks from 13 mood chips; a right guess earns the duck's official 'yes', a wrong one its official 'no'. A mood refused because another is playing is shown as refused and never scored.
- **真实地基**：POST /skill {"name":"<mood>"} (the 13 loaded official moods: angry, curious, defiant, devastated, excited, impatient, laugh, mmh, mock, no, sad, yes, yes_fast); GET /state.emotion{name,elapsed_s,duration_s}, /state.last_emotion_sound=="emotion:<mood>", /state.life.emotions{<mood>}; GET /collection.
- **验收**：POST /skill {"name":"mock"} -> /state.emotion.name=="mock" with duration_s==2.5; after the timeline ends emotion==null and life.emotions.mock is +1. A mood requested while another plays is refused ('an emotion is already playing') and the UI scores only moods that really started; a correct pick posts /skill {"name":"yes"} -> last_emotion_sound=="emotion:yes".

### 它今天有没有（Did It Happen Today?） · 可玩性 4/5 · small
- **玩法**：A yes/no interview with the duck. The child picks a question the duck can answer from its own published day facts (did we walk today? was it petted today? did it fall today? did it eat today?); the duck really performs the official yes or no emotion, and the card shows the fact and the exact number behind it. Questions without a published day fact are not offered, and every answer is the app reading the real delta. Asking is a real touch, so if it is asleep the app says so instead of faking an answer.
- **真实地基**：GET /state.life{metres,pets,fed,falls,ground_picks} with /state.life.day_started for today's delta; /state.care.asleep; /state.carrying; POST /skill {"name":"yes"|"no"} (both official moods, only offered when GET /collection lists them); /state.life.emotions counts.
- **验收**：Vet-book counter + UI assertion: with today's metres delta 0 and pets delta 1 ask 'did we walk today?', assert the app POSTs /skill no, life.emotions.no rises by 1, and the fact line shows 0.0 m exactly as /state does; ask 'was it petted today?', assert /skill yes and life.emotions.yes +1. UI assertion: a question with no posted day baseline is disabled and posts no emotion; while care.asleep is true the card offers no question.

### 今天的新心情（A New Face Today） · 可玩性 4/5 · small
- **玩法**：A growth ritual built on the existing collection row tap, not a new collection: each day the app reads the duck's own collection and names one official mood it has never shown. The child taps 'teach it'; the duck really performs it, and the core's own note_unlocks writes the first-ever date, which the card shows as 'first time - today'. A mood refused because the duck is paused, sitting or busy is shown as refused and never counted; teaching a sleeping duck is a real waking touch the card discloses.
- **真实地基**：GET /collection {emotions:[{name,count,first_day}],moods:{unlocked,total:13}}; POST /skill {"name":<mood>}; /state.life.emotions; /state.life.unlocks['emotion:<name>'] (Book::note_unlocks runs on the next tick, paused or not); /state.paused; /state.care.asleep; POST /quests/day {"day":...}. The shells' CollectionSheet already offers the per-row practice tap.
- **验收**：Vet-book counter + UI assertion: on a fresh core choose an emotion whose /collection count is 0, POST /quests/day with today's real day, POST /skill {name}, then assert /collection count==1 and first_day==today; XCUITest asserts the row changes from 'not tried yet' to 'first time - today'. With /time {paused:true} the same call is refused, the count stays 0, and the card shows the refusal.

### 心跳同拍（In Time With Its Heart） · 可玩性 4/5 · small
- **玩法**：A rhythm toy: the child taps a big heart to the duck's real heartbeat (care.heart_bpm). heart_bpm is a slow-moving pure reading of mood, hunger, bond and sleep, so the child taps against the current number rather than chasing a moving target. Stay in time for eight beats and the duck answers with its official yes; the tempo is slower when it is calm and fastest when it is hungry and excited. The game is disabled while care.asleep (bpm 46) so a tap can never turn into a real waking touch.
- **真实地基**：GET /state.care.heart_bpm (pure: 72 + 12*mood + 14*hunger - 8*bond, clamped 42..118; asleep exactly 46), /state.care{asleep}; POST /skill {"name":"yes"} (official mood); /state.life.emotions.yes; the existing HeartRipple already renders the same number.
- **验收**：Real /state change + vet-book counter: core unit extends care.rs's heart test (asleep 46, hungry>fed, happy>sad); XCUITest reads /state.care.heart_bpm, taps the 60/bpm interval for 8 beats, asserts the score, and asserts a successful finish posts /skill yes with life.emotions.yes +1. UI assertion: while /state.care.asleep is true the game shows 'asleep' and offers no scoring.

### 倒回它难过之前（Rewind the Sad） · 可玩性 4/5 · medium
- **玩法**：When the duck's mood falls below the core's sad line (-0.35) and it really plays its sad timeline, a rescue card appears. Tapping it seeks back to a checkpoint from before the sad moment; the duck's body, mood and current emotion really return to that tick, and the child then pets or feeds it. The asymmetry is the point: the checkpoint carries care, so the mood genuinely rewinds, but the vet book is not rewound, so the sad count and its emotion:sad event stay. The app says so rather than claiming it forgot.
- **真实地基**：POST /seek {"ticks":N} (restores the newest checkpoint <= tick-N on the next tick and truncates the later timeline); GET /time {checkpoints,oldest,newest}; /state.care{mood,asleep}; /state.emotion; /state.life.events (the sad moment is noted 'emotion:sad'); POST /pet, /feed. Checkpoint carries Care and last_sad, not the Book.
- **验收**：Real /state change + vet-book counter: iOS sim-drive sets care_time_scale=30, waits for /state.care.mood < -0.35 and life.events to contain 'emotion:sad', POSTs /seek {ticks:1200}, polls until /state.tick has gone back, then asserts care.mood > -0.35 while life.events still contains 'emotion:sad'; POSTs /pet and asserts mood rises. UI assertion: the rescue card appears only while the sad state is real and only offers targets within /time.oldest..newest.

### 等它做到才打开（Open When It Happens） · 可玩性 4/5 · medium
- **玩法**：The child writes a short note and seals it against a real milestone that has not happened yet (the next pet, the next 10 metres, the next quest-streak day). The baseline is captured from the duck's own /state counters at seal time; only a rise observed while the app is running opens the seal, showing the real count and the real day. If a restore or a branch moves the counter below the baseline, or a relaunch shows a jump, the app re-baselines instead of claiming a rise; an already-met target is refused at sealing time.
- **真实地基**：/state.life{pets,fed,metres,skills{},emotions{}}, /state.quests.streak, /state.quests.day; POST /pet, /feed, /path, /skill for the real rises. The note and baseline are shell-local (like the dance/alias stores); the counter is the duck's. The moved-counter guard mirrors quests.rs (re-baseline when now < baseline).
- **验收**：Real /state change: with a sealed capsule at pets=N+1 and /state.life.pets=N it stays sealed; a real POST /pet followed by a state read with pets=N+1 opens it once and shows N+1. UI assertion: a relaunch before the pet leaves it sealed; a restored old save that lands past N+1 does not open it (baseline re-set); an already-met target is disabled with 'already done'.

## 推荐下一批（小成本、高分）

1. **声音指挥官**（5/small）：喊出目标（逗笑/逗好奇），孩子对着麦克风拍手或吹口哨，鸭子用自己的分类器听见并用官方反应回答；有同名积木规则时如实显示规则接管。
2. **表情木头人**（5/small）：鸭子演真实情绪，孩子在「动/定」之间跟着玩；回合长度就是情绪时间线自己的 `duration_s`（excited 3.3s、angry 2.5s…）。
3. **左右开弓**（4/small）：节拍上交替左/右踢，计数就是 vetbook 的 `life.skills.kick_left/right`。
4. **情绪哑剧**（4/small）：鸭子演 13 个官方情绪之一，孩子猜；猜对鸭子点头（yes）、猜错摇头（no）。
5. **它今天有没有**（4/small）：用鸭子自己今天的真实账目回答「今天走过吗/被摸过吗/摔过吗」，答案配真实数字。

中等成本的高分项（绿灯行红灯停 5、照我做 5、快进的一天 5）留作再下一批，都需要新的页面或更长的验收链。

## 淘汰（44 条）

| 名字 | 可玩性 | 淘汰理由 |
|---|---|---|
| 起立坐下 | 4 | Near-duplicate of two surviving ideas: Posture Mirror already covers the same sit/stand verbs in a richer four-pose sequence, and Kick Rally already owns the on-beat tap-and-count loop. With only two poses left this is the weaker third beat game. |
| 过来呀 | 5 | Duplicates the shipped path-drawing feature: the app already lets the child draw a route, the duck walks it through the real walk policy, path.active/reached/total reports progress, and completion already plays the duck's 'excited'. The arrival greet and score are a cosmetic layer on an existing feature. |
| 走满一米 | 4 | Duplicates the shipped daily walk quest (quests.rs BLUEPRINTS 'walk' / Metric::Metres target 2.0), which already measures the life.metres delta from the day baseline and celebrates with the duck's official 'excited'. The progress bar is UI, not a new mechanic. |
| 学我走路 | 4 | The demo leaves no ghost trail, and /state.pose is explicitly the duck's own drifting odometry estimate, not ground truth; reproducing it to 0.10 m / 0.20 rad with an open-loop stick is not achievable for a child, so the acceptance is scriptable but the game is not fair. It is also a weaker walking mirror of Posture Mirror. |
| 捡捡放放 | 4 | Duplicates two shipped features joined together: the blocks ground_pick prop (a real ground_pick on the stage) plus path walking. The three lit spots and the greeting are cosmetic; no new verb or loop is added. |
| 抱我回家 | 4 | There is no published ground truth to judge the landing: /state.pose is deliberately the duck's own odometry estimate (not the simulator's trunk position), and /carry is a spring-damper force hold with the walking networks paused, so pose.position does not honestly track a carried duck. 'Landed on the pad' cannot be scripted from /state, and the candidate's own warning about life.pickups is correct. |
| 张嘴打拍 | 3 | Its premise is false: on the shipped walking and roller scenes has_mouth is false, so set_mouth refuses 'this scene has no mouth joint' and /state.proprioception.mouth never moves. The beak is the shell's local cosmetic, not a state change; the game would score a field that never changes - the same flaw as the already-dropped Beak Sync. |
| 滚一圈接力 | 3 | Near-duplicate: the same on-beat tap-and-count loop as Kick Rally, and the roulade answer is already covered by Posture Mirror's roulade pose and the stage's hoop roulade prop. Only the verb differs. |
| 跳房子 | 3 | No jump verb exists in the manifest or the Intent enum; a hop would be a fake animation, not the duck's body. |
| 接球 | 4 | Needs a grab-hands verb and real ball physics; the duck has neither, and props are targets, not simulated bodies. |
| 推积木 | 3 | POST /push sets the duck's own trunk velocity, not a prop's; making it move the block would invent physics the core does not have. |
| 听声辨心情 | 4 | Near-duplicate of Emotion Charades: the same guess-the-mood loop with the same 13 chips and the same yes/no scoring; only the stimulus is voice-only. One mood-guessing game is enough. |
| 拍手回音 | 4 | Duplicates the shipped Echo game (EchoGame: the duck sings a growing sequence and the child answers the same sequence in order). Swapping screen pads for claps does not make a new game loop. |
| 心情灯 | 4 | A display/customisation with no play loop: the child assigns colours once and then watches a field that is already drawn on screen. There is no goal, score or repeated action, so it fails the pure-display test (c). |
| 灯光接龙 | 4 | Duplicates the shipped Echo game: the same growing-sequence memory loop, with colour pads instead of sound pads. The colour output does not change the mechanic. |
| 灯光积木 | 4 | Already ships on iOS: RulesSheet.swift lists kinds ['skill','emotion','sound','light'], valueOptions for light returns the seven colours plus 'off', and saveRules posts it; the core accepts light and set_flashlight runs it. The real remaining work is Android parity (Companion.kt RULE_ACTIONS omits light) and the iOS import filter (DuckController.ruleActions omits light) - a parity/fix task, not a new play idea (d). |
| 再摸一会儿 | 3 | A stopwatch laid over the existing sound-petting sense (pet_detect / sound_pets / coo); there is no game loop beyond keeping a timer alive - no goal, opponent or decision. It fails the keep-only-replayable test. |
| 灯光琴 | 4 | Its premise is false and it duplicates the shipped Dance Studio: both ends already record and replay light steps (iOS DanceSheet quickSteps light yellow/off; Android DanceSheet 'light' -> onLight), alongside skill and sound steps. Light Piano is Dance Studio with a fixed pad layout. |
| 心情诊所 | 4 | Duplicates the shipped care feature: pet/feed/mood and the sad-at-mood<-0.35 timeline are the care layer's own mechanics, already surfaced in the Instrument care card. The clinic frame adds a label, not a new loop (d). |
| 对口型 | 3 | The shipped walking and roller scenes have no mouth actuator (has_mouth=false; mujoco asserts it), so POST /mouth is refused with 'this scene has no mouth joint'. The moving beak is the shell's local cosmetic, not a state change - the game would score a field that never moves. |
| 悄悄话与大声 | 2 | Needs an invented quiet/volume classification: the duck has one loud-startle threshold (rms > 0.35 -> alarm) and no whisper sense, so a graded loud/quiet score would be fake feedback. |
| 情绪旋律 | 3 | Needs new audio: the synth has seven fixed-voice tags with no pitch control, so a melody game would require new sounds or a new synthesis path - out of scope. |
| 两个明天 | 4 | A presentation of the shipped fork/branch time machine: the shells already expose fork and push-and-compare (ABSheet, DuckController.loadPolicyOnBranchB), and the only new part is reading two care numbers. It adds no new verb or play loop beyond the existing feature (d). |
| 稳稳两米 | 4 | Duplicates path drawing plus the shipped daily metres quest. The child draws and then watches; the no-fall line is a challenge rule bolted onto an existing feature, not a new play loop (d). |
| 第一次的纪念日 | 3 | A display/ritual, not a game: a date card with one re-perform tap and no repeated loop. The collection already renders empty first_day as unknown, so the honest part is existing; the anniversary is a label. |
| 超过昨天的自己 | 3 | Duplicates the shipped daily walk quest (BLUEPRINTS 'walk' / Metric::Metres target 2.0), which already measures the life.metres delta from the duck's own baseline and celebrates with excited. Only the target's source changes. |
| 心情曲线 | 2 | Invents numbers: /state publishes only the current care reading and /days has no mood/hunger history, so any whole-day curve would be interpolated fiction. |
| 破壳日 | 3 | No first-launch date exists in /state or the vet book - life.boots is a count and unlocks may be '' - so a hatch day would celebrate a date the duck never had. |
| 回到今早 | 4 | The timeline holds only 3000 checkpoints (~60 s) and a restore discards the future; 'this morning' would be a fabricated snapshot. |
| 补签连续天数 | 4 | Skips a real day: the quest streak resets on a missing previous day and past-day activity cannot be created; a repaired streak would be a lie. Keep the honest reset and show the gap. |
| 情绪接力 | 4 | Needs a second device: the whole relay depends on duck B being paired on the LAN, so it is not playable by one child with one duck (b). It also only recombines the shipped rules + gift features. |
| 合唱接龙 | 4 | Needs at least a second paired duck on the LAN to be playable; the chorus feature already ships, and a follow-the-leader framing does not make it single-device (b). |
| 零食解谜 | 3 | Needs a second paired device to receive the treat; the gift path already ships, and without a peer there is no game (b). |
| 晨间定时仪式 | 3 | Already ships: both block editors offer the 'timer' trigger (iOS triggers list, Android RULE_TRIGGERS) and runtime.rs fires timer rules only when !care.asleep. This is the existing rules feature with a ritual label, not a new play idea (d). |
| AI 故事接龙 | 4 | Its trigger is a gift or neighbour event, which only happens with a paired peer on a second device; with one duck there is nothing to narrate. It is also AI narration rather than a child-played loop (b). |
| 语音对鸭 | 4 | Needs two paired ducks: the /heard mapping on A and the gift to B cannot be played on one device (b). |
| 屏幕阅读器社交通道 | 4 | The gift and chorus it exposes need a paired peer on a second device to land, so it is not single-player playable (b); it is also an accessibility parity path over shipped features, not a play loop. |
| DKPD 遥控竞速 | 4 | Needs a second phone as the DKPD pad sender, so it is not playable on one device (b); it is also a remote-control tech demo rather than a child game on the duck's own verbs. |
| robot.* 远端指挥 | 3 | Needs an external client/codebase to drive the duck (b), and a JSON-RPC scripting interface is not a child-playable loop on one device. |
| 口哨合奏 | 4 | Needs a paired duck for the chorus half, so it is not playable on one device (b); it recombines the shipped rules + chorus features. |
| AI 看家 | 3 | Needs peers on the LAN (a second device) for there to be anything to narrate (b); it is ambient AI narration, not a child-played loop. |
| 配对密码门 | 3 | Needs two ducks to pair, so it is not playable on one device (b); the pairing gate already ships as the trust boundary for every shared action. |
| AI 自主改规则 | 2 | The AI decision channel only exposes skill/sound/sit/stand/stop/look/move + say, and Intent::SetRules is not on it, so the AI cannot autonomously edit the child's rules. |
| 夜间定时仪式 | 2 | Timer rules and autonomous chorus are gated by !care.asleep (runtime.rs), so a night-time ritual cannot fire, and care.budget_s expiry puts the duck to sleep. |

淘汰的类别：需要第二台设备/外部客户端、需要鸭子没有的动词（跳、抓取手、张嘴）、与已交付功能重复（画路、回声、编舞、规则灯、每日任务）、纯展示无回路、以及会伪造数字的（心情曲线、破壳日、补签连续天数）。
