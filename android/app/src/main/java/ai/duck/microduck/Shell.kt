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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
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
    // Stop the feed only when the shell leaves composition (the activity is
    // going away). Toggling the mic off stops it explicitly in onToggleMic; a
    // DisposableEffect keyed on micState would fire its onDispose the instant
    // micState flips to On and kill the feed right after it started.
    DisposableEffect(Unit) {
        onDispose { micFeed?.stop() }
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

        // Top bar: the duck's name and what it is doing, and the surface switch.
        Row(
            Modifier
                .align(Alignment.TopStart)
                .padding(16.dp)
                .background(Color.Black.copy(alpha = 0.35f), RoundedCornerShape(18.dp))
                .padding(12.dp),
            verticalAlignment = Alignment.Top,
        ) {
            Column {
                Text(Copy.name, color = Color.White, fontWeight = FontWeight.SemiBold)
                Text(
                    when {
                        core == null -> Copy.waking
                        carrying -> Copy.held
                        core.paused() -> Copy.freeze
                        else -> Copy.live
                    },
                    color = Color.White.copy(alpha = 0.7f),
                    fontSize = 12.sp,
                )
            }
            Spacer(Modifier.size(14.dp))
            SurfaceChip(Copy.stick("drive"), stick == "drive") { stick = "drive" }
            SurfaceChip(Copy.stick("head"), stick == "head") { stick = "head" }
            SurfaceChip(Copy.stick("body"), stick == "body") { stick = "body" }
            SurfaceChip(Copy.live, surface == Surface.Live) { onSurface(Surface.Live) }
            SurfaceChip(Copy.director, surface == Surface.Director) { onSurface(Surface.Director) }
            SurfaceChip(Copy.language.label, false) {
                Copy.language = if (Copy.language == Copy.Language.English) {
                    Copy.Language.Chinese
                } else {
                    Copy.Language.English
                }
                languageTick += 1
            }
        }

        // Bottom bar: time machine and skills live one gesture away.
        Row(
            Modifier
                .align(Alignment.BottomStart)
                .padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            RoundChip(Copy.time) { showSheet = true }
            RoundChip(Copy.skills) { showRing = true }
            core?.let { duck ->
                RoundChip(Copy.quack) { quack?.invoke() }
                RoundChip(if (carrying) Copy.putDown else Copy.grab) {
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
private fun RoundChip(label: String, onClick: () -> Unit) {
    Box(
        Modifier
            .size(52.dp)
            .background(Color.White.copy(alpha = 0.10f), CircleShape)
            .pointerInput(Unit) { detectTapGestures { onClick() } },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White.copy(alpha = 0.9f), fontSize = 11.sp)
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
    onToggleMic: () -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
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
    LaunchedEffect(core) {
        while (true) {
            micSummary = fetchMicSummary()
            behaviorSummary = fetchBehaviorSummary()
            delay(1000)
        }
    }

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(Copy.thisDuck, color = Color.White, fontWeight = FontWeight.SemiBold)
            if (status.size >= 14) {
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
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(Copy.behavior, color = Color.White.copy(alpha = 0.75f), fontSize = 12.sp)
                Text(behaviorSummary, color = Color.White.copy(alpha = 0.6f), fontSize = 11.sp)
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
