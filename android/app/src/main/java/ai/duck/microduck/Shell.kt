package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.changedToUp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import kotlin.math.hypot
import kotlin.math.min
import kotlin.math.roundToInt
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The fused shell, the same shape as the iOS one: a live duck first, an
 * instrument underneath, a viewfinder on demand.
 *
 * - **Live**: drag on the scene to drive. Press ahead to walk forward, either
 *   side to turn; release to stop. Tap the sparkle for the skill ring.
 * - **Instrument**: the Time button raises the sheet — the scrubber over the
 *   ticks the core kept, freeze, step, rewind, fork, speed, the numbers, the
 *   sensors and the fingerprint.
 * - **Director**: a letterboxed viewfinder with the tick in the corner.
 */
@Composable
fun Shell(
    core: DuckCore?,
    skills: List<String>,
    input: DuckInput,
    surface: Surface,
    onSurface: (Surface) -> Unit,
    quack: (() -> Unit)? = null,
    onFeed: (() -> Unit)? = null,
    onWheee: ((Boolean) -> Unit)? = null,
    onPhoto: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    var showRing by remember { mutableStateOf(false) }
    var showSheet by remember { mutableStateOf(false) }
    var showRules by remember { mutableStateOf(false) }
    var showPeers by remember { mutableStateOf(false) }
    var peers by remember { mutableStateOf(PeersUi()) }
    var quests by remember { mutableStateOf(QuestBoardUi()) }
    // The user's own model: config, last answer and the loop's guard.
    var showAI by remember { mutableStateOf(false) }
    var showDance by remember { mutableStateOf(false) }
    var showPostcards by remember { mutableStateOf(false) }
    var showDex by remember { mutableStateOf(false) }
    var showDiary by remember { mutableStateOf(false) }
    // The More drawer and its daily-goals sheet (B4a, portrait).
    var showMore by remember { mutableStateOf(false) }
    var showBig by remember { mutableStateOf(false) }
    var showQuests by remember { mutableStateOf(false) }
    // The drawer's night row follows /state.night, so it can offer
    // good night or good morning honestly.
    var nightActive by remember { mutableStateOf(false) }
    var dex by remember { mutableStateOf(emptyList<DexEntryUi>()) }
    val dance = remember(context) { DanceStudio(context) }
    var danceShow by remember { mutableStateOf(0) }
    var aiConfig by remember { mutableStateOf(AIStore.load(context)) }
    var aiStatus by remember { mutableStateOf("") }
    var aiDecision by remember { mutableStateOf("") }
    var aiError by remember { mutableStateOf<String?>(null) }
    var aiThinking by remember { mutableStateOf(false) }
    var lastAIThink by remember { mutableStateOf(0L) }
    val aiScope = rememberCoroutineScope()
    // The system voice for the duck's answers (`say`): the same
    // possession-mode channel the diary reads with, rendering through the
    // duck's own playback session. The beak follows through /mouth.
    val speaker = remember(context) {
        Speaker(context) { speaking ->
            postDuckCommand(
                "/mouth",
                org.json.JSONObject().put("open", if (speaking) 1.0 else 0.0).toString(),
            )
        }
    }
    // The one-line pill above the rail: care by default, a room sound, a
    // block or the model's decision for a moment.
    var soundLine by remember { mutableStateOf<String?>(null) }

    // One decision: fetch what the duck sees, ask the endpoint, execute the
    // validated answer. The core never talks to the network.
    val thinkAI: () -> Unit = {
        if (!aiThinking) {
            val config = aiConfig
            aiThinking = true
            aiError = null
            aiStatus = Copy.aiThinking
            aiScope.launch {
                val state = withContext(Dispatchers.IO) { fetchRawState() }
                val skills = withContext(Dispatchers.IO) { fetchSkillNames() }
                val heard = withContext(Dispatchers.IO) { fetchLastHeard() } ?: ""
                val result: Pair<AIDecision?, String?> = withContext(Dispatchers.IO) {
                    if (state == null) null to "the duck did not answer /state"
                    else AIBrain.think(config, state, skills, heard)
                }
                val decision = result.first
                val error = result.second
                if (decision != null) {
                    // The duck's answer first: `say` is spoken in the shell's
                    // language, the body follows the action. The speaker is
                    // the same possession-mode channel the diary reads with;
                    // it renders through the duck's own playback session.
                    if (decision.say.isNotEmpty()) {
                        withContext(Dispatchers.Main) {
                            speaker.speak(
                                decision.say,
                                if (Copy.language == Copy.Language.Chinese) {
                                    java.util.Locale.SIMPLIFIED_CHINESE
                                } else {
                                    java.util.Locale.US
                                },
                            )
                        }
                    }
                    aiDecision = executeAIDecision(decision)
                    soundLine = aiDecision
                    aiStatus = if (config.enabled) Copy.aiOn else Copy.aiOff
                }
                if (error != null) {
                    aiError = error
                    aiStatus = Copy.aiOffline
                }
                aiThinking = false
            }
        }
    }
    var carrying by remember { mutableStateOf(false) }
    // The pad's D-pad modes, reduced to what a phone can do: drive, look,
    // crouch/lean the standing body, and draw a path.
    var stick by remember { mutableStateOf("drive") }
    // Path mode: the stroke the child drew, in stage pixels, and the stage's
    // own size (the mapping to metres is the stage's short side spans 2.2 m).
    var pathPoints by remember { mutableStateOf(emptyList<Offset>()) }
    var stagePx by remember { mutableStateOf(IntSize.Zero) }
    // What the core says about the walk it is running, from the top-level
    // "path" object of /state: active, the reached/total counts and the
    // accepted body-frame waypoints as posted.
    var pathActive by remember { mutableStateOf(false) }
    var pathProgress by remember { mutableStateOf(0 to 0) }
    var pathActivePoints by remember { mutableStateOf(emptyList<List<Double>>()) }
    // The draw-time mapping, remembered when the stroke is posted so the walk
    // can be re-projected onto the stage while it runs.
    var pathOrigin by remember { mutableStateOf(Offset.Zero) }
    var pathMpp by remember { mutableStateOf(0f) }
    // A Go is on the wire; a second tap must not queue the walk twice.
    var pathPosting by remember { mutableStateOf(false) }
    // A Stop press makes the walk's end a stop, not a finish.
    var pathCancelRequested by remember { mutableStateOf(false) }
    val pathScope = rememberCoroutineScope()
    // A stroke feeds the collector: about 0.05 m between kept points — the
    // floor the stage's own scale gives — and 64 points at most, the cap the
    // core walks.
    val onPathDraw: (Offset) -> Unit = { point ->
        val last = pathPoints.lastOrNull()
        val spacing = 0.05f * min(stagePx.width, stagePx.height).coerceAtLeast(1) / 2.2f
        if (last == null || hypot(point.x - last.x, point.y - last.y) >= spacing) {
            if (pathPoints.size < 64) pathPoints = pathPoints + point
        }
    }
    // Walk it: the stroke becomes metres in the duck's body frame at the
    // stroke's start and one POST /path. A refusal (caps, server down) leaves
    // the drawing up so the child can clear it or try again.
    val goPath: () -> Unit = {
        val metres = pathMetres(pathPoints, stagePx)
        if (metres != null && pathWalkable(metres) && !pathPosting) {
            pathPosting = true
            // Remember the draw-time mapping for the walk's re-projection.
            pathOrigin = pathPoints.first()
            pathMpp = 2.2f / min(stagePx.width, stagePx.height).coerceAtLeast(1)
            pathScope.launch {
                val queued = withContext(Dispatchers.IO) { postPath(metres) }
                pathPosting = false
                // The accepted walk renders from /state.path; the drawn
                // stroke only survives a refusal.
                if (queued) pathPoints = emptyList()
            }
        }
    }
    // Flipping this re-renders every Copy string in the shell.
    var languageTick by remember { mutableStateOf(0) }
    // The face's flashlight: 0 is off, 1..7 are robotd's seven colours. The
    // top-bar chip and the sheet's chip both write here, so the two never
    // disagree about whether the light is on.
    var flashlightState by remember { mutableStateOf(0) }
    // The one menu that replaced the row of chips.
    var menuOpen by remember { mutableStateOf(false) }
    // The last skill the ring sent to the core. A skill's own label is brief;
    // this verdict outlives it, so the top bar can say what just ran.
    var lastSkill by remember { mutableStateOf("") }
    // The core's own state word from /state.label. The raw string is the
    // protocol's; the top bar maps it through Copy.behaviourOf before it is
    // shown, and the instrument keeps the raw one.
    var behaviourRaw by remember { mutableStateOf("") }
    // One tap flips the light, exactly like `robot.flashlight` on the duck.
    // The core takes its own lock, so this stays off the render thread's
    // critical path.
    val toggleFlashlight: () -> Unit = {
        val duck = core
        if (duck != null) flashlightState = duck.flashlight(true, false, "white")
    }

    // First-launch hint: one SharedPreferences flag, shown once and never
    // again. The coroutine runs on the Compose dispatcher, so the 4 s delay
    // and the flip back happen on the main thread.
    val hintPrefs = remember(context) {
        context.getSharedPreferences("duck", Context.MODE_PRIVATE)
    }
    var hintDismiss by remember { mutableStateOf(!hintPrefs.getBoolean("hintShown", false)) }
    LaunchedEffect(hintDismiss) {
        if (hintDismiss) {
            hintPrefs.edit().putBoolean("hintShown", true).apply()
            delay(6000)
            hintDismiss = false
        }
    }

    // The companion layer: the pill reads the core's own numbers once a
    // second. The state is kept as numbers; the words are computed at
    // composition, so a language switch is immediate.
    var careUi by remember { mutableStateOf<CareUi?>(null) }
    LaunchedEffect(core) {
        val duck = core ?: return@LaunchedEffect
        var lastSoundTick = -1
        var lastRuleTick = -1
        var lastPeerTick = -1
        var lastGiftTick = -1
        var lastChorusTick = -1
        // The night is a state, not an event, so the edge is the guard: the
        // inactive object is zeroed, and only the last active publish knows
        // how long the quiet lasted.
        var lastNightActive = false
        var lastNightQuiet = 0.0
        while (true) {
            // One /state for the whole shell: no extra fetch can race the
            // snapshot it persists.
            val stateText = withContext(Dispatchers.IO) { fetchRawState() }
            val careNow = stateText?.let(::parseCare)
            if (careNow != null) careUi = careNow
            val label = stateText?.let(::parseLabel)
            if (label != null) behaviourRaw = label
            val nightNow = stateText?.let(::parseNight)
            if (nightNow != null) nightActive = nightNow.active
            val event = stateText?.let(::parseSoundEvent)
            if (event != null && event.kind.isNotEmpty() && event.tick != lastSoundTick) {
                lastSoundTick = event.tick
                soundLine = Copy.heardSound(event.kind)
            }
            val rule = stateText?.let(::parseRuleEvent)
            if (rule != null && rule.tick != lastRuleTick) {
                lastRuleTick = rule.tick
                soundLine = rule.text
            }
            val peersNow = stateText?.let(::parsePeers)
            if (peersNow != null) peers = peersNow
            val questsNow = stateText?.let(::parseQuestsFromState)
            if (questsNow != null && questsNow.quests.isNotEmpty()) quests = questsNow
            val dexNow = stateText?.let {
                DexStore.build(context, it, skills, Copy.language == Copy.Language.Chinese)
            }
            if (dexNow != null && dexNow.isNotEmpty()) dex = dexNow
            val peerEvent = stateText?.let(::parsePeerEvent)
            if (peerEvent != null && peerEvent.tick != lastPeerTick) {
                lastPeerTick = peerEvent.tick
                soundLine = peerEvent.text
            }
            // A neighbour's gift lands on a published tick; the guard keeps
            // the same gift from raising its pill on every poll.
            val giftEvent = stateText?.let(::parseGiftEvent)
            if (giftEvent != null && giftEvent.tick != lastGiftTick) {
                lastGiftTick = giftEvent.tick
                soundLine = giftEvent.text
            }
            // The night: a chosen sleep, ended by the morning verb or by a
            // real hand. Its end is the active->inactive edge, and the pill
            // uses the quiet the last active publish carried.
            if (nightNow != null) {
                if (lastNightActive && !nightNow.active && lastNightQuiet > 0.0) {
                    soundLine = Copy.goodMorningLine((lastNightQuiet / 60.0).roundToInt())
                }
                lastNightActive = nightNow.active
                if (nightNow.active) lastNightQuiet = nightNow.quietSeconds
            }
            // A chorus is a shared moment: the pill names the room size on
            // the inactive->active edge, once per song (started_tick guard).
            val chorusNow = stateText?.let(::parseChorus)
            if (chorusNow != null) {
                if (chorusNow.active && chorusNow.startedTick != lastChorusTick) {
                    lastChorusTick = chorusNow.startedTick
                    soundLine = Copy.chorusLine(chorusNow.peers)
                }
            }
            // The path runner: the same publish carries whether a walk is on,
            // the progress through it and the waypoints as accepted.
            val pathNow = stateText?.let(::parsePathFromState) ?: PathUi()
            // The duck's own excited mood is the real celebration; the pill
            // names it when a walk ended without the child's Stop. The end
            // and the clear land on the same publish, so the transition —
            // not the final count — is what says "finished".
            if (!pathNow.active && pathActive && !pathCancelRequested) {
                soundLine = Copy.pathDone
            }
            if (!pathNow.active) pathCancelRequested = false
            pathProgress = pathNow.reached to pathNow.total
            pathActive = pathNow.active
            pathActivePoints = pathNow.points
            val now = System.currentTimeMillis()
            if (aiConfig.enabled && !aiThinking &&
                now - lastAIThink >= maxOf(10, aiConfig.interval) * 1000L
            ) {
                lastAIThink = now
                thinkAI()
            }
            delay(1500)
        }
    }

    // A clap or a whistle stays on screen for a moment, then the care line
    // has the slot back.
    LaunchedEffect(soundLine) {
        if (soundLine != null) {
            delay(4000)
            soundLine = null
        }
    }

    // The microphone: a toggle in the instrument sheet. Off by default; the
    // feed, the permission launcher and the summary live here so the sheet has
    // one owner. The mic is the room, never the duck's senses. The state is
    // declared before the launcher because the launcher's callback reads it.
    var micState by remember { mutableStateOf(MicState.Off) }
    var micFeed by remember { mutableStateOf<MicFeed?>(null) }
    val audioLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted ->
        if (granted) {
            // The feed is created here, not in onToggleMic: that path only
            // launches the prompt, so by the time the callback runs micFeed is
            // still null and starting it here is the one place it can begin.
            micFeed = MicFeed().also { it.start() }
            micState = MicState.On
        } else {
            micState = MicState.Denied
        }
    }
    // Spoken commands: a second, independent channel. It is a command path
    // (the core maps 翻滚/坐下/开心 to skills), never a sense, and it is off
    // until the toggle is tapped. The holder is created here on the main
    // thread because SpeechRecognizer and its callbacks must live on the main
    // Looper; the sheet reads its Compose state directly.
    val speechFeed = remember(context) { SpeechFeed(context) }
    // A fresh transcript asks the model for one decision immediately: a
    // conversation cannot wait out the periodic think. Same 10 s floor.
    LaunchedEffect(Unit) {
        speechFeed.onUtterance = { _ ->
            val now = System.currentTimeMillis()
            if (aiConfig.enabled && !aiThinking && now - lastAIThink >= 10_000L) {
                lastAIThink = now
                thinkAI()
            }
        }
    }
    val voiceLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted ->
        if (granted) {
            speechFeed.start()
        } else {
            speechFeed.markDenied()
        }
    }
    // Stop both feeds only when the shell leaves composition (the activity is
    // going away). Toggling the mic off stops it explicitly in onToggleMic; a
    // DisposableEffect keyed on micState would fire its onDispose the instant
    // micState flips to On and kill the feed right after it started.
    DisposableEffect(Unit) {
        onDispose {
            micFeed?.stop()
            speechFeed.stop()
        }
    }
    // The instrument is the lab, the live stage is the room. The renderer
    // cannot see a Compose sheet, so the shell tells it which surface is up
    // and the floor switches to the cold checkerboard there (STAGE_LOOK §3).
    LaunchedEffect(showSheet) { input.instrumentOpen = showSheet }

    val _unused = languageTick
    BoxWithConstraints(modifier.fillMaxSize()) {
        // Portrait is the product; landscape is the two-hand instrument and
        // demo layout. A width under 600 keeps phones (and a narrow split
        // view) in the one-thumb shell; wider surfaces keep the wide chrome.
        val portrait = maxWidth < 600.dp && maxHeight > maxWidth
        // A gesture interrupted by a rotation never reports its release, and
        // the core is not recreated. Hand the duck back neutral once whenever
        // the shell swaps between the portrait and wide compositions.
        LaunchedEffect(portrait) {
            input.vx = 0.0
            input.wz = 0.0
            core?.setHead(0.0, 0.0, 0.0, 0.0)
            core?.releaseBody()
            // A draft is measured in the old screen space, which the new
            // orientation no longer means.
            pathPoints = emptyList()
        }
        if (portrait) {
            val duck = core
            if (duck != null) {
                // The viewfinder's frame is scene, not chrome: it is drawn
                // under the column so the rail and deck stay legible.
                if (surface == Surface.Director) {
                    DirectorOverlay(duck, Modifier.fillMaxSize())
                }
                Column(
                    Modifier
                        .fillMaxSize()
                        .windowInsetsPadding(WindowInsets.safeDrawing),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    PortraitTitleBar(
                        core = duck,
                        carrying = carrying,
                        lastSkill = lastSkill,
                        behaviourRaw = behaviourRaw,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                    // The stage. Steering is translation-based from wherever
                    // the thumb lands: a hidden centre-origin joystick makes
                    // a resting thumb command a reversal. In path mode the
                    // same stage draws the walk instead.
                    Box(
                        Modifier
                            .weight(1f)
                            .fillMaxWidth()
                            .onSizeChanged { stagePx = it },
                    ) {
                        Spacer(
                            Modifier
                                .fillMaxSize()
                                .portraitStageGesture(
                                    core = duck,
                                    input = input,
                                    carrying = carrying,
                                    stick = stick,
                                    pathActive = pathActive,
                                    onPathDraw = onPathDraw,
                                    ringOpen = { showRing },
                                    onLongPress = { showRing = true },
                                ),
                        )
                        // The stroke is scene, not chrome: it draws over the
                        // stage and never takes a touch from the gesture.
                        if (stick == "path" || pathActive) {
                            PathOverlay(
                                drawn = pathPoints,
                                active = pathActive,
                                accepted = pathActivePoints,
                                reached = pathProgress.first,
                                origin = pathOrigin,
                                mpp = pathMpp,
                                modifier = Modifier.matchParentSize(),
                            )
                        }
                    }
                    // The one-time gesture hint lives in the column, above
                    // the rail: no magic bottom offset to drift onto a button.
                    AnimatedVisibility(visible = hintDismiss) {
                        Text(
                            Copy.firstLaunchHint,
                            color = Color.White.copy(alpha = 0.7f),
                            fontSize = 12.sp,
                            modifier = Modifier.padding(bottom = 6.dp),
                        )
                    }
                    // Once the hint is gone, the same slot answers "how is it
                    // doing" — or, for a moment, what a clap or a whistle just
                    // did. The care words are derived here, not stored, so a
                    // language switch reaches the pill on the same frame.
                    val mood = careUi?.let { Copy.moodLine(it.hunger, it.mood, it.asleep) }
                    val pill = soundLine ?: mood ?: ""
                    if (!hintDismiss && pill.isNotEmpty()) {
                        Text(
                            pill,
                            color = Color.White.copy(alpha = 0.72f),
                            fontSize = 11.sp,
                            modifier = Modifier
                                .padding(bottom = 6.dp)
                                .testTag(if (soundLine != null) "sound-pill" else "care-pill"),
                        )
                    }
                    // Path mode's one row: draw, walk it, clear — or the
                    // progress and the stop chip while the duck walks.
                    PathBar(
                        drawing = stick == "path" && !pathActive,
                        canGo = !pathPosting && pathWalkable(
                            pathMetres(pathPoints, stagePx) ?: emptyList(),
                        ),
                        active = pathActive,
                        reached = pathProgress.first,
                        total = pathProgress.second,
                        onGo = goPath,
                        onClear = { pathPoints = emptyList() },
                        onCancel = {
                            pathCancelRequested = true
                            postPathCancel()
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp),
                    )
                    // The observation HUD: a few REAL numbers the stage is
                    // running with - the same /state the instrument sheet
                    // reads, drawn over the duck instead of behind a sheet.
                    if (input.hudOn && core != null) {
                        Text(
                            "tick ${core.tickIndex()} · ${core.mode()} · mouth %.2f".format(
                                core.status().getOrNull(12) ?: 0.0,
                            ),
                            color = Color.White.copy(alpha = 0.85f),
                            fontSize = 11.sp,
                            fontFamily = FontFamily.Monospace,
                            modifier = Modifier
                                .padding(horizontal = 16.dp)
                                .background(Color.Black.copy(alpha = 0.45f), RoundedCornerShape(8.dp))
                                .padding(horizontal = 8.dp, vertical = 4.dp)
                                .testTag("hud-overlay"),
                        )
                    }
                    // The second player: the right edge belongs to them -
                    // three verbs at arm's length while player one drives.
                    if (input.p2On && core != null) {
                        Row(
                            Modifier
                                .align(Alignment.CenterEnd)
                                .padding(end = 8.dp),
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            ButtonChip(Copy.bigButtonAction(0), "p2-pet") { postDuckCommand("/pet") }
                            ButtonChip(Copy.bigButtonAction(2), "p2-quack") { quack?.invoke() }
                            ButtonChip(Copy.bigButtonAction(3), "p2-sit") { core.sit() }
                        }
                    }
                    PortraitTimeRail(
                        core = duck,
                        surface = surface,
                        onSurface = onSurface,
                        onOpenSheet = { showSheet = true },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 8.dp),
                    )
                    ThumbDeck(
                        core = duck,
                        carrying = carrying,
                        onCarrying = { carrying = it },
                        quack = quack,
                        onFeed = {
                            dance.record("feed", label = Copy.feed)
                            onFeed?.invoke()
                        },
                        onWheee = onWheee,
                        onSkills = { showRing = true },
                        onMore = { showMore = true },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 8.dp, vertical = 8.dp),
                    )
                }
            }
        } else {
            // Driving: a drag on the scene becomes the twist the real pad sends.
            // While the duck is held, the same drag moves the hand — a carry is an
            // external force, and the pickup classifier decides what the policy
            // does about it. In path mode the drag draws instead: the same
            // collector the portrait stage feeds, one behaviour in both
            // orientations (a stroke is a walk). The wide layout has no tap
            // detector, so a waypoint only ever comes from a stroke here.
            Box(
                Modifier
                    .fillMaxSize()
                    .onSizeChanged { stagePx = it }
                    .pointerInput(core, carrying, stick, pathActive) {
                        val duck = core ?: return@pointerInput
                        var start = Offset.Zero
                        detectDragGestures(
                            onDragStart = {
                                start = it
                                if (stick == "path" && !pathActive && !carrying && !showRing) {
                                    onPathDraw(it)
                                }
                            },
                            onDrag = { change, _ ->
                                // The skill ring is up: no carry, no drive
                                // while it is.
                                if (showRing) {
                                    input.vx = 0.0
                                    input.wz = 0.0
                                    return@detectDragGestures
                                }
                                change.consume()
                                val width = size.width.toFloat().coerceAtLeast(1f)
                                val height = size.height.toFloat().coerceAtLeast(1f)
                                if (carrying) {
                                    val dx = (change.position.x - start.x) / (width * 0.5f)
                                    val dy = (change.position.y - start.y) / (height * 0.5f)
                                    duck.carry(dx * 0.35, 0.0, -dy * 0.30, true)
                                    return@detectDragGestures
                                }
                                val dx = (change.position.x - width * 0.5f) / (width * 0.5f)
                                val dy = (change.position.y - height * 0.52f) / (height * 0.5f)
                                if (stick == "path" && !pathActive) {
                                    // Drawing, not steering. While the duck
                                    // walks the path the drag is an external
                                    // twist again — the branch below runs, and
                                    // the core hands the walk back to the
                                    // child's hand.
                                    onPathDraw(change.position)
                                } else when (stick) {
                                    "head" -> duck.setHead(
                                        0.0,
                                        dy.toDouble() * 0.6,
                                        dx.toDouble() * 1.2,
                                        0.0,
                                    )
                                    "body" -> duck.setBody(-dy.toDouble() * 0.05, dx.toDouble() * 0.12, 0.0)
                                    else -> {
                                        val forward = -dy
                                        input.vx = deadzone(forward.toFloat()).toDouble() * 0.4
                                        input.wz = -deadzone(dx.toFloat()).toDouble() * 1.0
                                    }
                                }
                            },
                            onDragEnd = {
                                if (!carrying) {
                                    input.vx = 0.0
                                    input.wz = 0.0
                                    // Leaving a mode puts back what it moved. The
                                    // body release is a different call: setting
                                    // (0,0,0) starts holding nominal, and a held
                                    // pose zeroes the twist for good.
                                    duck.setHead(0.0, 0.0, 0.0, 0.0)
                                    duck.releaseBody()
                                }
                            },
                            onDragCancel = {
                                if (!carrying) {
                                    input.vx = 0.0
                                    input.wz = 0.0
                                    duck.setHead(0.0, 0.0, 0.0, 0.0)
                                    duck.releaseBody()
                                }
                            },
                        )
                    }
                    // The same 0.35 s hold as portrait: hold still on the scene
                    // and the ring comes up. A drag cancels it (the drag
                    // detector consumes movement), so driving never opens it.
                    .sceneLongPress(enabled = core != null) { showRing = true },
            )

            // The stroke the wide layout draws, and the walk it becomes —
            // scene, not chrome, so it never takes a touch from the drag.
            if (stick == "path" || pathActive) {
                PathOverlay(
                    drawn = pathPoints,
                    active = pathActive,
                    accepted = pathActivePoints,
                    reached = pathProgress.first,
                    origin = pathOrigin,
                    mpp = pathMpp,
                    modifier = Modifier.fillMaxSize(),
                )
            }

            if (surface == Surface.Director) {
                DirectorOverlay(core, Modifier.fillMaxSize())
            }

            // Path mode's row, the same one the portrait rail hosts, under the
            // top bar so the thumb clusters in the corners keep their space.
            PathBar(
                drawing = stick == "path" && !pathActive,
                canGo = !pathPosting && pathWalkable(
                    pathMetres(pathPoints, stagePx) ?: emptyList(),
                ),
                active = pathActive,
                reached = pathProgress.first,
                total = pathProgress.second,
                onGo = goPath,
                onClear = { pathPoints = emptyList() },
                onCancel = {
                    pathCancelRequested = true
                    postPathCancel()
                },
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .systemBarsPadding()
                    .padding(top = 76.dp, start = 12.dp, end = 12.dp)
                    .fillMaxWidth(),
            )

            // Top bar: the duck's state, and ONE menu. Six text chips in a row
            // was a desktop panel crammed onto a phone: on a small landscape
            // screen they ran off the edge and nothing was legible. The stick
            // modes, the surface, the language and the flashlight now live in a
            // menu that a thumb can open.
            TopBar(
                core = core,
                carrying = carrying,
                lastSkill = lastSkill,
                behaviourRaw = behaviourRaw,
                menuOpen = menuOpen,
                onMenuOpen = { menuOpen = it },
                onStick = { stick = it },
                onSurface = onSurface,
                onFlashlight = toggleFlashlight,
                onRules = { showRules = true },
                onDance = { showDance = true },
                onPeers = { showPeers = true },
                onAI = { showAI = true },
                onLanguage = {
                    Copy.language = if (Copy.language == Copy.Language.English) {
                        Copy.Language.Chinese
                    } else {
                        Copy.Language.English
                    }
                    languageTick += 1
                    soundLine = null
                },
                modifier = Modifier
                    .align(Alignment.TopStart)
                    // Fullscreen theme: keep the menu out of the display cutout.
                    .systemBarsPadding()
                    .padding(12.dp),
            )

        // Bottom bar: the duck itself is the primary control, so the one
        // action that needs the thumb gets it — the rest are one row of
        // small targets on the other side.
        Row(
            Modifier
                .align(Alignment.BottomStart)
                .systemBarsPadding()
                .padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            RoundChip(Copy.time, size = 48.dp, testTag = "duck-time") { showSheet = true }
            RoundChip(Copy.skills, size = 48.dp, testTag = "duck-skills") { showRing = true }
            core?.let { duck ->
                WheeeChip(
                    Copy.quack,
                    testTag = "duck-quack",
                    onTap = { quack?.invoke() },
                    onHold = { onWheee?.invoke(it) },
                )
            }
            // A treat on the floor: the official ground-pick peck, counted
            // in the life record. POST /feed; the loop refuses when busy.
            RoundChip(Copy.feed, size = 48.dp, testTag = "duck-feed") { onFeed?.invoke() }
        }
        // Pick the duck up or put it down: the biggest target on screen, in
        // the corner a thumb reaches first.
        core?.let { duck ->
            Box(
                Modifier
                    .align(Alignment.BottomEnd)
                    .systemBarsPadding()
                    .padding(12.dp),
            ) {
                RoundChip(
                    if (carrying) Copy.putDown else Copy.grab,
                    size = 62.dp,
                    selected = carrying,
                    testTag = "duck-grab",
                ) {
                    carrying = !carrying
                    if (carrying) duck.carry(0.0, 0.0, 0.0, true) else duck.carry(0.0, 0.0, 0.0, false)
                }
            }
        }

        // First-launch hint: one small centered caption above the bottom bar,
        // shown once (SharedPreferences flag) and fading out after 4 s.
        // AnimatedVisibility's default enter/exit are a fade, so no explicit
        // transition is needed; the timer only flips the flag off.
        AnimatedVisibility(
            visible = hintDismiss && core != null,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = 92.dp),
        ) {
            Text(
                Copy.firstLaunchHint,
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 12.sp,
            )
        }

        }

        // The Director shutter: one tap reads the frame on screen back, puts
        // it over the camera still (or over the studio when there is no
        // camera) and opens the preview. Outside the portrait/landscape
        // branches: the camera exists in both.
        if (surface == Surface.Director && core != null && onPhoto != null) {
            Box(
                Modifier
                    .align(Alignment.TopEnd)
                    .systemBarsPadding()
                    .padding(top = 72.dp, end = 16.dp),
            ) {
                RoundChip(
                    Copy.photoShort,
                    size = 64.dp,
                    selected = true,
                    selectedColor = Color.White.copy(alpha = 0.28f),
                    testTag = "duck-photo",
                    onClick = onPhoto,
                )
            }
        }

        // The ring is shell-wide: portrait anchors it to the bottom (the duck
        // above it stays visible), landscape keeps the centred card.
        if (showRing && core != null) {
            SkillRing(
                skills = skills,
                bottomAnchored = portrait,
                onPick = { name ->
                    dance.record("skill", name, skillLabel(name))
                    when (name) {
                        "sit" -> core.sit()
                        "ground_pick" -> core.groundPick()
                        else -> core.skill(name)
                    }
                    lastSkill = name
                    showRing = false
                },
                onClose = { showRing = false },
            )
        }
    }

    if (showSheet && core != null) {
        InstrumentSheet(
            core,
            input = input,
            micState = micState,
            flashlightState = flashlightState,
            onFlashlight = toggleFlashlight,
            speechFeed = speechFeed,
            onToggleSpeech = {
                when (speechFeed.state) {
                    SpeechState.Off, SpeechState.Denied, SpeechState.Unavailable -> {
                        val granted = ContextCompat.checkSelfPermission(
                            context,
                            Manifest.permission.RECORD_AUDIO,
                        ) == PackageManager.PERMISSION_GRANTED
                        if (granted) {
                            speechFeed.start()
                        } else {
                            voiceLauncher.launch(Manifest.permission.RECORD_AUDIO)
                        }
                    }
                    SpeechState.Listening -> speechFeed.stop()
                }
            },
            onToggleMic = {
                when (micState) {
                    MicState.Off -> {
                        val granted = ContextCompat.checkSelfPermission(
                            context,
                            Manifest.permission.RECORD_AUDIO,
                        ) == PackageManager.PERMISSION_GRANTED
                        if (granted) {
                            micFeed = MicFeed()
                            micFeed?.start()
                            micState = MicState.On
                        } else {
                            audioLauncher.launch(Manifest.permission.RECORD_AUDIO)
                        }
                    }
                    MicState.On -> {
                        micFeed?.stop()
                        micFeed = null
                        micState = MicState.Off
                    }
                    MicState.Denied -> audioLauncher.launch(Manifest.permission.RECORD_AUDIO)
                }
            },
            quests = quests,
            onPostcards = { showPostcards = true },
            onDex = { showDex = true },
            onDiary = { showDiary = true },
        ) { showSheet = false }
    }

    if (showDiary) {
        DiarySheet(onClose = { showDiary = false })
    }

    if (showDex) {
        DexSheet(dex, onClose = { showDex = false })
    }

    if (showPostcards) {
        PostcardsSheet(onClose = { showPostcards = false })
    }

    if (showQuests) {
        QuestsSheet(quests, onClose = { showQuests = false })
    }

    // The More drawer: every row is one tap from Live. Rows that open a
    // sheet close the drawer first (one sheet at a time, like the shell).
    if (showMore) {
        MoreSheet(
            board = quests,
            peers = peers,
            aiConfig = aiConfig,
            nightActive = nightActive,
            flashlightOn = flashlightState > 0,
            stick = stick,
            onPath = {
                stick = "path"
                core?.setHead(0.0, 0.0, 0.0, 0.0)
                core?.releaseBody()
                showMore = false
            },
            onMode = { mode ->
                stick = mode
                core?.setHead(0.0, 0.0, 0.0, 0.0)
                core?.releaseBody()
            },
            onRules = { showMore = false; showRules = true },
            onDance = { showMore = false; showDance = true },
            onPeers = { showMore = false; showPeers = true },
            onQuests = { showMore = false; showQuests = true },
            onDiary = { showMore = false; showDiary = true },
            onBig = { showMore = false; showBig = true },
            onPostcards = { showMore = false; showPostcards = true },
            onDex = { showMore = false; showDex = true },
            onAI = { showMore = false; showAI = true },
            onNight = {
                postDuckCommand(
                    "/night",
                    org.json.JSONObject().put("on", !nightActive).toString(),
                )
            },
            onFlashlight = toggleFlashlight,
            onLanguage = {
                Copy.language = if (Copy.language == Copy.Language.English) {
                    Copy.Language.Chinese
                } else {
                    Copy.Language.English
                }
                languageTick += 1
                soundLine = null
            },
            onClose = { showMore = false },
        )
    }

    if (showDance && core != null) {
        DanceSheet(
            studio = dance,
            onSkill = { name ->
                dance.record("skill", name, skillLabel(name))
                when (name) {
                    "sit" -> core.sit()
                    "ground_pick" -> core.groundPick()
                    else -> core.skill(name)
                }
            },
            onSound = { tag ->
                dance.record("sound", tag, tag)
                quack?.invoke()
            },
            onFeed = {
                dance.record("feed", label = Copy.feed)
                onFeed?.invoke()
            },
            onLight = { color ->
                dance.record("light", color, color)
                // The duck's own LED: the same client intent a rule uses.
                postDuckCommand(
                    "/flashlight",
                    """{"toggle":false,"on":${color != "off"},"color":"$color"}""",
                )
            },
            onClose = { showDance = false },
        )
    }

    if (showBig && core != null) {
        BigButtonsSheet(
            onPet = { postDuckCommand("/pet") },
            onFeed = { onFeed?.invoke() },
            onQuack = { quack?.invoke() },
            onSit = { core.sit() },
            onClose = { showBig = false },
        )
    }

    if (showRules && core != null) {
        RulesSheet(onClose = { showRules = false })
    }

    if (showPeers) {
        PeersSheet(
            peers = peers,
            onStart = { startPeers(context, android.os.Build.MODEL) },
            onGreet = { id -> greetPeer(context, id) },
            onPair = { code, id ->
                Thread {
                    postDuckCommand(
                        "/peers/pair",
                        org.json.JSONObject()
                            .put("code", code)
                            .apply { if (id != null) put("id", id) }
                            .toString(),
                    )
                }.start()
            },
            onPad = { on -> postDuckCommand("/pad", """{"on":$on}""") },
            onClose = { showPeers = false },
        )
    }

    if (showAI) {
        AISheet(
            config = aiConfig,
            status = aiStatus,
            decision = aiDecision,
            error = aiError,
            onSave = { config ->
                aiConfig = config
                AIStore.save(context, config)
                aiStatus = if (config.enabled) Copy.aiOn else Copy.aiOff
                aiError = null
            },
            onThink = { config ->
                aiConfig = config
                AIStore.save(context, config)
                lastAIThink = System.currentTimeMillis()
                thinkAI()
            },
            onClose = { showAI = false },
        )
    }
}

/** The user's own model: endpoint, key and how often it may decide. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AISheet(
    config: AIConfig,
    status: String,
    decision: String,
    error: String?,
    onSave: (AIConfig) -> Unit,
    onThink: (AIConfig) -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var enabled by remember { mutableStateOf(config.enabled) }
    var endpoint by remember { mutableStateOf(config.endpoint) }
    var model by remember { mutableStateOf(config.model) }
    var key by remember { mutableStateOf(config.key) }
    var interval by remember { mutableStateOf(config.interval.toString()) }
    var persona by remember { mutableStateOf(config.persona) }
    val current: () -> AIConfig = {
        AIConfig(
            enabled = enabled,
            endpoint = endpoint,
            model = model,
            key = key,
            interval = interval.toIntOrNull()?.coerceIn(10, 300) ?: 30,
            persona = persona,
        )
    }
    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.ai,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 18.sp,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("ai-title"),
                )
                CloseChip("ai-close", onClose)
            }
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.ai, color = Color.White, fontSize = 13.sp)
                Spacer(Modifier.weight(1f))
                Switch(checked = enabled, onCheckedChange = { enabled = it })
            }
            OutlinedTextField(
                value = endpoint,
                onValueChange = { endpoint = it },
                label = { Text(Copy.aiEndpoint) },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("ai-endpoint"),
            )
            OutlinedTextField(
                value = model,
                onValueChange = { model = it },
                label = { Text(Copy.aiModel) },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("ai-model"),
            )
            OutlinedTextField(
                value = key,
                onValueChange = { key = it },
                label = { Text(Copy.aiKey) },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("ai-key"),
            )
            OutlinedTextField(
                value = persona,
                onValueChange = { persona = it },
                label = { Text(Copy.aiPersona) },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("ai-persona"),
            )
            OutlinedTextField(
                value = interval,
                onValueChange = { interval = it },
                label = { Text(Copy.aiInterval) },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("ai-interval"),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                ButtonChip(Copy.aiSave, "ai-save") { onSave(current()) }
                ButtonChip(Copy.aiThinkNow, "ai-think") { onThink(current()) }
            }
            if (status.isNotEmpty()) {
                Text(
                    status,
                    color = Color.White.copy(alpha = 0.65f),
                    fontSize = 12.sp,
                    modifier = Modifier.testTag("ai-status"),
                )
            }
            if (error != null) {
                Text(error, color = Color(0xFFFFB74D), fontSize = 11.sp)
            }
            if (decision.isNotEmpty()) {
                Text(
                    decision,
                    color = Color(0xFF80DEEA),
                    fontSize = 12.sp,
                    modifier = Modifier.testTag("ai-decision"),
                )
            }
            Text(Copy.aiHint, color = Color.White.copy(alpha = 0.5f), fontSize = 10.sp)
            Text(Copy.aiNote, color = Color.White.copy(alpha = 0.4f), fontSize = 10.sp)
        }
    }
}

/**
 * Other ducks on this Wi-Fi. Discovery is the core's UDP announce; the list
 * updates from /state while this sheet is open. A greeting is one of the
 * duck's social reactions, and the answer comes back as "X answered!". A
 * gift is a treat or a cheer aimed at one duck, sent through /peers/gift.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PeersSheet(
    peers: PeersUi,
    onStart: () -> Unit,
    onGreet: (String?) -> Unit,
    onPair: (String, String?) -> Unit,
    onPad: (Boolean) -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    // One gift on the wire; the POST is IO-bound and a refusal is the core's
    // to say, so the sheet stays as it was either way.
    val sendGift: (String, String, String) -> Unit = { id, gift, value ->
        scope.launch {
            withContext(Dispatchers.IO) {
                try {
                    postJson(
                        "/peers/gift",
                        org.json.JSONObject()
                            .put("id", id)
                            .put("gift", gift)
                            .put("value", value)
                            .toString(),
                    )
                } catch (_: Throwable) {
                }
            }
        }
    }
    // A chorus is for the room: the default two official moves, posted on IO
    // like a gift. The core broadcasts and starts the shared countdown.
    val startChorus: () -> Unit = {
        scope.launch {
            withContext(Dispatchers.IO) {
                try {
                    postJson("/chorus", "{}")
                } catch (_: Throwable) {
                }
            }
        }
    }
    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        Copy.peers,
                        color = Color.White,
                        fontWeight = FontWeight.SemiBold,
                        fontSize = 18.sp,
                        modifier = Modifier.testTag("peers-title"),
                    )
                    if (peers.name.isNotEmpty()) {
                        Text(
                            "${Copy.name} · ${peers.name}",
                            color = Color.White.copy(alpha = 0.6f),
                            fontSize = 11.sp,
                        )
                    }
                }
                CloseChip("peers-close", onClose)
            }
            var netPadOn by remember { mutableStateOf(false) }
    var pairCode by remember { mutableStateOf("") }
    OutlinedTextField(
        value = pairCode,
        onValueChange = { text -> pairCode = text.filter { it.isDigit() }.take(6) },
        label = { Text(Copy.pairCode) },
        singleLine = true,
        modifier = Modifier.fillMaxWidth().testTag("peers-pair-code"),
    )
    Text(
        Copy.pairNote,
        color = Color.White.copy(alpha = 0.6f),
        fontSize = 10.sp,
    )
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(Copy.netPad, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
        Spacer(Modifier.size(8.dp))
        ButtonChip(
            if (netPadOn) Copy.flashlightOff else Copy.flashlightOn,
            "peers-netpad",
        ) {
            netPadOn = !netPadOn
            onPad(netPadOn)
        }
    }
    Text(
        Copy.netPadNote,
        color = Color.White.copy(alpha = 0.6f),
        fontSize = 10.sp,
    )

    if (peers.peers.isEmpty()) {
                Text(
                    Copy.peersEmpty,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 12.sp,
                    modifier = Modifier.testTag("peers-empty"),
                )
            } else {
                peers.peers.forEachIndexed { index, peer ->
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .background(Color.White.copy(alpha = 0.06f), RoundedCornerShape(12.dp))
                            .padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Row(
                            Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            Box(
                                Modifier
                                    .size(8.dp)
                                    .background(
                                        if (peer.replied) Color(0xFF66BB6A) else Color(0xFF4FC3F7),
                                        CircleShape,
                                    ),
                            )
                            Column(Modifier.weight(1f)) {
                                Text(
                                    peer.name,
                                    color = Color.White,
                                    fontSize = 14.sp,
                                    fontWeight = FontWeight.SemiBold,
                                    modifier = Modifier.testTag("peer-row-$index"),
                                )
                                Text(
                                    if (peer.replied) Copy.peerRepliedLabel else Copy.peerNew,
                                    color = Color.White.copy(alpha = 0.6f),
                                    fontSize = 10.sp,
                                )
                            }
                            if (!peer.paired) {
                                ButtonChip(Copy.pairCode, "peer-pair-$index") {
                                    if (pairCode.length == 6) onPair(pairCode, peer.id)
                                }
                            }
                            ButtonChip(Copy.peerSayHi, "peer-greet-$index") { onGreet(peer.id) }
                        }
                        // Gifts in the greeting's own chip language: a treat
                        // feeds through the real peck, a cheer runs the
                        // official excited timeline on the other duck.
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            ButtonChip(Copy.giftTreat, "gift-treat-${peer.id}") {
                                sendGift(peer.id, "treat", "")
                            }
                            ButtonChip(Copy.giftEmotion, "gift-emotion-${peer.id}") {
                                sendGift(peer.id, "emotion", "excited")
                            }
                        }
                    }
                }
                ButtonChip(Copy.peerSayAll, "peer-greet-all") { onGreet(null) }
                if (pairCode.length == 6) {
                    ButtonChip(Copy.pairCode, "peers-pair-code-set") { onPair(pairCode, null) }
                }
                val sendEnsemble: (List<String>, List<String>) -> Unit = { moves, sounds ->
                    val body = org.json.JSONObject()
                        .put("moves", org.json.JSONArray(moves))
                        .put("sounds", org.json.JSONArray(sounds))
                    Thread { postDuckCommand("/ensemble", body.toString()) }.start()
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ButtonChip(Copy.formation, "ensemble-formation") {
                        sendEnsemble(listOf("laugh", "yes_fast", "excited", "curious"), emptyList())
                    }
                    ButtonChip(Copy.roleDance, "ensemble-roles") {
                        sendEnsemble(listOf("laugh", "yes_fast", "excited", "curious", "sad", "inquire"), emptyList())
                    }
                    ButtonChip(Copy.voiceChoir, "ensemble-choir") {
                        sendEnsemble(listOf("laugh", "curious"), listOf("chirp", "coo", "greet", "inquire"))
                    }
                }
                // A chorus needs someone to sing with: a lone duck sees the
                // disabled affordance rather than losing the idea of it.
                // PathChip is the shared chip face that carries `enabled`.
                PathChip(
                    Copy.chorus,
                    enabled = peers.peers.isNotEmpty(),
                    testTag = "chorus-start",
                ) { startChorus() }
            }
            if (peers.name.isEmpty()) {
                ButtonChip(Copy.peerFind, "peers-start") { onStart() }
            }
        }
    }
}

/**
 * The block program, edited as big chips: when a clap/whistle/pet happens,
 * make the duck do one of its own moves, moods or sounds. Every edit is POSTed
 * to the core, which runs it; the shell keeps the copy across launches.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RulesSheet(onClose: () -> Unit) {
    val context = LocalContext.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var rules by remember { mutableStateOf(emptyList<RuleUi>()) }
    // Whether the core's own program has been read. Until it has, the list
    // here is not the duck's program but the default empty one, and merging
    // an import into it would replace everything the child built.
    var rulesLoaded by remember { mutableStateOf(false) }
    var trigger by remember { mutableStateOf("clap") }
    var action by remember { mutableStateOf("skill") }
    var value by remember { mutableStateOf("roulade") }
    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) { fetchRules() }?.let {
            rules = it
            rulesLoaded = true
        }
    }
    // Show the new program at once, then keep it only if the core took it.
    // A refusal puts the old program back and says so: the editor must never
    // show blocks the duck is not running.
    fun applyRules(next: List<RuleUi>) {
        val previous = rules
        rules = next
        saveRules(context, next) { accepted ->
            if (!accepted) {
                rules = previous
                Toast.makeText(context, Copy.rulesRefusedText, Toast.LENGTH_LONG).show()
            }
        }
    }
    val triggers = listOf("clap", "whistle", "pet")
    val kinds = listOf("skill", "emotion", "sound")
    val values = when (action) {
        "skill" -> listOf("roulade", "kick_left", "kick_right", "sit", "ground_pick")
        "emotion" -> listOf(
            "excited", "laugh", "curious", "yes", "no", "sad", "angry",
            "defiant", "impatient", "mock", "mmh", "yes_fast", "devastated",
        )
        else -> listOf("chirp", "coo", "greet", "inquire", "peck", "alarm")
    }
    LaunchedEffect(action) { value = values.first() }

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.blocks,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 18.sp,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("rules-title"),
                )
                CloseChip("rules-close", onClose)
            }
            if (rules.isEmpty()) {
                Text(
                    Copy.rulesEmpty,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 12.sp,
                )
            }
            // A program travels between phones as text: the share sheet
            // carries the versioned document, paste reads one back and
            // merges it into what the child already built.
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(Copy.rulesShare, "rules-share") {
                    if (rules.isNotEmpty()) {
                        val send = android.content.Intent(android.content.Intent.ACTION_SEND)
                            .setType("text/plain")
                            .putExtra(android.content.Intent.EXTRA_TEXT, rulesShareText(rules))
                        context.startActivity(
                            android.content.Intent.createChooser(send, Copy.rulesShare),
                        )
                    }
                }
                ButtonChip(Copy.rulesPaste, "rules-paste") {
                    val clip = (context.getSystemService(Context.CLIPBOARD_SERVICE)
                        as? android.content.ClipboardManager)
                        ?.primaryClip
                        ?.takeIf { it.itemCount > 0 }
                        ?.getItemAt(0)
                        ?.coerceToText(context)
                        ?.toString()
                    val imported = clip?.let(::parseSharedRules)
                    if (!rulesLoaded) {
                        // The core's program has not been read yet, so there
                        // is nothing to merge onto: doing it anyway would
                        // replace everything the child built.
                        Toast.makeText(context, Copy.rulesNotReady, Toast.LENGTH_LONG).show()
                    } else if (imported != null) {
                        val next = mergeRules(rules, imported)
                        val added = next.size - rules.size
                        applyRules(next)
                        Toast.makeText(
                            context,
                            Copy.rulesPasted(added.coerceAtLeast(0)),
                            Toast.LENGTH_SHORT,
                        ).show()
                    } else {
                        Toast.makeText(context, Copy.rulesPasteFailed, Toast.LENGTH_SHORT).show()
                    }
                }
            }
            rules.forEachIndexed { index, rule ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .background(
                            Color(0xFF2A4A5E),
                            RoundedCornerShape(12.dp),
                        )
                        .padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        "${Copy.ruleWhen} ${Copy.ruleTrigger(rule.trigger)} → " +
                            if (rule.action == "sound") rule.value else skillLabel(rule.value),
                        color = Color.White,
                        fontSize = 13.sp,
                        modifier = Modifier
                            .weight(1f)
                            .padding(vertical = 12.dp)
                            .testTag("rule-$index"),
                    )
                    CloseChip("rule-delete-$index", {
                        applyRules(rules.filterIndexed { at, _ -> at != index })
                    })
                }
            }
            Text(Copy.ruleWhen, color = Color.White.copy(alpha = 0.75f), fontSize = 13.sp)
            Row(
                Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                triggers.forEach { option ->
                    RuleChip(
                        Copy.ruleTrigger(option),
                        "rule-trigger-$option",
                        trigger == option,
                    ) { trigger = option }
                }
            }
            Text(Copy.ruleThen, color = Color.White.copy(alpha = 0.75f), fontSize = 13.sp)
            Row(
                Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                kinds.forEach { option ->
                    RuleChip(
                        Copy.ruleKind(option),
                        "rule-kind-$option",
                        action == option,
                    ) { action = option }
                }
            }
            Row(
                Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                values.forEach { option ->
                    RuleChip(
                        if (action == "sound") option else skillLabel(option),
                        "rule-value-$option",
                        value == option,
                    ) { value = option }
                }
            }
            var chain by remember { mutableStateOf(emptyList<RuleStep>()) }
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.ruleChain,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 12.sp,
                )
                Spacer(Modifier.weight(1f))
                ButtonChip(Copy.ruleChainAdd, "rule-chain-add") {
                    if (chain.size < 4) chain = chain + RuleStep(action, value)
                }
            }
            if (chain.isNotEmpty()) {
                Row(
                    Modifier.horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    chain.forEachIndexed { index, step ->
                        RuleChip(
                            if (step.action == "sound") step.value else skillLabel(step.value),
                            "rule-chain-$index",
                            false,
                        ) { chain = chain.filterIndexed { at, _ -> at != index } }
                    }
                }
            }
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "${rules.size}/8",
                    color = Color.White.copy(alpha = 0.5f),
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                )
                Spacer(Modifier.weight(1f))
                ButtonChip(
                    if (rules.size >= 8) Copy.ruleLimit else Copy.addRule,
                    "rule-add",
                ) {
                    if (rules.size < 8) {
                        applyRules(rules + RuleUi(trigger, action, value, chain))
                        chain = emptyList()
                    }
                }
            }
        }
    }
}

@Composable
private fun RuleChip(
    label: String,
    tag: String,
    selected: Boolean,
    onPick: () -> Unit,
) {
    Box(
        Modifier
            .heightIn(min = 44.dp)
            .background(
                if (selected) Color.White.copy(alpha = 0.92f)
                else Color.White.copy(alpha = 0.10f),
                RoundedCornerShape(22.dp),
            )
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = label,
                role = Role.Button,
            ) { onPick() }
            .testTag(tag)
            .padding(horizontal = 14.dp, vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = if (selected) Color.Black else Color.White.copy(alpha = 0.85f),
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

enum class Surface { Live, Director }

/**
 * The status capsule and the one menu, shared by both compositions so the
 * portrait bar and the wide bar cannot drift apart. The capsule carries the
 * duck's name, its live label and the last skill the ring ran; the menu holds
 * the stick modes, the surface, the language and the flashlight.
 */
@Composable
private fun TopBar(
    core: DuckCore?,
    carrying: Boolean,
    lastSkill: String,
    behaviourRaw: String,
    menuOpen: Boolean,
    onMenuOpen: (Boolean) -> Unit,
    onStick: (String) -> Unit,
    onSurface: (Surface) -> Unit,
    onFlashlight: () -> Unit,
    onRules: () -> Unit,
    onDance: () -> Unit,
    onPeers: () -> Unit,
    onAI: () -> Unit,
    onLanguage: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Column(
            Modifier
                .background(Color.Black.copy(alpha = 0.35f), RoundedCornerShape(14.dp))
                .padding(horizontal = 12.dp, vertical = 7.dp),
        ) {
            Text(Copy.name, color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
            Text(
                when {
                    core == null -> Copy.waking
                    carrying -> Copy.held
                    core.paused() -> Copy.freeze
                    // The wire word is the core's; a person reads the
                    // translated one. The sheet still keeps the raw label.
                    // Until the first /state poll lands there is no word to
                    // translate, so say the honest thing: Live.
                    behaviourRaw.isEmpty() -> Copy.live
                    else -> Copy.behaviourOf(behaviourRaw)
                },
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 11.sp,
                modifier = Modifier.testTag("duck-label"),
            )
            if (lastSkill.isNotEmpty()) {
                Text(
                    // The ring's verdict outlives the move. The protocol name
                    // is `kick_left`; the shell says what a person calls it.
                    skillLabel(lastSkill),
                    color = Color.White.copy(alpha = 0.55f),
                    fontSize = 10.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.testTag("duck-last-skill"),
                )
            }
        }
        Spacer(Modifier.weight(1f))
        Box {
            RoundChip("⋯", size = 48.dp, testTag = "duck-menu") { onMenuOpen(true) }
            DropdownMenu(
                expanded = menuOpen,
                onDismissRequest = { onMenuOpen(false) },
                modifier = Modifier.background(Color(0xFF141A22)),
            ) {
                DropdownMenuItem(
                    text = { Text(Copy.stick("drive"), color = Color.White) },
                    onClick = { onStick("drive"); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.stick("head"), color = Color.White) },
                    onClick = { onStick("head"); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.stick("body"), color = Color.White) },
                    onClick = { onStick("body"); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.stick("path"), color = Color.White) },
                    onClick = { onStick("path"); onMenuOpen(false) },
                    modifier = Modifier.testTag("mode-path"),
                )
                HorizontalDivider(color = Color.White.copy(alpha = 0.12f))
                DropdownMenuItem(
                    text = { Text(Copy.live, color = Color.White) },
                    onClick = { onSurface(Surface.Live); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.director, color = Color.White) },
                    onClick = { onSurface(Surface.Director); onMenuOpen(false) },
                )
                HorizontalDivider(color = Color.White.copy(alpha = 0.12f))
                DropdownMenuItem(
                    text = { Text(Copy.flashlight, color = Color.White) },
                    onClick = { onFlashlight(); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.blocks, color = Color.White) },
                    onClick = { onRules(); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.dance, color = Color.White) },
                    onClick = { onDance(); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.peers, color = Color.White) },
                    onClick = { onPeers(); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.ai, color = Color.White) },
                    onClick = { onAI(); onMenuOpen(false) },
                )
                DropdownMenuItem(
                    text = { Text(Copy.language.label, color = Color.White) },
                    onClick = { onLanguage(); onMenuOpen(false) },
                )
            }
        }
    }
}

/**
 * The portrait title bar (B4 §1): read-only. Name, what the duck is doing,
 * and the ring's last verdict; every menu moved into the More drawer.
 */
@Composable
private fun PortraitTitleBar(
    core: DuckCore?,
    carrying: Boolean,
    lastSkill: String,
    behaviourRaw: String,
    modifier: Modifier = Modifier,
) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Column(
            Modifier
                .background(Color.Black.copy(alpha = 0.35f), RoundedCornerShape(14.dp))
                .padding(horizontal = 12.dp, vertical = 7.dp),
        ) {
            Text(Copy.name, color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
            Text(
                when {
                    core == null -> Copy.waking
                    carrying -> Copy.held
                    core.paused() -> Copy.freeze
                    behaviourRaw.isEmpty() -> Copy.live
                    else -> Copy.behaviourOf(behaviourRaw)
                },
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 11.sp,
                modifier = Modifier.testTag("duck-label"),
            )
            if (lastSkill.isNotEmpty()) {
                Text(
                    skillLabel(lastSkill),
                    color = Color.White.copy(alpha = 0.55f),
                    fontSize = 10.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.testTag("duck-last-skill"),
                )
            }
        }
        Spacer(Modifier.weight(1f))
    }
}

/**
 * The portrait time rail (B4 §1): two buttons, icon only. Time opens the
 * time machine and a long press freezes at once; Director is the second.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PortraitTimeRail(
    core: DuckCore,
    surface: Surface,
    onSurface: (Surface) -> Unit,
    onOpenSheet: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var tick by remember { mutableStateOf(core.tickIndex()) }
    var oldest by remember { mutableStateOf(core.oldestTick()) }
    LaunchedEffect(core) {
        while (true) {
            tick = core.tickIndex()
            oldest = core.oldestTick()
            delay(200)
        }
    }
    // The share of retained history, not of the whole run: the bar fills as
    // checkpoints age out of the ring buffer.
    val newest = tick.coerceAtLeast(0L)
    val oldestClamped = oldest.coerceIn(0L, newest)
    val fraction = (
        (newest - oldestClamped).toDouble() / newest.coerceAtLeast(1L).toDouble()
        ).coerceIn(0.0, 1.0)
    Row(
        modifier.height(48.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier
                .weight(1f)
                .fillMaxHeight()
                .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(14.dp))
                .combinedClickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClickLabel = Copy.timeMachine,
                    onLongClickLabel = Copy.freeze,
                    role = Role.Button,
                    onClick = onOpenSheet,
                    onLongClick = { core.setPaused(true) },
                )
                .testTag("duck-time")
                .padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(
                Icons.time,
                contentDescription = null,
                tint = Color.White.copy(alpha = 0.85f),
                modifier = Modifier.size(18.dp),
            )
            // The bar is about 60% of the row: it reads as a progress, not
            // as a divider between the time and Director buttons.
            Box(
                Modifier.weight(1f),
                contentAlignment = Alignment.CenterStart,
            ) {
                Box(
                    Modifier
                        .fillMaxWidth(0.6f)
                        .height(3.dp)
                        .background(Color.White.copy(alpha = 0.08f), CircleShape),
                ) {
                    Box(
                        Modifier
                            .fillMaxWidth(fraction.toFloat())
                            .fillMaxHeight()
                            // accent.data #7FB2D9, solid: the retained-history
                            // fill must read against the track (P1-10).
                            .background(Color(0xFF7FB2D9), CircleShape),
                    )
                }
            }
        }
        IconChip(
            Icons.director,
            if (surface == Surface.Live) Copy.director else Copy.live,
            size = 48.dp,
            testTag = "surface-toggle",
        ) {
            onSurface(if (surface == Surface.Live) Surface.Director else Surface.Live)
        }
    }
}

/**
 * The portrait thumb deck (B4 §1): four verbs and the More drawer, all one
 * primary shape — a 52 dp circle, an 18 dp icon and an 11 sp label.
 */
@Composable
private fun ThumbDeck(
    core: DuckCore,
    carrying: Boolean,
    onCarrying: (Boolean) -> Unit,
    quack: (() -> Unit)?,
    onFeed: (() -> Unit)?,
    onWheee: ((Boolean) -> Unit)?,
    onSkills: () -> Unit,
    onMore: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier
            .height(90.dp)
            .background(Color(0xFF141A22).copy(alpha = 0.94f), RoundedCornerShape(28.dp))
            .padding(horizontal = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Spacer(Modifier.weight(1f))
        PrimaryVerb(Copy.feed, Icons.feed, "duck-feed", onTap = { onFeed?.invoke() })
        PrimaryVerb(Copy.skills, Icons.skills, "duck-skills", onTap = { onSkills() })
        PrimaryVerb(
            Copy.quack,
            Icons.quack,
            "duck-quack",
            onTap = { quack?.invoke() },
            onHold = onWheee,
        )
        PrimaryVerb(
            if (carrying) Copy.putDown else Copy.grab,
            if (carrying) Icons.drop else Icons.grab,
            "duck-grab",
            selected = carrying,
            onTap = {
                val next = !carrying
                onCarrying(next)
                core.carry(0.0, 0.0, 0.0, next)
            },
        )
        PrimaryVerb(Copy.more, Icons.more, "duck-more", onTap = { onMore() })
    }
}

/**
 * The one primary button (B4 §3): 52 dp circle, 18 dp icon, 11 sp label
 * underneath. Optional hold = the wheee ride on the quack button.
 */
@Composable
private fun PrimaryVerb(
    label: String,
    icon: ImageVector,
    tag: String,
    selected: Boolean = false,
    onTap: () -> Unit,
    onHold: ((Boolean) -> Unit)? = null,
) {
    // The quack's hold is the official wheee ride: it starts on the long
    // press and has to end on the real lift, so it uses the same gesture as
    // the old WheeeChip. Plain verbs are ordinary clicks.
    val hold = onHold
    val gesture = if (hold != null) {
        Modifier.pointerInput(Unit) {
            var riding = false
            detectTapGestures(
                onTap = { onTap() },
                onLongPress = {
                    riding = true
                    hold(true)
                },
                onPress = {
                    try {
                        tryAwaitRelease()
                    } finally {
                        if (riding) {
                            riding = false
                            hold(false)
                        }
                    }
                },
            )
        }
    } else {
        Modifier.clickable(
            interactionSource = remember { MutableInteractionSource() },
            indication = null,
            onClickLabel = label,
            role = Role.Button,
            onClick = onTap,
        )
    }
    Column(
        Modifier
            .width(52.dp)
            .then(gesture)
            .testTag(tag),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Box(
            Modifier
                .size(52.dp)
                .background(
                    if (selected) ActionAccent.copy(alpha = 0.35f)
                    else Color.White.copy(alpha = 0.10f),
                    CircleShape,
                ),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                icon,
                contentDescription = null,
                tint = Color.White.copy(alpha = 0.9f),
                modifier = Modifier.size(18.dp),
            )
        }
        Text(
            label,
            color = Color.White.copy(alpha = 0.85f),
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            softWrap = false,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** A 48 dp secondary button: icon only, always with an accessibility label. */
@Composable
private fun IconChip(
    icon: ImageVector,
    label: String,
    size: androidx.compose.ui.unit.Dp = 48.dp,
    testTag: String? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .size(size.coerceAtLeast(48.dp))
            .background(Color.White.copy(alpha = 0.10f), CircleShape)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = label,
                role = Role.Button,
                onClick = onClick,
            )
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            icon,
            contentDescription = label,
            tint = Color.White.copy(alpha = 0.9f),
            modifier = Modifier.size(18.dp),
        )
    }
}

/** A 48 dp close target shared by the ring and the instrument sheet. */
@Composable
internal fun CloseChip(tag: String, onClose: () -> Unit, size: Dp = 48.dp) {
    Box(
        Modifier
            .size(size.coerceAtLeast(48.dp))
            .background(Color.White.copy(alpha = 0.10f), CircleShape)
            // The glyph alone is noise to TalkBack: name the action, and let
            // a double-tap run the same close the finger tap does.
            .semantics { contentDescription = Copy.close }
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = Copy.close,
                role = Role.Button,
            ) { onClose() }
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) {
        Text("×", color = Color.White.copy(alpha = 0.9f), fontSize = 16.sp)
    }
}

/**
 * Portrait steering is translation-based from wherever the thumb lands:
 * 8 dp of slack, 90 dp for full scale, up is forward and sideways turns.
 * A tap on the duck posts /pet; a tap on the floor in drive mode nudges the
 * duck for 250 ms; a stationary 0.35 s hold opens the skill ring.
 * In path mode the same stage draws instead: the down lays the first
 * waypoint, the drag lays the rest, and while the duck walks its path the
 * stage takes no stroke at all — the cancel chip is the only control, and a
 * drag is the external twist that hands the walk back to the child's hand.
 */
private fun Modifier.portraitStageGesture(
    core: DuckCore,
    input: DuckInput,
    carrying: Boolean,
    stick: String,
    pathActive: Boolean,
    onPathDraw: (Offset) -> Unit,
    ringOpen: () -> Boolean,
    onLongPress: () -> Unit,
): Modifier = pointerInput(core, carrying, stick, pathActive) {
    val deadzone = 8.dp.toPx()
    val fullScale = 90.dp.toPx()
    val handler = Handler(Looper.getMainLooper())
    // A new touch invalidates any pending pulse reset, so an old timer cannot
    // stop the shove a newer gesture just started.
    var pulseToken = 0
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false)
        pulseToken += 1
        input.vx = 0.0
        input.wz = 0.0
        var dragged = false
        var longPressed = false
        var finished = false
        var last = down
        // Path mode draws: the down lays the first waypoint, the ring guard
        // comes first — nothing is collected while it is up — and a carry
        // keeps moving the hand.
        if (stick == "path" && !pathActive && !carrying && !ringOpen()) {
            onPathDraw(down.position)
        }
        // A Handler, not a coroutine: `awaitEachGesture` runs in a restricted
        // suspension scope and cannot launch on another scope.
        val hold = Runnable {
            if (!dragged && !longPressed) {
                longPressed = true
                onLongPress()
            }
        }
        handler.postDelayed(hold, 350)
        try {
            while (true) {
                val event = awaitPointerEvent()
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                last = change
                val dx = change.position.x - down.position.x
                val dy = change.position.y - down.position.y
                val distance = hypot(dx, dy)
                if (!dragged && distance >= deadzone) {
                    dragged = true
                    handler.removeCallbacks(hold)
                }
                if (dragged && !longPressed) {
                    // The skill ring is up: no carry, no drive while it is —
                    // the gesture that opened it must not steer underneath.
                    if (ringOpen()) {
                        input.vx = 0.0
                        input.wz = 0.0
                        return@awaitEachGesture
                    }
                    change.consume()
                    if (carrying) {
                        val halfWidth = size.width.toFloat().coerceAtLeast(1f) * 0.5f
                        val halfHeight = size.height.toFloat().coerceAtLeast(1f) * 0.5f
                        core.carry(
                            (dx / halfWidth * 0.35f).toDouble(),
                            0.0,
                            (-dy / halfHeight * 0.30f).toDouble(),
                            true,
                        )
                    } else if (stick == "path" && !pathActive) {
                        // Drawing, not steering: the collector keeps the
                        // spacing and the cap. While the duck walks, the drag
                        // steers below — an external twist the core answers by
                        // handing the walk back.
                        onPathDraw(change.position)
                    } else {
                        val scale = min(distance / fullScale, 1f)
                        val nx = if (distance > 0f) dx / distance * scale else 0f
                        val ny = if (distance > 0f) dy / distance * scale else 0f
                        when (stick) {
                            "head" -> core.setHead(
                                0.0,
                                (ny * 0.6f).toDouble(),
                                (nx * 1.2f).toDouble(),
                                0.0,
                            )
                            "body" -> core.setBody(
                                (-ny * 0.05f).toDouble(),
                                (nx * 0.12f).toDouble(),
                                0.0,
                            )
                            else -> {
                                input.vx = (-ny * 0.4f).toDouble()
                                input.wz = (-nx * 1.0f).toDouble()
                            }
                        }
                    }
                }
                if (!change.pressed) break
            }
            handler.removeCallbacks(hold)
            if (carrying) {
                // A held duck stays where the hand left it.
                finished = true
                return@awaitEachGesture
            }
            val heldFor = last.uptimeMillis - down.uptimeMillis
            val wasTap = last.changedToUp() && !dragged && heldFor < 300
            if (wasTap && !longPressed) {
                // A path tap is the one waypoint it already laid on the down;
                // pet and the drive pulse are other modes' words.
                if (stick != "path") {
                    val width = size.width.toFloat().coerceAtLeast(1f)
                    val height = size.height.toFloat().coerceAtLeast(1f)
                    val x = last.position.x
                    val y = last.position.y
                    val insideDuck = x > width * 0.25f && x < width * 0.75f &&
                        y > height * 0.33f && y < height * 0.68f
                    if (insideDuck) {
                        postDuckCommand("/pet")
                    } else if (stick == "drive") {
                        val dx = x - width * 0.5f
                        val dy = y - height * 0.5f
                        val norm = maxOf(hypot(dx, dy), 1f)
                        input.vx = (-dy / norm * 0.6f).toDouble()
                        input.wz = (-dx / norm * 0.6f).toDouble()
                        val token = pulseToken
                        handler.postDelayed({
                            if (pulseToken == token) {
                                input.vx = 0.0
                                input.wz = 0.0
                            }
                        }, 250)
                    }
                }
                finished = true
            } else {
                input.vx = 0.0
                input.wz = 0.0
                core.setHead(0.0, 0.0, 0.0, 0.0)
                core.releaseBody()
                finished = true
            }
        } finally {
            handler.removeCallbacks(hold)
            // A gesture cancelled by the system never reports its release;
            // hand the duck back neutral rather than leaving a walk latched.
            if (!finished && !carrying) {
                input.vx = 0.0
                input.wz = 0.0
                core.setHead(0.0, 0.0, 0.0, 0.0)
                core.releaseBody()
            }
        }
    }
}

/**
 * The wide layout's scene hold: 0.35 s of stillness opens the ring. This is a
 * second pointer stream beside the drag detector; a drag consumes movement
 * once the touch slop is crossed, which cancels [waitForUpOrCancellation] and
 * with it the hold.
 */
private fun Modifier.sceneLongPress(
    enabled: Boolean,
    onLongPress: () -> Unit,
): Modifier {
    if (!enabled) return this
    return pointerInput(enabled) {
        awaitEachGesture {
            // A foundation helper, not a coroutine: the restricted pointer
            // scope cannot launch timers on its own.
            val down = awaitFirstDown(requireUnconsumed = false)
            if (awaitLongPressOrCancellation(down.id) != null) {
                onLongPress()
            }
        }
    }
}

/**
 * One command POST on a worker thread: the control that drew it must never
 * block the frame. Both /pet and /feed answer with a refusal when the duck is
 * busy; that refusal has nothing to show on screen.
 */
internal fun postDuckCommand(path: String, body: String = "{}") {
    Thread {
        try {
            val connection = java.net.URL("http://127.0.0.1:8765$path")
                .openConnection() as java.net.HttpURLConnection
            connection.requestMethod = "POST"
            connection.connectTimeout = 2000
            connection.readTimeout = 5000
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", "application/json")
            connection.outputStream.use { it.write(body.toByteArray()) }
            connection.inputStream.readBytes()
            connection.disconnect()
        } catch (_: Exception) {
            // The loop refuses when busy; nothing to show.
        }
    }.start()
}

/**
 * Queue a drawn walk: POST /path with the stroke in metres, body frame at the
 * path's start (+x forward, +y left), the first element the duck's own origin
 * — the core drops it. The answer is a queue verdict; a refusal (over the 64
 * point or 20 m caps) comes back as {"ok":false} and nothing is queued.
 * Synchronous on purpose: the caller owns the thread.
 */
internal fun postPath(points: List<List<Double>>): Boolean {
    val array = org.json.JSONArray()
    points.forEach { point ->
        val pair = org.json.JSONArray()
        pair.put(point[0])
        pair.put(point[1])
        array.put(pair)
    }
    return try {
        postJson("/path", org.json.JSONObject().put("points", array).toString())
    } catch (_: Throwable) {
        false
    }
}

/// POST /path/cancel, fire-and-forget: {"ok":true} always, a no-op when
/// nothing is active.
internal fun postPathCancel() {
    Thread {
        try {
            postJson("/path/cancel", "{}")
        } catch (_: Throwable) {
        }
    }.start()
}

/// The walk the path runner reports, from the top-level "path" object of
/// GET /state: active, the waypoints reached and owed, and the accepted
/// body-frame list as posted. An object nobody published (an older core, a
/// dead server) reads as "nothing walking".
data class PathUi(
    val active: Boolean = false,
    val reached: Int = 0,
    val total: Int = 0,
    val points: List<List<Double>> = emptyList(),
)

internal fun parsePathFromState(stateText: String): PathUi = try {
    val path = org.json.JSONObject(stateText).optJSONObject("path") ?: return PathUi()
    val array = path.optJSONArray("points") ?: org.json.JSONArray()
    PathUi(
        active = path.optBoolean("active", false),
        reached = path.optInt("reached", 0),
        total = path.optInt("total", 0),
        points = (0 until array.length()).mapNotNull { index ->
            val pair = array.optJSONArray(index) ?: return@mapNotNull null
            listOf(pair.optDouble(0, 0.0), pair.optDouble(1, 0.0))
        },
    )
} catch (_: Throwable) {
    PathUi()
}

/// The stroke in metres, body frame at the stroke's start: screen up is
/// forward, screen left is +y left. The stage's short side spans 2.2 m, the
/// mapping the post assumes. Null while the stage has no size yet.
internal fun pathMetres(points: List<Offset>, stagePx: IntSize): List<List<Double>>? {
    val short = min(stagePx.width, stagePx.height)
    if (points.isEmpty() || short <= 0) return null
    val metresPerPoint = 2.2f / short
    val x0 = points.first().x
    val y0 = points.first().y
    return points.map { point ->
        listOf(
            (-(point.y - y0) * metresPerPoint).toDouble(),
            (-(point.x - x0) * metresPerPoint).toDouble(),
        )
    }
}

/// The waypoints the core would keep from a posted stroke: the origin (the
/// first element) dropped, then anything 0.1 m or more from the last kept
/// one. The go chip's enabled state is this count, so the UI never invites
/// the refusal the core would answer.
internal fun keptWaypoints(metres: List<List<Double>>): List<List<Double>> {
    val kept = mutableListOf<List<Double>>()
    for (point in metres.drop(1)) {
        val last = kept.lastOrNull()
        if (last == null || hypot(point[0] - last[0], point[1] - last[1]) >= 0.1) {
            kept.add(point)
        }
    }
    return kept
}

/// A stroke is walkable when two or more waypoints survive the thinning and
/// the whole polyline — the duck's real walking distance, origin included —
/// stays inside the core's 20 m budget.
internal fun pathWalkable(metres: List<List<Double>>): Boolean {
    if (keptWaypoints(metres).size < 2) return false
    var total = 0.0
    for (index in 1 until metres.size) {
        total += hypot(
            metres[index][0] - metres[index - 1][0],
            metres[index][1] - metres[index - 1][1],
        )
    }
    return total <= 20.0
}

@Composable
private fun SurfaceChip(
    label: String,
    selected: Boolean,
    testTag: String? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .padding(horizontal = 2.dp)
            // The chip was a 26 dp target; every control gets a 48 dp floor.
            .sizeIn(minWidth = 48.dp, minHeight = 48.dp)
            .background(
                if (selected) Color.White.copy(alpha = 0.18f) else Color.Transparent,
                CircleShape,
            )
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = label,
                role = Role.Button,
            ) { onClick() }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier)
            .padding(horizontal = 10.dp, vertical = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White.copy(alpha = 0.85f), fontSize = 11.sp)
    }
}

@Composable
private fun RoundChip(
    label: String,
    size: Dp = 52.dp,
    selected: Boolean = false,
    selectedColor: Color = Color.White.copy(alpha = 0.24f),
    testTag: String? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .size(size.coerceAtLeast(48.dp))
            .clip(CircleShape)
            .background(
                if (selected) selectedColor else Color.White.copy(alpha = 0.10f),
                CircleShape,
            )
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = label,
                role = Role.Button,
            ) { onClick() }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = Color.White.copy(alpha = 0.9f),
            fontSize = 11.sp,
            maxLines = 1,
            softWrap = false,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/**
 * The quack chip with a hold: tap quacks, hold rides the duck. detectTapGestures
 * fires onLongPress while the finger is still down, so the release is awaited in
 * onPress: tryAwaitRelease() resumes on lift or cancellation (and on disposal of
 * the pointer input itself), and the finally always lets the ride go, so a held
 * ride cannot outlive the gesture. A plain tap never started one, so its release
 * sends nothing.
 */
@Composable
private fun WheeeChip(
    label: String,
    size: Dp = 52.dp,
    testTag: String? = null,
    onTap: () -> Unit,
    onHold: (Boolean) -> Unit,
) {
    Box(
        Modifier
            .size(size.coerceAtLeast(48.dp))
            .clip(CircleShape)
            .background(Color.White.copy(alpha = 0.10f), CircleShape)
            .pointerInput(Unit) {
                var riding = false
                detectTapGestures(
                    onTap = { onTap() },
                    onLongPress = {
                        riding = true
                        onHold(true)
                    },
                    onPress = {
                        try {
                            tryAwaitRelease()
                        } finally {
                            if (riding) {
                                riding = false
                                onHold(false)
                            }
                        }
                    },
                )
            }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = Color.White.copy(alpha = 0.9f),
            fontSize = 11.sp,
            maxLines = 1,
            softWrap = false,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

// The 13 official robotd emotions ride the same /skills list as the action
// skills. The split lives in the shell; the wire names stay the protocol's.
private val emotionSkills = setOf(
    "angry", "curious", "defiant", "devastated", "excited", "impatient",
    "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
)

@Composable
private fun SkillRing(
    skills: List<String>,
    bottomAnchored: Boolean = false,
    onPick: (String) -> Unit,
    onClose: () -> Unit,
) {
    // Group, don't filter: an unknown future skill falls into Actions rather
    // than disappearing from the ring.
    val actions = skills.filter { it !in emotionSkills }
    val emotions = skills.filter { it in emotionSkills }
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.55f))
            .pointerInput(Unit) { detectTapGestures { onClose() } },
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.safeDrawing),
            contentAlignment = if (bottomAnchored) Alignment.BottomCenter else Alignment.Center,
        ) {
            Column(
                (
                    if (bottomAnchored) {
                        Modifier.fillMaxWidth().fillMaxHeight(0.72f)
                    } else {
                        Modifier.fillMaxHeight(0.86f).widthIn(max = 360.dp)
                    }
                    )
                    .padding(horizontal = 12.dp)
                    .background(Color(0xFF141A22).copy(alpha = 0.96f), RoundedCornerShape(22.dp))
                    // Taps on the panel itself (headings, padding) must not
                    // bubble to the scrim's dismissal: consuming them here
                    // keeps only taps outside the panel closing the ring.
                    .pointerInput(Unit) { detectTapGestures { } }
                    .padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        Copy.skills,
                        color = Color.White,
                        fontWeight = FontWeight.SemiBold,
                        fontSize = 15.sp,
                    )
                    Spacer(Modifier.weight(1f))
                    // The ring's own 48 dp dismissal: tapping the scrim works
                    // too, but a visible target is what a thumb looks for.
                    CloseChip("ring-close", onClose)
                }
                Spacer(Modifier.height(8.dp))
                Column(
                    Modifier
                        .weight(1f, fill = false)
                        .verticalScroll(rememberScrollState()),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (actions.isNotEmpty()) {
                        SkillHeading(Copy.actions)
                        actions.forEach { name -> SkillChip(name, onPick) }
                    }
                    if (emotions.isNotEmpty()) {
                        if (actions.isNotEmpty()) Spacer(Modifier.height(10.dp))
                        SkillHeading(Copy.emotions)
                        emotions.forEach { name -> SkillChip(name, onPick) }
                    }
                    // `sit` and `ground_pick` are already in the skills list
                    // (/skills publishes them now); the ring shows the list,
                    // not a second copy.
                }
            }
        }
    }
}

@Composable
private fun SkillHeading(label: String) {
    Text(
        label,
        color = Color.White.copy(alpha = 0.55f),
        fontSize = 11.sp,
        fontWeight = FontWeight.Medium,
    )
}

@Composable
private fun SkillChip(name: String, onPick: (String) -> Unit) {
    Box(
        Modifier
            .sizeIn(minWidth = 48.dp, minHeight = 48.dp)
            .background(Color.White.copy(alpha = 0.16f), CircleShape)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = skillLabel(name),
                role = Role.Button,
            ) { onPick(name) }
            .testTag("ring-$name")
            .padding(horizontal = 18.dp, vertical = 10.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(skillLabel(name), color = Color.White, fontSize = 13.sp)
    }
}

@Composable
private fun DirectorOverlay(core: DuckCore?, modifier: Modifier = Modifier) {
    Box(modifier) {
        Column(Modifier.fillMaxSize()) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(42.dp)
                    .background(Color.Black.copy(alpha = 0.55f))
            )
            Spacer(Modifier.weight(1f))
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(42.dp)
                    .background(Color.Black.copy(alpha = 0.55f))
            )
        }
        Text(
            Copy.viewfinder,
            color = Color.White.copy(alpha = 0.8f),
            fontSize = 10.sp,
            modifier = Modifier.padding(start = 16.dp, top = 16.dp),
        )
    }
}

/** The instrument sheet: time machine, numbers, sensors, fingerprint. */
/**
 * Hunger, mood and bond, drawn from the core's own numbers. Fullness fills the
 * first bar: a full bar means a fed duck. The nap is a state, not a number.
 */

/**
 * Three quests from the core's real counters. One progress meter per row;
 * text, percent and a checkmark carry the meaning so it never relies on hue.
 */
@Composable
internal fun QuestCard(board: QuestBoardUi) {
    Column(
        Modifier
            .fillMaxWidth()
            .background(Color.White.copy(alpha = 0.05f), RoundedCornerShape(14.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                Copy.quests,
                color = Color.White,
                fontSize = 14.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier
                    .weight(1f)
                    .testTag("quests-title"),
            )
            if (board.streak > 0) {
                Text(
                    Copy.questStreak(board.streak),
                    color = Color(0xFFFFB74D),
                    fontSize = 11.sp,
                )
            }
        }
        if (board.quests.isEmpty()) {
            Text("…", color = Color.White.copy(alpha = 0.6f), fontSize = 12.sp)
        } else {
            board.quests.forEachIndexed { index, quest ->
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(
                        Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            if (quest.done) "✓" else "${index + 1}",
                            color = if (quest.done) Color(0xFF66BB6A) else Color.White.copy(alpha = 0.55f),
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Bold,
                            modifier = Modifier.width(18.dp),
                        )
                        Text(
                            quest.title,
                            color = Color.White.copy(alpha = 0.85f),
                            fontSize = 12.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            if (quest.done) Copy.questDone else "${(quest.progress * 100).roundToInt()}%",
                            color = Color.White.copy(alpha = 0.6f),
                            fontSize = 10.sp,
                            fontFamily = FontFamily.Monospace,
                        )
                    }
                    LinearProgressIndicator(
                        progress = { quest.progress },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(5.dp)
                            .clip(RoundedCornerShape(3.dp))
                            .testTag("quest-progress-$index"),
                        color = if (quest.done) Color(0xFF66BB6A) else Color(0xFF4FC3F7),
                        trackColor = Color.White.copy(alpha = 0.10f),
                    )
                }
            }
            if (board.quests.all { it.done }) {
                Text(
                    Copy.questAllDone,
                    color = Color(0xFF66BB6A),
                    fontSize = 11.sp,
                    modifier = Modifier.testTag("quest-all-done"),
                )
            }
        }
    }
}

@Composable
private fun CareCard(care: CareUi) {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                Copy.care,
                color = Color.White,
                fontSize = 13.sp,
                fontWeight = FontWeight.SemiBold,
            )
            if (care.asleep) {
                Text(
                    Copy.dozing,
                    color = Color.White.copy(alpha = 0.7f),
                    fontSize = 11.sp,
                    modifier = Modifier.testTag("care-sleep"),
                )
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            CareBar(Copy.fullness, care.fullness, Color(0xFFFFB74D), "care-hunger", Modifier.weight(1f))
            CareBar(Copy.mood, care.moodFraction, Color(0xFFF48FB1), "care-mood", Modifier.weight(1f))
            CareBar(Copy.bond, care.bondFraction, Color(0xFF80DEEA), "care-bond", Modifier.weight(1f))
        }
        // The numbers live here, in the instrument: the stage says how the
        // duck feels, the dashboard says how much.
        Text(
            Copy.careLine(care.hunger, care.mood, care.bond, care.asleep),
            color = Color.White.copy(alpha = 0.6f),
            fontSize = 11.sp,
            modifier = Modifier.testTag("care-detail"),
        )
    }
}

/**
 * The night row: one chip to say good night, and while the night runs the
 * quiet it has kept plus the chip that ends it. The chips post the one
 * night verb; neither feeds the duck or otherwise counts as a hand.
 */
@Composable
private fun NightRow(night: NightUi, onNight: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (night.active) {
            Text(
                Copy.nightQuiet((night.quietSeconds / 60.0).roundToInt()),
                color = Color.White.copy(alpha = 0.6f),
                fontSize = 11.sp,
                modifier = Modifier.testTag("night-quiet"),
            )
            ButtonChip(Copy.goodMorning, "night-wake") { onNight(false) }
        } else {
            Spacer(Modifier.weight(1f))
            ButtonChip(Copy.goodNight, "night-sleep") { onNight(true) }
        }
    }
}

@Composable
private fun CareBar(
    label: String,
    value: Float,
    tint: Color,
    tag: String,
    modifier: Modifier = Modifier,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, color = Color.White.copy(alpha = 0.75f), fontSize = 11.sp)
        LinearProgressIndicator(
            progress = { value },
            modifier = Modifier
                .fillMaxWidth()
                .height(6.dp)
                .clip(RoundedCornerShape(3.dp))
                .testTag(tag),
            color = tint,
            trackColor = Color.White.copy(alpha = 0.10f),
        )
        Text(
            "${(value * 100).roundToInt()}%",
            color = Color.White.copy(alpha = 0.6f),
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
        )
    }
}

/**
 * The lifetime record's three headline numbers, and the export. The button
 * writes the core's own Markdown document and hands it to the system chooser;
 * there is no separate counting path in the shell.
 */
@Composable
private fun LifeCard(life: LifeUi, onExport: () -> Unit) {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            Copy.lifeRecord,
            color = Color.White,
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            LifeStat("${life.metres.roundToInt()} m", Copy.walked, Modifier.weight(1f))
            LifeStat("${life.rolls}", Copy.rolls, Modifier.weight(1f))
            LifeStat("${life.pickups}", Copy.pickups, Modifier.weight(1f))
        }
        if (life.days.isNotEmpty()) {
            Text(
                life.days.takeLast(7).joinToString("
"),
                color = Color.White.copy(alpha = 0.6f),
                fontSize = 11.sp,
            )
        }
        ButtonChip(Copy.exportVetbook, "export-vetbook", onExport)
    }
}

@Composable
private fun LifeStat(value: String, label: String, modifier: Modifier = Modifier) {
    Column(
        modifier
            .background(Color.White.copy(alpha = 0.06f), RoundedCornerShape(12.dp))
            .padding(vertical = 10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(value, color = Color.White, fontSize = 19.sp, fontWeight = FontWeight.SemiBold)
        Text(label, color = Color.White.copy(alpha = 0.65f), fontSize = 10.sp)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun InstrumentSheet(
    core: DuckCore,
    input: DuckInput,
    micState: MicState,
    flashlightState: Int,
    onFlashlight: () -> Unit,
    speechFeed: SpeechFeed,
    onToggleSpeech: () -> Unit,
    onToggleMic: () -> Unit,
    quests: QuestBoardUi,
    onPostcards: () -> Unit,
    onDex: () -> Unit,
    onDiary: () -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var showDetails by remember { mutableStateOf(false) }
    var paused by remember { mutableStateOf(core.paused()) }
    var rewinding by remember { mutableStateOf(false) }
    LaunchedEffect(rewinding) {
        while (rewinding) {
            core.seek(200)
            delay(150)
        }
    }
    var speed by remember { mutableStateOf(core.speed()) }
    var branches by remember { mutableStateOf(0) }
    // The duck's own state, sampled while the sheet is up: /state publishes the
    // same numbers for a client, so what the sheet shows is what the API says.
    var status by remember { mutableStateOf(core.status()) }
    LaunchedEffect(core) {
        while (true) {
            status = core.status()
            // A rewind or the rail's chip changes the pause elsewhere; the
            // sheet's label must follow, not stay at its composition value.
            paused = core.paused()
            delay(200)
        }
    }
    // The phone sensors the core last saw, polled at 1 Hz so the microphone row
    // shows what the server published (the same GET /sensors the iOS shell
    // reads). The feed POSTs on its own thread; this only reads the summary
    // back. It lives here, in the sheet's own loop, so it costs nothing while
    // the sheet is closed.
    var micSummary by remember { mutableStateOf("—") }
    // The duck's own behaviour: an emotion timeline, or the idle look-around.
    // A static duck reads as switched off, so the sheet says what it is doing.
    var behaviorSummary by remember { mutableStateOf("—") }
    // The face: the status LED, the four-light battery gauge, the pack
    // percentage and the flashlight, published on /state.leds. A duck with
    // no face is a duck that looks broken, so the sheet draws one.
    var face by remember { mutableStateOf(FaceUi()) }
    // The trick book: how many of its own moods and moves the duck has run.
    var trickBook by remember { mutableStateOf("—") }
    // The companion layer, read from /state.care on the same 1 Hz loop.
    var care by remember { mutableStateOf(CareUi()) }
    // The palm's heartbeat lives IN THIS SHEET: the row is here, so the
    // state and the vibrator are here. A reading of the real care numbers,
    // never a fact; silent until the row says on.
    val heart = remember(context) { Heart(context) }
    var heartOn by remember { mutableStateOf(
        context.getSharedPreferences("duck", Context.MODE_PRIVATE).getBoolean("duck_heart_on", false)
    ) }
    LaunchedEffect(heartOn) {
        heart.setEnabled(heartOn)
        context.getSharedPreferences("duck", Context.MODE_PRIVATE)
            .edit().putBoolean("duck_heart_on", heartOn).apply()
    }
    // The chosen night, the top-level /state.night beside that care: the
    // row says whether the duck is down for the night and how quietly.
    var night by remember { mutableStateOf(NightUi()) }
    // The vet book's three headline numbers, from the same /state.life.
    // The move-out flow: the SAF picker's verdict lands as a status line.
    var backupStatus by remember { mutableStateOf<String?>(null) }
    val backupPicker = rememberLauncherForActivityResult(
        androidx.activity.compose.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            backupStatus = Copy.backupWorking
            Thread {
                val ok = restoreBackup(context, uri)
                scope.launch { backupStatus = if (ok) Copy.backupDone else Copy.backupFailed }
            }.start()
        }
    }
    var lifeRecord by remember { mutableStateOf(LifeUi()) }
    // What the duck logged while the app was away; remembered across closes
    // by hoisting it to the activity's state via a remembered ref.
    var awaySummary by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) {
            val current = fetchLifeEvents()
            val known = AwayStore.atBackground
            if (known != null) {
                val fresh = current.filter { it !in known }
                if (fresh.isNotEmpty()) {
                    val parts = mutableListOf<String>()
                    val rituals = fresh.count { it == "idle_ritual" }
                    val falls = fresh.count { isFall(it) }
                    val feeds = fresh.count { it == "feed" }
                    val moods = fresh.count { it.startsWith("emotion:") }
                    if (rituals > 0) parts.add(Copy.awayRituals(rituals))
                    if (falls > 0) parts.add(Copy.awayFalls(falls))
                    if (feeds > 0) parts.add(Copy.awayFeeds(feeds))
                    if (moods > 0) parts.add(Copy.awayMoods(moods))
                    if (parts.isNotEmpty()) withContext(Dispatchers.Main) {
                        awaySummary = parts.joinToString(" · ")
                    }
                }
            }
        }
    }
    LaunchedEffect(core) {
        while (true) {
            micSummary = fetchMicSummary()
            behaviorSummary = fetchBehaviorSummary()
            face = fetchFace()
            trickBook = fetchTrickBook()
            fetchCare()?.let {
                care = it
                // The palm reads the same numbers this loop already fetched.
                heart.tick(it.heartBpm, System.currentTimeMillis())
            }
            // The night rides the same /state: the row is only ever what the
            // core published, and a failed read keeps the last one.
            night = withContext(Dispatchers.IO) { fetchNight() } ?: night
            fetchLifeRecord()?.let { lifeRecord = it }
            branches = withContext(Dispatchers.IO) { fetchBranchCount() }
            delay(1000)
        }
    }

    // One night verb: the chip posts, and the row shows what the core
    // accepted. The 1 Hz loop above is the fallback when the tick is slow.
    val setNight: (Boolean) -> Unit = { on ->
        scope.launch {
            val queued = withContext(Dispatchers.IO) {
                postJson("/night", org.json.JSONObject().put("on", on).toString())
            }
            if (queued) withContext(Dispatchers.IO) { fetchNight() }?.let { night = it }
        }
    }

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        // The sheet outgrew a phone screen, so its column scrolls and the
        // dense status text collapses behind one chip.
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.thisDuck,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("sheet-title"),
                )
                // A deterministic dismissal: a portrait medium detent would
                // otherwise make a swipe-down scroll the sheet instead.
                CloseChip("sheet-close", onClose)
            }
            ButtonChip(if (showDetails) Copy.showLess else Copy.showMore) { showDetails = !showDetails }
            QuestCard(quests)
            CareCard(care)
            NightRow(night, setNight)
            if (showDetails && status.size >= 14) {
                Text(
                    buildString {
                        append(Copy.bringupText(status[0].toInt()))
                        append(" · ")
                        append(if (status[9] > 0.5) Copy.imu else Copy.imuWarming)
                        // [0 bringup, 1 fallen, 2 gain, 3..5 trunk, 6..8 gravity,
                        //  9 imu_ready, 10 picked_up, 11 p_held, 12 mouth, 13 volts,
                        //  14 has_mouth, 15 fps, 16 ticks/s]
                        append(" · ${Copy.mouth} ${"%.2f".format(status[12])}")
                        if (status[10] > 0.5) append(" · ${Copy.held}")
                        if (status[13] > 0.0) append(" · ${Copy.battery} ${"%.2f".format(status[13])} V")
                        if ((status.getOrNull(15) ?: 0.0) > 0.0) append(
                            " · ${"%.0f".format(status[15])} fps · ${"%.1f".format(status[16])} Hz"
                        )
                    },
                    color = Color.White.copy(alpha = 0.65f),
                    fontSize = 11.sp,
                )
            }
            // Microphone: the room, not a sense. Off (default) shows "—"; enabling
            // asks RECORD_AUDIO first, denied shows the bilingual denial, granted
            // starts the feed whose 1 Hz summary is read back below.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.microphone,
                    color = Color.White.copy(alpha = 0.75f),
                    fontSize = 12.sp,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        when (micState) {
                            MicState.Off -> "—"
                            MicState.Denied -> Copy.denied
                            MicState.On -> micSummary
                        },
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    SurfaceChip(if (micState == MicState.On) "ON" else "OFF", micState == MicState.On) {
                        onToggleMic()
                    }
                }
            }
            // Spoken commands: a separate toggle from the room mic. Off until
            // tapped; enabling asks RECORD_AUDIO. The last final transcript is
            // the row's reading, so what the duck heard is visible next to the
            // skill it probably just performed. Unavailable/denied are shown,
            // not thrown: CI has no recognizer and must still render the sheet.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.voiceCommands,
                    color = Color.White.copy(alpha = 0.75f),
                    fontSize = 12.sp,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        when (speechFeed.state) {
                            SpeechState.Off -> speechFeed.lastHeard ?: "—"
                            SpeechState.Listening -> speechFeed.lastHeard ?: Copy.speechListening
                            SpeechState.Denied -> Copy.denied
                            SpeechState.Unavailable -> Copy.speechUnavailable
                        },
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    SurfaceChip(
                        Copy.voiceEar,
                        speechFeed.state == SpeechState.Listening,
                        testTag = "sheet-voice-toggle",
                    ) {
                        onToggleSpeech()
                    }
                }
            }
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.behavior, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Text(
                    behaviorSummary,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 11.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
            }
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.trickBook, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Text(
                    trickBook,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 11.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
            }
            // The token a tool on another machine has to send. The app itself
            // never needs it — it reaches the duck over the loopback address
            // — but a parent pointing an AI client at this phone does.
            val token = core.intentToken()
            if (token.isNotEmpty()) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(Copy.apiToken, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                    Text(
                        token,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 10.sp,
                        maxLines = 3,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                }
            }
            LifeCard(lifeRecord) { shareVetbook(context) }
            ButtonChip(Copy.postcardShelf, "postcards-open", onPostcards)
            ButtonChip(Copy.dex, "dex-open", onDex)
            ButtonChip(Copy.diary, "diary-open", onDiary)
            awaySummary?.let { summary ->
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(Copy.awayTitle, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                    Text(
                        summary,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                }
            }
            // The face: the status LED's verdict, the four-light battery gauge
            // and the pack percentage, all read back from /state.leds.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.face, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Text(
                        Copy.faceStatus(face.status),
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                    )
                    Text(
                        gaugeBars(face.bars),
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                    )
                    Text(
                        face.percent,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                    )
                }
            }
            // The flashlight: the face's own LED, the same seven colours the
            // duck has. One tap flips it; the sheet shows which one is lit.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.flashlight, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        face.color?.let { Copy.faceColor(it) }
                            ?: if (flashlightState > 0) Copy.faceColor("white") else "—",
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                    )
                    Spacer(Modifier.size(8.dp))
                    ButtonChip(Copy.flashlight) { onFlashlight() }
                }
            }
            // The palm's heartbeat: one tap flips it; the bpm is the core's
            // own reading, and a device with no vibrator shows the number
            // without pretending to pulse.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.heart, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (heartOn && heart.bpm > 0) "%.0f bpm".format(heart.bpm) else Copy.heartRowNote,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.size(8.dp))
                    ButtonChip(
                        if (heartOn) Copy.flashlightOff else Copy.flashlightOn,
                        testTag = "heart-toggle",
                    ) { heartOn = !heartOn }
                }
            }
            // The room's lamp: real lux when the device has a sensor (the
            // scale follows the room), or five manual stops. The duck cannot
            // see it — render layer only.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.roomLamp, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (input.ambientLux >= 0f) "${input.ambientLux.roundToInt()} lx"
                        else "${(input.ambientScale * 100).roundToInt()}%",
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.size(8.dp))
                    ButtonChip(
                        if (input.ambientLux >= 0f) Copy.flashlightOff else Copy.roomLamp,
                        "lamp-toggle",
                    ) {
                        // Manual stops, only when there is no sensor to trust.
                        if (input.ambientLux < 0f) {
                            val stops = listOf(0.55f, 0.7f, 0.85f, 1.0f, 1.15f)
                            input.ambientScale =
                                stops[(stops.indexOf(input.ambientScale.coerceIn(0.55f, 1.15f)) + 1) % stops.size]
                        }
                    }
                }
            }
            // The compass: the home keeps one real-world bearing when the
            // device has a rotation vector. Off by default - a rotating
            // room can confuse a driving child - and honest when absent.
            var compassOn by remember { mutableStateOf(
                context.getSharedPreferences("duck", Context.MODE_PRIVATE)
                    .getBoolean("duck_compass_on", false)
            ) }
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.compass, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                ButtonChip(
                    if (compassOn) Copy.flashlightOff else Copy.flashlightOn,
                    "compass-toggle",
                ) {
                    compassOn = !compassOn
                    context.getSharedPreferences("duck", Context.MODE_PRIVATE)
                        .edit().putBoolean("duck_compass_on", compassOn).apply()
                }
            }
            // Back up & move: export the core's document, or import one the
            // person carried here. The route refuses anything without its
            // version at the door, and the sheet says what the duck said.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.backup, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        backupStatus ?: Copy.backupNote,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 10.sp,
                        maxLines = 2,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.size(8.dp))
                    ButtonChip(Copy.backupExport, "backup-export") {
                        backupStatus = Copy.backupWorking
                        exportBackup(context)
                    }
                    Spacer(Modifier.size(8.dp))
                    ButtonChip(Copy.backupImport, "backup-import") { backupPicker = true }
                }
            }
            // The parent's daily allowance: one chip cycles off -> 15 -> 30
            // -> 60 minutes. The number shown is the core's own accounting.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.parentTime, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        care.parentMinutes?.let { minutes ->
                            Copy.parentLeft(minutes, (care.parentLeftS / 60.0).toInt())
                        } ?: Copy.parentOff,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.size(8.dp))
                    ButtonChip(
                        (listOf(0, 15, 30, 60).indexOf(care.parentMinutes ?: 0) + 1)
                            .let { listOf(15, 30, 60, 0)[it % 4] }
                            .let { if (it == 0) Copy.parentOff else "$it" },
                        "parent-toggle",
                    ) {
                        val next = when (care.parentMinutes) {
                            null, 0 -> 15
                            15 -> 30
                            30 -> 60
                            else -> 0
                        }
                        postDuckCommand("/parent", """{"minutes":$next}""")
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(
                    if (paused) Copy.resume else Copy.freeze,
                    testTag = if (paused) "time-play.fill" else "time-pause.fill",
                ) {
                    paused = !paused
                    core.setPaused(paused)
                }
                ButtonChip(Copy.step) { core.step() }
                // Rewind-play: while on, the duck walks its own history
                // backwards, 200 ticks a pop - the same /seek the rewind
                // chip uses, just continuously.
                ButtonChip(
                    if (rewinding) Copy.flashlightOff else Copy.rewindPlay,
                    "rewind-play",
                ) { rewinding = !rewinding }
                // Push & compare: checkpoint, fork, and push branch B -
                // the split the rewind machine already keeps, one tap.
                ButtonChip(Copy.pushCompare, "push-compare") {
                    core.checkpoint()
                    core.fork()
                    postDuckCommand("/push", """{"vx":0.6,"vy":0.0,"vz":0.0}""")
                    Toast.makeText(context, Copy.pushCompareDone, Toast.LENGTH_SHORT).show()
                }
                ButtonChip(Copy.rewind) { core.seek(200) }
                ButtonChip(Copy.fork) {
                    core.fork()
                    // The JNI call gives no verdict; the count is the truth.
                    scope.launch {
                        branches = withContext(Dispatchers.IO) { fetchBranchCount() }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(Copy.drive + ": " + if (core.mode() == "roller") Copy.roller else Copy.walk) {
                    core.setMode(if (core.mode() == "roller") "walk" else "roller")
                }
            }
            // Policy A/B: fork, then load a policy DIRECTORY on branch B and
            // switch to it. The directories live beside the bundled policies;
            // a folder the manifest cannot read is the reload route's own
            // honest refusal.
            var abFolder by remember { mutableStateOf("") }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(Copy.abTry, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Spacer(Modifier.size(8.dp))
                OutlinedTextField(
                    value = abFolder,
                    onValueChange = { abFolder = it.take(64) },
                    singleLine = true,
                    modifier = Modifier.weight(1f).testTag("ab-folder"),
                )
                Spacer(Modifier.size(8.dp))
                ButtonChip(Copy.flashlightOn, "ab-load") {
                    core.fork()
                    core.reload(abFolder)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(
                    if (input.hudOn) Copy.flashlightOff else Copy.hud,
                    "hud-toggle",
                ) { input.hudOn = !input.hudOn }
                ButtonChip(
                    if (input.p2On) Copy.flashlightOff else Copy.twoPlayer,
                    "p2-toggle",
                ) { input.p2On = !input.p2On }
            }
            Text(Copy.abNote, color = Color.White.copy(alpha = 0.55f), fontSize = 10.sp)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(0.25, 0.5, 1.0, 2.0).forEach { option ->
                    ButtonChip(if (option == 1.0) "1×" else "${option}×") {
                        speed = option
                        core.setSpeed(option)
                    }
                }
            }
            Text(
                "speed ${speed}× · ${Copy.branches} $branches · AI API :8765 · MCP /mcp",
                color = Color.White.copy(alpha = 0.6f),
                fontSize = 11.sp,
            )
        }
    }
}

@Composable
internal fun ButtonChip(
    label: String,
    testTag: String? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            // A 48 dp floor: the sheet's controls were ~34 dp tall.
            .heightIn(min = 48.dp)
            .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(10.dp))
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = label,
                role = Role.Button,
            ) { onClick() }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier)
            .padding(horizontal = 14.dp, vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White, fontSize = 12.sp)
    }
}

/**
 * A path chip: the ButtonChip language with an enabled face. The click stays
 * wired (Role.Button, named) and simply does nothing while disabled, so
 * TalkBack never loses the control.
 */
@Composable
private fun PathChip(
    label: String,
    enabled: Boolean = true,
    testTag: String? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            // A 48 dp floor, the same one every chip got.
            .heightIn(min = 48.dp)
            .background(
                Color.White.copy(alpha = if (enabled) 0.10f else 0.05f),
                RoundedCornerShape(10.dp),
            )
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = label,
                role = Role.Button,
            ) { if (enabled) onClick() }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier)
            .padding(horizontal = 14.dp, vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White.copy(alpha = if (enabled) 1f else 0.4f), fontSize = 12.sp)
    }
}

/**
 * Path mode's one row. Drawing: walk it — real only when two or more
 * waypoints would survive the core's own thinning — and clear. Walking: the
 * step count and the stop chip, because drawing is off until the core says
 * the walk is over.
 */
@Composable
private fun PathBar(
    drawing: Boolean,
    canGo: Boolean,
    active: Boolean,
    reached: Int,
    total: Int,
    onGo: () -> Unit,
    onClear: () -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    if (!drawing && !active) return
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (active) {
            Text(
                // One-based, like iOS: the child is walking the
                // (reached+1)-st of the total steps.
                Copy.pathStep(reached + 1, total),
                color = Color.White.copy(alpha = 0.75f),
                fontSize = 12.sp,
                maxLines = 1,
                modifier = Modifier
                    .weight(1f)
                    .testTag("path-progress"),
            )
            PathChip(Copy.pathCancel, testTag = "path-cancel", onClick = onCancel)
        } else {
            Spacer(Modifier.weight(1f))
            PathChip(Copy.pathGo, enabled = canGo, testTag = "path-go", onClick = onGo)
            PathChip(Copy.pathClear, testTag = "path-clear", onClick = onClear)
        }
    }
}

/**
 * The stroke over the stage. Before the walk it is what the child drew; once
 * the core is walking, it re-projects the accepted body-frame waypoints back
 * through the draw mapping (the stroke's start and the metres-per-pixel the
 * draw used), so a rotation mid-walk drifts until the next stroke. Reached
 * waypoints go dim and the rest stay bright — the same reading as the
 * progress text beside them.
 */
@Composable
private fun PathOverlay(
    drawn: List<Offset>,
    active: Boolean,
    accepted: List<List<Double>>,
    reached: Int,
    origin: Offset,
    mpp: Float,
    modifier: Modifier = Modifier,
) {
    Canvas(modifier) {
        if (!active && drawn.isNotEmpty()) {
            val stroke = Path()
            stroke.moveTo(drawn.first().x, drawn.first().y)
            drawn.drop(1).forEach { stroke.lineTo(it.x, it.y) }
            drawPath(
                stroke,
                color = Color(0xFF4FC3F7),
                style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round),
            )
            drawn.forEach { drawCircle(Color(0xFF4FC3F7).copy(alpha = 0.7f), 3.dp.toPx(), it) }
        } else if (active && mpp > 0f && accepted.isNotEmpty()) {
            // Metres back to pixels through the inverse of the draw mapping.
            val walk = Path()
            accepted.forEachIndexed { index, point ->
                val here = Offset(
                    origin.x - (point[1] / mpp).toFloat(),
                    origin.y - (point[0] / mpp).toFloat(),
                )
                if (index == 0) walk.moveTo(here.x, here.y) else walk.lineTo(here.x, here.y)
                drawCircle(
                    if (index < reached) Color.White.copy(alpha = 0.25f) else Color(0xFF4FC3F7),
                    3.dp.toPx(),
                    here,
                )
            }
            drawPath(
                walk,
                color = Color(0xFF4FC3F7).copy(alpha = 0.8f),
                style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round),
            )
        }
    }
}

enum class MicState { Off, On, Denied }

/**
 * The duck's face, read from GET /state: the status LED's verdict, the
 * four-light battery gauge, the pack percentage and the flashlight's colour.
 * The default is the "no reading yet" face: no verdict, no bars, no percent.
 */
private data class FaceUi(
    val status: String = "—",
    val bars: Int = 0,
    val percent: String = "—",
    val color: String? = null,
)

/// The events the duck had logged when the app last went to the background.
/// Process-level: the activity's onPause writes it, the sheet's first
/// composition after onResume reads it.
object AwayStore {
    @Volatile var atBackground: Set<String>? = null
}

/// The duck's event log, from GET /state -> life.events. Null when the
/// server is not up.
internal fun fetchLifeEvents(): List<String> {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        // Parsed as JSON, not scraped with a regular expression and split
        // on commas: a comma inside one event used to split it in two, and
        // the diary counted the halves.
        val events = org.json.JSONObject(text).optJSONObject("life")?.optJSONArray("events")
            ?: return emptyList()
        (0 until events.length()).mapNotNull { index ->
            if (events.isNull(index)) null else events.optString(index, null)
        }
    } catch (_: Exception) {
        emptyList()
    }
}

/// The duck's branch count, from GET /branches. Zero when the server is not
/// up.
internal fun fetchBranchCount(): Int {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/branches")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        org.json.JSONObject(text).optJSONArray("branches")?.length() ?: 0
    } catch (_: Exception) {
        0
    }
}

/**
 * The face's own numbers, from GET /state: the `leds` object the core
 * publishes once a second. Any failure (server not up yet, no reading) gives
 * the empty face rather than throwing, so the sheet's poll loop never breaks.
 */
private fun fetchFace(): FaceUi {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        // The leds block is flat: no nested braces, so `[^}]*` stops at its
        // own end. Scoping to it keeps the pack's own battery_percent (a
        // different object) out of the reading.
        val leds = Regex(""""leds"\s*:\s*\{[^}]*\}""").find(text)?.value
            ?: return FaceUi()
        val status = Regex(""""status"\s*:\s*"([^"]+)"""")
            .find(leds)?.groupValues?.get(1) ?: "—"
        val bars = Regex(""""bars"\s*:\s*([0-9]+)""")
            .find(leds)?.groupValues?.get(1)?.toIntOrNull() ?: 0
        val raw = Regex(""""battery_percent"\s*:\s*([0-9.]+)""")
            .find(leds)?.groupValues?.get(1)?.toDoubleOrNull()
        val percent = if (raw == null) "—" else "%.0f%%".format(raw)
        val color = Regex(""""color"\s*:\s*"([^"]+)"""")
            .find(leds)?.groupValues?.get(1)
        FaceUi(status, bars, percent, color)
    } catch (_: Throwable) {
        FaceUi()
    }
}

/** The trick book, from the same /state the sheet already polls. */
private fun fetchTrickBook(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        // org.json, not a regex: the life object holds nested maps whose
        // braces defeat a flat pattern.
        val life = org.json.JSONObject(text).optJSONObject("life") ?: return "—"
        val emotions = life.optJSONObject("emotions")
        val skills = life.optJSONObject("skills")
        var tricks = 0L
        if (skills != null) {
            val names = skills.keys()
            while (names.hasNext()) tricks += skills.optLong(names.next())
        }
        "${emotions?.length() ?: 0}/13 moods · $tricks tricks"
    } catch (_: Throwable) {
        "—"
    }
}

/** The battery gauge as text: one filled circle per lit LED of the four. */
private fun gaugeBars(bars: Int): String {
    val lit = bars.coerceIn(0, 4)
    return buildString {
        repeat(4) { index -> append(if (index < lit) '●' else '○') }
    }
}

/** One line from GET /state: the emotion playing, or the idle look-around. */
private fun fetchBehaviorSummary(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        val emotion = Regex(""""emotion"\s*:\s*\{[^}]*\}""").find(text)?.value
        val name = emotion?.let { Regex(""""name"\s*:\s*"([^"]+)"""").find(it)?.groupValues?.get(1) }
        if (name != null) {
            val elapsed = Regex(""""elapsed_s"\s*:\s*([0-9.]+)"""").find(emotion)?.groupValues?.get(1) ?: "0"
            val duration = Regex(""""duration_s"\s*:\s*([0-9.]+)"""").find(emotion)?.groupValues?.get(1) ?: "0"
            // The wire name is `yes_fast`; the sheet says what a person calls
            // it. The clock is the instrument's own reading.
            return "${skillLabel(name)} $elapsed/${duration}s"
        }
        val gain = Regex(""""idle"\s*:\s*\{[^}]*\}""").find(text)?.value
            ?.let { Regex(""""takeover"\s*:\s*([0-9.]+)"""").find(it)?.groupValues?.get(1)?.toDoubleOrNull() }
        if (gain != null && gain > 0.1) return Copy.lookingAround((gain * 100).toInt())
        "—"
    } catch (_: Throwable) {
        "—"
    }
}

/**
 * The one-line summary the core last published for the microphone, read from
 * GET /sensors. The feed POSTs `{rms, peak_hz}` once a second; this formats
 * them the way the iOS shell does. Any failure (server not up yet, no reading)
 * returns "—" rather than throwing, so the sheet's poll loop never breaks.
 */
private fun fetchMicSummary(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/sensors")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        val microphone = Regex(""""microphone"\s*:\s*\{[^}]*\}""").find(text)?.value
            ?: return "—"
        val rms = Regex("\"rms\"\\s*:\\s*([0-9.]+)").find(microphone)?.groupValues?.get(1)
            ?: return "—"
        val peak = Regex("\"peak_hz\"\\s*:\\s*([0-9.]+)").find(microphone)?.groupValues?.get(1)
            ?: return "—"
        "rms $rms · peak_hz $peak"
    } catch (_: Throwable) {
        "—"
    }
}
