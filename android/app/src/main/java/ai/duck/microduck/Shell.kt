package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
    var showRing by remember { mutableStateOf(false) }
    var showSheet by remember { mutableStateOf(false) }
    var carrying by remember { mutableStateOf(false) }
    // The pad's D-pad modes, reduced to what a phone can do: drive, look,
    // and crouch/lean the standing body.
    var stick by remember { mutableStateOf("drive") }
    // Flipping this re-renders every Copy string in the shell.
    var languageTick by remember { mutableStateOf(0) }

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
        InstrumentSheet(core) { showSheet = false }
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

@Composable
private fun SkillRing(
    skills: List<String>,
    onPick: (String) -> Unit,
    onClose: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.55f))
            .pointerInput(Unit) { detectTapGestures { onClose() } },
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            skills.forEach { name ->
                Box(
                    Modifier
                        .background(Color.White.copy(alpha = 0.16f), CircleShape)
                        .pointerInput(Unit) { detectTapGestures { onPick(name) } }
                        .padding(horizontal = 18.dp, vertical = 10.dp),
                ) {
                    Text(skillLabel(name), color = Color.White, fontSize = 13.sp)
                }
            }
            // `sit` and `ground_pick` are already in the skills list (/skills
            // publishes them now); the ring shows the list, not a second copy.
        }
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
private fun InstrumentSheet(core: DuckCore, onClose: () -> Unit) {
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
                        //  14 fps, 15 ticks/s]
                        append(" · beak ${"%.2f".format(status[12])}")
                        if (status[10] > 0.5) append(" · in a hand")
                        if (status[13] > 0.0) append(" · pack ${"%.2f".format(status[13])} V")
                        if ((status.getOrNull(14) ?: 0.0) > 0.0) append(
                            " · ${"%.0f".format(status[14])} fps · ${"%.1f".format(status[15])} Hz"
                        )
                    },
                    color = Color.White.copy(alpha = 0.65f),
                    fontSize = 11.sp,
                )
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
