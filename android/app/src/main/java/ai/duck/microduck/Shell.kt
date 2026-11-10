package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import kotlinx.coroutines.delay

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
    Box(modifier.fillMaxSize()) {
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
                },
        )

        if (surface == Surface.Director) {
            DirectorOverlay(core, Modifier.fillMaxSize())
        }

        // Top bar: the duck's state, and ONE menu. Six text chips in a row
        // was a desktop panel crammed onto a phone: on a small landscape
        // screen they ran off the edge and nothing was legible. The stick
        // modes, the surface, the language and the flashlight now live in a
        // menu that a thumb can open.
        Row(
            Modifier
                .align(Alignment.TopStart)
                // Fullscreen theme: keep the menu out of the display cutout.
                .systemBarsPadding()
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
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
                )
            }
            Spacer(Modifier.weight(1f))
            Box {
                RoundChip("⋯", size = 48.dp) { menuOpen = true }
                DropdownMenu(
                    expanded = menuOpen,
                    onDismissRequest = { menuOpen = false },
                    modifier = Modifier.background(Color(0xFF141A22)),
                ) {
                    DropdownMenuItem(
                        text = { Text(Copy.stick("drive"), color = Color.White) },
                        onClick = { stick = "drive"; menuOpen = false },
                    )
                    DropdownMenuItem(
                        text = { Text(Copy.stick("head"), color = Color.White) },
                        onClick = { stick = "head"; menuOpen = false },
                    )
                    DropdownMenuItem(
                        text = { Text(Copy.stick("body"), color = Color.White) },
                        onClick = { stick = "body"; menuOpen = false },
                    )
                    HorizontalDivider(color = Color.White.copy(alpha = 0.12f))
                    DropdownMenuItem(
                        text = { Text(Copy.live, color = Color.White) },
                        onClick = { onSurface(Surface.Live); menuOpen = false },
                    )
                    DropdownMenuItem(
                        text = { Text(Copy.director, color = Color.White) },
                        onClick = { onSurface(Surface.Director); menuOpen = false },
                    )
                    HorizontalDivider(color = Color.White.copy(alpha = 0.12f))
                    DropdownMenuItem(
                        text = { Text(Copy.flashlight, color = Color.White) },
                        onClick = { toggleFlashlight(); menuOpen = false },
                    )
                    DropdownMenuItem(
                        text = { Text(Copy.language.label, color = Color.White) },
                        onClick = {
                            Copy.language = if (Copy.language == Copy.Language.English) {
                                Copy.Language.Chinese
                            } else {
                                Copy.Language.English
                            }
                            languageTick += 1
                            menuOpen = false
                        },
                    )
                }
            }
        }

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
            RoundChip(Copy.time, size = 48.dp) { showSheet = true }
            RoundChip(Copy.skills, size = 48.dp) { showRing = true }
            core?.let { duck ->
                WheeeChip(
                    Copy.quack,
                    onTap = { quack?.invoke() },
                    onHold = { onWheee?.invoke(it) },
                )
            }
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

        if (showRing && core != null) {
            SkillRing(
                skills = skills,
                onPick = { name ->
                    when (name) {
                        "sit" -> core.sit()
                        "ground_pick" -> core.groundPick()
                        else -> core.skill(name)
                    }
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

@Composable
private fun SurfaceChip(label: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .padding(horizontal = 2.dp)
            .background(
                if (selected) Color.White.copy(alpha = 0.18f) else Color.Transparent,
                CircleShape,
            )
            .pointerInput(Unit) { detectTapGestures { onClick() } }
            .padding(horizontal = 10.dp, vertical = 6.dp),
    ) {
        Text(label, color = Color.White.copy(alpha = 0.85f), fontSize = 11.sp)
    }
}

@Composable
private fun RoundChip(
    label: String,
    size: Dp = 52.dp,
    selected: Boolean = false,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .size(size)
            .background(
                if (selected) Color.White.copy(alpha = 0.24f) else Color.White.copy(alpha = 0.10f),
                CircleShape,
            )
            .pointerInput(Unit) { detectTapGestures { onClick() } },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White.copy(alpha = 0.9f), fontSize = 11.sp, maxLines = 1, softWrap = false)
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
    onTap: () -> Unit,
    onHold: (Boolean) -> Unit,
) {
    Box(
        Modifier
            .size(size)
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
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White.copy(alpha = 0.9f), fontSize = 11.sp, maxLines = 1, softWrap = false)
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
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .fillMaxHeight(0.86f)
                .verticalScroll(rememberScrollState())
                .padding(vertical = 12.dp),
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
            // `sit` and `ground_pick` are already in the skills list (/skills
            // publishes them now); the ring shows the list, not a second copy.
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
            .background(Color.White.copy(alpha = 0.16f), CircleShape)
            .pointerInput(Unit) { detectTapGestures { onPick(name) } }
            .padding(horizontal = 18.dp, vertical = 10.dp),
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
            Text(Copy.thisDuck, color = Color.White, fontWeight = FontWeight.SemiBold)
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
                    )
                    SurfaceChip(
                        if (speechFeed.state == SpeechState.Listening) "ON" else "OFF",
                        speechFeed.state == SpeechState.Listening,
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
                Text(behaviorSummary, color = Color.White.copy(alpha = 0.6f), fontSize = 11.sp)
            }
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.trickBook, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Text(trickBook, color = Color.White.copy(alpha = 0.6f), fontSize = 11.sp)
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
                ButtonChip(if (paused) Copy.resume else Copy.freeze) {
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
private fun ButtonChip(label: String, onClick: () -> Unit) {
    Box(
        Modifier
            .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(10.dp))
            .pointerInput(Unit) { detectTapGestures { onClick() } }
            .padding(horizontal = 14.dp, vertical = 9.dp),
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
