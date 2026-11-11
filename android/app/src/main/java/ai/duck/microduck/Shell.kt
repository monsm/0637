package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Handler
import android.os.Looper
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.changedToUp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import kotlin.math.hypot
import kotlin.math.min
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
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    var showRing by remember { mutableStateOf(false) }
    var showSheet by remember { mutableStateOf(false) }
    var carrying by remember { mutableStateOf(false) }
    // The pad's D-pad modes, reduced to what a phone can do: drive, look,
    // and crouch/lean the standing body.
    var stick by remember { mutableStateOf("drive") }
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
            delay(4000)
            hintDismiss = false
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
                    TopBar(
                        core = duck,
                        carrying = carrying,
                        lastSkill = lastSkill,
                        menuOpen = menuOpen,
                        onMenuOpen = { menuOpen = it },
                        onStick = { stick = it },
                        onSurface = onSurface,
                        onFlashlight = toggleFlashlight,
                        onLanguage = {
                            Copy.language = if (Copy.language == Copy.Language.English) {
                                Copy.Language.Chinese
                            } else {
                                Copy.Language.English
                            }
                            languageTick += 1
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                    // The stage. Steering is translation-based from wherever
                    // the thumb lands: a hidden centre-origin joystick makes
                    // a resting thumb command a reversal.
                    Spacer(
                        Modifier
                            .weight(1f)
                            .fillMaxWidth()
                            .portraitStageGesture(
                                core = duck,
                                input = input,
                                carrying = carrying,
                                stick = stick,
                                onLongPress = { showRing = true },
                            ),
                    )
                    // The one-time gesture hint lives in the column, above
                    // the rail: no magic bottom offset to drift onto a button.
                    AnimatedVisibility(visible = hintDismiss) {
                        Text(
                            Copy.dragHint,
                            color = Color.White.copy(alpha = 0.7f),
                            fontSize = 12.sp,
                            modifier = Modifier.padding(bottom = 6.dp),
                        )
                    }
                    PortraitTimeRail(
                        core = duck,
                        surface = surface,
                        onSurface = onSurface,
                        onOpenSheet = { showSheet = true },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp),
                    )
                    ThumbDeck(
                        core = duck,
                        carrying = carrying,
                        onCarrying = { carrying = it },
                        quack = quack,
                        onFeed = onFeed,
                        onWheee = onWheee,
                        onSkills = { showRing = true },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                }
            }
        } else {
            // Driving: a drag on the scene becomes the twist the real pad sends.
            // While the duck is held, the same drag moves the hand — a carry is an
            // external force, and the pickup classifier decides what the policy
            // does about it.
            Box(
                Modifier
                    .fillMaxSize()
                    .pointerInput(core, carrying, stick) {
                        val duck = core ?: return@pointerInput
                        var start = Offset.Zero
                        detectDragGestures(
                            onDragStart = { start = it },
                            onDrag = { change, _ ->
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
                                when (stick) {
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

            if (surface == Surface.Director) {
                DirectorOverlay(core, Modifier.fillMaxSize())
            }

            // Top bar: the duck's state, and ONE menu. Six text chips in a row
            // was a desktop panel crammed onto a phone: on a small landscape
            // screen they ran off the edge and nothing was legible. The stick
            // modes, the surface, the language and the flashlight now live in a
            // menu that a thumb can open.
            TopBar(
                core = core,
                carrying = carrying,
                lastSkill = lastSkill,
                menuOpen = menuOpen,
                onMenuOpen = { menuOpen = it },
                onStick = { stick = it },
                onSurface = onSurface,
                onFlashlight = toggleFlashlight,
                onLanguage = {
                    Copy.language = if (Copy.language == Copy.Language.English) {
                        Copy.Language.Chinese
                    } else {
                        Copy.Language.English
                    }
                    languageTick += 1
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
                Copy.dragHint,
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 12.sp,
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
        ) { showSheet = false }
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
    menuOpen: Boolean,
    onMenuOpen: (Boolean) -> Unit,
    onStick: (String) -> Unit,
    onSurface: (Surface) -> Unit,
    onFlashlight: () -> Unit,
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
                    else -> Copy.live
                },
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 11.sp,
                modifier = Modifier.testTag("duck-label"),
            )
            if (lastSkill.isNotEmpty()) {
                Text(
                    lastSkill,
                    color = Color.White.copy(alpha = 0.55f),
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
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
                    text = { Text(Copy.language.label, color = Color.White) },
                    onClick = { onLanguage(); onMenuOpen(false) },
                )
            }
        }
    }
}

/**
 * The 48 dp time rail: the time machine button (tick + retention bar), the
 * rail's own freeze/resume, and the camera/Director toggle that moved down
 * from the portrait top bar.
 */
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
    var paused by remember { mutableStateOf(core.paused()) }
    LaunchedEffect(core) {
        while (true) {
            tick = core.tickIndex()
            oldest = core.oldestTick()
            paused = core.paused()
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
                .pointerInput(Unit) { detectTapGestures { onOpenSheet() } }
                .testTag("duck-time")
                .padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "t $tick",
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                softWrap = false,
            )
            Box(
                Modifier
                    .weight(1f)
                    .height(3.dp)
                    .background(Color.White.copy(alpha = 0.08f), CircleShape),
            ) {
                Box(
                    Modifier
                        .fillMaxWidth(fraction.toFloat())
                        .fillMaxHeight()
                        .background(Color(0xFF4FC3F7).copy(alpha = 0.6f), CircleShape),
                )
            }
        }
        RoundChip(
            if (paused) Copy.resume else Copy.freeze,
            size = 48.dp,
            selected = paused,
            selectedColor = Color(0xFFFFA726).copy(alpha = 0.28f),
            testTag = "rail-pause",
        ) {
            paused = !paused
            core.setPaused(paused)
        }
        RoundChip(
            if (surface == Surface.Live) Copy.director else Copy.live,
            size = 48.dp,
            testTag = "surface-toggle",
        ) {
            onSurface(if (surface == Surface.Live) Surface.Director else Surface.Live)
        }
    }
}

/**
 * The right-weighted thumb deck: Feed, Skills, Quack and Grab. Grab is the
 * largest target and turns cyan with "Drop" while the duck is carried, the
 * same affordance the wide layout gives the corner chip.
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
    modifier: Modifier = Modifier,
) {
    Row(
        modifier
            .height(76.dp)
            .background(Color(0xFF141A22).copy(alpha = 0.94f), RoundedCornerShape(28.dp))
            .padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Spacer(Modifier.weight(1f))
        RoundChip(Copy.feed, size = 52.dp, testTag = "duck-feed") { onFeed?.invoke() }
        RoundChip(Copy.skills, size = 52.dp, testTag = "duck-skills") { onSkills() }
        WheeeChip(
            Copy.quack,
            size = 56.dp,
            testTag = "duck-quack",
            onTap = { quack?.invoke() },
            onHold = { onWheee?.invoke(it) },
        )
        RoundChip(
            if (carrying) Copy.putDown else Copy.grab,
            size = 64.dp,
            selected = carrying,
            selectedColor = Color.Cyan.copy(alpha = 0.35f),
            testTag = "duck-grab",
        ) {
            val next = !carrying
            onCarrying(next)
            core.carry(0.0, 0.0, 0.0, next)
        }
    }
}

/** A 48 dp close target shared by the ring and the instrument sheet. */
@Composable
private fun CloseChip(tag: String, onClose: () -> Unit, size: Dp = 48.dp) {
    Box(
        Modifier
            .size(size.coerceAtLeast(48.dp))
            .background(Color.White.copy(alpha = 0.10f), CircleShape)
            .pointerInput(Unit) { detectTapGestures { onClose() } }
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
 */
private fun Modifier.portraitStageGesture(
    core: DuckCore,
    input: DuckInput,
    carrying: Boolean,
    stick: String,
    onLongPress: () -> Unit,
): Modifier = pointerInput(core, carrying, stick) {
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
internal fun postDuckCommand(path: String) {
    Thread {
        try {
            val connection = java.net.URL("http://127.0.0.1:8765$path")
                .openConnection() as java.net.HttpURLConnection
            connection.requestMethod = "POST"
            connection.connectTimeout = 2000
            connection.readTimeout = 5000
            connection.doOutput = true
            connection.outputStream.use { it.write("{}".toByteArray()) }
            connection.inputStream.readBytes()
            connection.disconnect()
        } catch (_: Exception) {
            // The loop refuses when busy; nothing to show.
        }
    }.start()
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
            .pointerInput(Unit) { detectTapGestures { onClick() } }
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
            .pointerInput(Unit) { detectTapGestures { onClick() } }
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
            .pointerInput(Unit) { detectTapGestures { onPick(name) } }
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
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun InstrumentSheet(
    core: DuckCore,
    micState: MicState,
    flashlightState: Int,
    onFlashlight: () -> Unit,
    speechFeed: SpeechFeed,
    onToggleSpeech: () -> Unit,
    onToggleMic: () -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    var showDetails by remember { mutableStateOf(false) }
    var paused by remember { mutableStateOf(core.paused()) }
    var speed by remember { mutableStateOf(core.speed()) }
    var branches by remember { mutableStateOf(0) }
    // The duck's own state, sampled while the sheet is up: /state publishes the
    // same numbers for a client, so what the sheet shows is what the API says.
    var status by remember { mutableStateOf(core.status()) }
    LaunchedEffect(core) {
        while (true) {
            status = core.status()
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
                    val falls = fresh.count { it.startsWith("fall") }
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
            delay(1000)
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
            if (showDetails && status.size >= 14) {
                Text(
                    buildString {
                        append(Copy.bringup(status[0].toInt()))
                        append(" · ")
                        append(if (status[9] > 0.5) "IMU warm" else "IMU warming")
                        // [0 bringup, 1 fallen, 2 gain, 3..5 trunk, 6..8 gravity,
                        //  9 imu_ready, 10 picked_up, 11 p_held, 12 mouth, 13 volts,
                        //  14 has_mouth, 15 fps, 16 ticks/s]
                        append(" · beak ${"%.2f".format(status[12])}")
                        if (status[10] > 0.5) append(" · in a hand")
                        if (status[13] > 0.0) append(" · pack ${"%.2f".format(status[13])} V")
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
                            SpeechState.Listening -> speechFeed.lastHeard ?: Copy.listening
                            SpeechState.Denied -> Copy.denied
                            SpeechState.Unavailable -> Copy.unavailable
                        },
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = 11.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    SurfaceChip(
                        if (speechFeed.state == SpeechState.Listening) "ON" else "OFF",
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
                Text(Copy.flashlightRow, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
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
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(
                    if (paused) Copy.resume else Copy.freeze,
                    testTag = if (paused) "time-play.fill" else "time-pause.fill",
                ) {
                    paused = !paused
                    core.setPaused(paused)
                }
                ButtonChip(Copy.step) { core.step() }
                ButtonChip(Copy.rewind) { core.seek(200) }
                ButtonChip(Copy.fork) { branches += 1 }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(Copy.drive + ": " + if (core.mode() == "roller") Copy.roller else Copy.walk) {
                    core.setMode(if (core.mode() == "roller") "walk" else "roller")
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(0.25, 0.5, 1.0, 2.0).forEach { option ->
                    ButtonChip(if (option == 1.0) "1×" else "${option}×") {
                        speed = option
                        core.setSpeed(option)
                    }
                }
            }
            Text(
                "speed ${speed}× · branches $branches · AI API :8765 · MCP /mcp",
                color = Color.White.copy(alpha = 0.6f),
                fontSize = 11.sp,
            )
        }
    }
}

@Composable
private fun ButtonChip(
    label: String,
    testTag: String? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            // A 48 dp floor: the sheet's controls were ~34 dp tall.
            .heightIn(min = 48.dp)
            .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(10.dp))
            .pointerInput(Unit) { detectTapGestures { onClick() } }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier)
            .padding(horizontal = 14.dp, vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White, fontSize = 12.sp)
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
        Regex(""""events"\s*:\s*\[(.*?)]""", RegexOption.DOT_MATCHES_ALL)
            .find(text)?.groupValues?.get(1)
            ?.split(",")
            ?.mapNotNull { entry ->
                Regex(""""([^"]+)"""").find(entry)?.groupValues?.get(1)
            } ?: emptyList()
    } catch (_: Exception) {
        emptyList()
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
            return "$name $elapsed/${duration}s"
        }
        val gain = Regex(""""idle"\s*:\s*\{[^}]*\}""").find(text)?.value
            ?.let { Regex(""""takeover"\s*:\s*([0-9.]+)"""").find(it)?.groupValues?.get(1)?.toDoubleOrNull() }
        if (gain != null && gain > 0.1) return "looking around ${(gain * 100).toInt()}%"
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
