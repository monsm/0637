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
import kotlin.math.abs

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
    modifier: Modifier = Modifier,
) {
    var surface by remember { mutableStateOf(Surface.Live) }
    var showRing by remember { mutableStateOf(false) }
    var showSheet by remember { mutableStateOf(false) }

    Box(modifier.fillMaxSize()) {
        // Driving: a drag on the scene becomes the twist the real pad sends.
        Box(
            Modifier
                .fillMaxSize()
                .pointerInput(core) {
                    if (core == null) return@pointerInput
                    var centre = Offset.Zero
                    detectDragGestures(
                        onDragStart = { centre = it },
                        onDrag = { change, _ ->
                            change.consume()
                            val width = size.width.toFloat().coerceAtLeast(1f)
                            val height = size.height.toFloat().coerceAtLeast(1f)
                            val dx = (change.position.x - width * 0.5f) / (width * 0.5f)
                            val dy = (change.position.y - height * 0.52f) / (height * 0.5f)
                            val forward = maxOf(0f, -dy)
                            input.vx = deadzone(forward) * 0.4
                            input.wz = -deadzone(dx) * 1.0
                        },
                        onDragEnd = { input.vx = 0.0; input.wz = 0.0 },
                        onDragCancel = { input.vx = 0.0; input.wz = 0.0 },
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
                Text("MicroDuck", color = Color.White, fontWeight = FontWeight.SemiBold)
                Text(
                    core?.let { if (it.paused()) "paused" else "live" } ?: "waking up…",
                    color = Color.White.copy(alpha = 0.7f),
                    fontSize = 12.sp,
                )
            }
            Spacer(Modifier.size(14.dp))
            SurfaceChip("Live", surface == Surface.Live) { surface = Surface.Live }
            SurfaceChip("Director", surface == Surface.Director) { surface = Surface.Director }
        }

        // Bottom bar: time machine and skills live one gesture away.
        Row(
            Modifier
                .align(Alignment.BottomStart)
                .padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            RoundChip("Time") { showSheet = true }
            RoundChip("Skills") { showRing = true }
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

private enum class Surface { Live, Director }

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
                    Text(name, color = Color.White, fontSize = 13.sp)
                }
            }
            Box(
                Modifier
                    .background(Color.White.copy(alpha = 0.16f), CircleShape)
                    .pointerInput(Unit) { detectTapGestures { onPick("sit") } }
                    .padding(horizontal = 18.dp, vertical = 10.dp),
            ) { Text("sit", color = Color.White, fontSize = 13.sp) }
            Box(
                Modifier
                    .background(Color.White.copy(alpha = 0.16f), CircleShape)
                    .pointerInput(Unit) { detectTapGestures { onPick("ground_pick") } }
                    .padding(horizontal = 18.dp, vertical = 10.dp),
            ) { Text("pick", color = Color.White, fontSize = 13.sp) }
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
            "VIEWFINDER",
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
    var tick by remember { mutableStateOf(0L) }
    var branches by remember { mutableStateOf(0) }

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("This duck", color = Color.White, fontWeight = FontWeight.SemiBold)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ButtonChip(if (paused) "Resume" else "Freeze") {
                    paused = !paused
                    core.setPaused(paused)
                }
                ButtonChip("Step") { core.step() }
                ButtonChip("Rewind") { core.seek(200) }
                ButtonChip("Fork") { branches += 1 }
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
