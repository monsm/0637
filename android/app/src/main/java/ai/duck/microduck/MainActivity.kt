package ai.duck.microduck

import android.opengl.GLSurfaceView
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
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
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import java.io.File
import kotlin.math.roundToInt
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    private lateinit var core: DuckCore
    private lateinit var input: DuckInput

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = AssetStager.stage(this)
        core = DuckCore(
            File(root, "robot/microduck/scene_backlash.xml").path,
            File(root, "policies").path,
        )
        core.startIntentServer(8765)
        input = DuckInput()
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                DuckScreen(core, input)
            }
        }
    }

    override fun onDestroy() {
        core.close()
        super.onDestroy()
    }
}

@Composable
private fun DuckScreen(core: DuckCore, input: DuckInput) {
    var status by remember { mutableStateOf(DoubleArray(0)) }
    var skills by remember { mutableStateOf(emptyList<String>()) }

    LaunchedEffect(core) {
        skills = (0 until core.skillCount()).mapNotNull { core.skillName(it) }
        while (true) {
            status = core.status()
            delay(100)
        }
    }

    Box(Modifier.fillMaxSize().background(Color(0xFF0E131A))) {
        AndroidView(
            factory = { context ->
                GLSurfaceView(context).apply {
                    setEGLContextClientVersion(3)
                    setRenderer(DuckRenderer(context, core, input))
                    renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
                }
            },
            modifier = Modifier.fillMaxSize(),
        )

        StatusPanel(status, Modifier.align(Alignment.TopStart).padding(16.dp))

        Joystick(Modifier.align(Alignment.BottomStart).padding(24.dp).size(150.dp)) { x, y ->
            input.vx = -y * 0.4
            input.wz = x * 1.0
        }

        Controls(core, skills, Modifier.align(Alignment.BottomEnd).padding(24.dp))
    }
}

@Composable
private fun StatusPanel(status: DoubleArray, modifier: Modifier = Modifier) {
    val bringup = status.getOrNull(0)?.toInt() ?: 0
    val fallen = (status.getOrNull(1) ?: 0.0) != 0.0
    val gain = status.getOrNull(2)?.toInt() ?: 200
    val bringupText = when (bringup) {
        1 -> "Homing"
        2 -> "Ready"
        else -> "Limp"
    }
    Column(
        modifier
            .background(Color.Black.copy(alpha = 0.35f), RoundedCornerShape(12.dp))
            .padding(12.dp)
    ) {
        Text("MicroDuck", color = Color.White, fontWeight = FontWeight.Bold)
        Text("$bringupText · gain $gain", color = Color.White.copy(alpha = 0.7f), fontSize = 12.sp)
        Text("AI API :8765 · shake to push", color = Color.White.copy(alpha = 0.55f), fontSize = 11.sp)
        if (fallen) {
            Text("FALLEN", color = Color(0xFFFF6B6B), fontWeight = FontWeight.Bold, fontSize = 12.sp)
        }
    }
}

@Composable
private fun Controls(core: DuckCore, skills: List<String>, modifier: Modifier = Modifier) {
    Column(
        modifier,
        horizontalAlignment = Alignment.End,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        skills.forEach { name ->
            Button(onClick = { core.skill(name) }) {
                Text(name, fontSize = 12.sp)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { core.sit() }) {
                Text("Sit / Stand", fontSize = 12.sp)
            }
            Button(onClick = { core.groundPick() }) {
                Text("Ground pick", fontSize = 12.sp)
            }
        }
        Button(onClick = { core.reload() }) {
            Text("Reload AI", fontSize = 12.sp)
        }
    }
}

@Composable
private fun Joystick(modifier: Modifier = Modifier, onChange: (Float, Float) -> Unit) {
    var offset by remember { mutableStateOf(Offset.Zero) }
    Box(
        modifier = modifier
            .background(Color.White.copy(alpha = 0.06f), CircleShape)
            .border(1.dp, Color.White.copy(alpha = 0.15f), CircleShape)
            .pointerInput(Unit) {
                detectDragGestures(
                    onDrag = { change, drag ->
                        change.consume()
                        val max = size.width / 2f
                        offset = Offset(
                            (offset.x + drag.x).coerceIn(-max, max),
                            (offset.y + drag.y).coerceIn(-max, max),
                        )
                        onChange(offset.x / max, offset.y / max)
                    },
                    onDragEnd = {
                        offset = Offset.Zero
                        onChange(0f, 0f)
                    },
                    onDragCancel = {
                        offset = Offset.Zero
                        onChange(0f, 0f)
                    },
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .offset { IntOffset(offset.x.roundToInt(), offset.y.roundToInt()) }
                .size(54.dp)
                .background(Color.White.copy(alpha = 0.22f), CircleShape)
        )
    }
}
