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
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.viewinterop.AndroidView
import java.io.File
import kotlin.math.roundToInt
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    private var core: DuckCore? = null
    private var loop: DuckLoop? = null
    private lateinit var input: DuckInput

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Copy.load(this)
        input = DuckInput()
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                DuckScreen(
                    coreProvider = { core },
                    onCoreCreated = { created ->
                        core = created
                        created.startIntentServer(8765)
                        loop = DuckLoop(created, input).also { it.start() }
                    },
                    input = input,
                )
            }
        }
    }

    override fun onDestroy() {
        loop?.stop()
        loop = null
        core?.close()
        core = null
        super.onDestroy()
    }
}

@Composable
private fun DuckScreen(
    coreProvider: () -> DuckCore?,
    onCoreCreated: (DuckCore) -> Unit,
    input: DuckInput,
) {
    val context = LocalContext.current
    var core by remember { mutableStateOf(coreProvider()) }
    var error by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf(DoubleArray(0)) }
    var skills by remember { mutableStateOf(emptyList<String>()) }

    // MuJoCo scene + ONNX policy loading takes seconds; doing it on the main
    // thread froze the UI (Choreographer: Skipped 40 frames) and stalled the
    // loader. Create the core on a worker thread.
    LaunchedEffect(Unit) {
        if (core != null) return@LaunchedEffect
        val created = withContext(Dispatchers.IO) {
            runCatching {
                val root = AssetStager.stage(context)
                DuckCore(
                    File(root, "robot/microduck/scene_backlash.xml").path,
                    File(root, "policies").path,
                )
            }
        }
        created.onSuccess { if (it.ready) { onCoreCreated(it); core = it } else { error = "core failed" } }
            .onFailure { error = it.message }
    }

    LaunchedEffect(core) {
        val active = core ?: return@LaunchedEffect
        skills = (0 until active.skillCount()).mapNotNull { active.skillName(it) }
        while (true) {
            status = active.status()
            delay(100)
        }
    }

    Box(Modifier.fillMaxSize().background(Color(0xFF0E131A))) {
        val active = core
        if (active != null) {
            AndroidView(
                factory = { ctx ->
                    GLSurfaceView(ctx).apply {
                        setEGLContextClientVersion(3)
                        setRenderer(DuckRenderer(ctx, active, input))
                        renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
            // The fused shell owns the chrome: live duck, instrument sheet,
            // viewfinder.
            Shell(active, skills, input, Modifier.fillMaxSize())
        } else {
            Text(
                error ?: "Loading MuJoCo + policies…",
                color = Color.White,
                modifier = Modifier.align(Alignment.Center).padding(24.dp),
            )
            StatusPanel(status, Modifier.align(Alignment.TopStart).padding(16.dp))
        }
    }
}

@Composable
private fun StatusPanel(status: DoubleArray, modifier: Modifier = Modifier) {
    val bringup = status.getOrNull(0)?.toInt() ?: 0
    val fallen = (status.getOrNull(1) ?: 0.0) != 0.0
    val gain = status.getOrNull(2)?.toInt() ?: 200
    val bringupText = Copy.bringup(bringup)
    Column(
        modifier
            .background(Color.Black.copy(alpha = 0.35f), RoundedCornerShape(12.dp))
            .padding(12.dp)
    ) {
        Text(Copy.name, color = Color.White, fontWeight = FontWeight.Bold)
        Text("$bringupText · gain $gain", color = Color.White.copy(alpha = 0.7f), fontSize = 12.sp)
        Text("AI API :8765 · shake to push", color = Color.White.copy(alpha = 0.55f), fontSize = 11.sp)
        if (fallen) {
            Text(Copy.fell, color = Color(0xFFFF6B6B), fontWeight = FontWeight.Bold, fontSize = 12.sp)
        }
    }
}

/** robotd's AxisBinding::evaluate: inside 0.1 the axis reads zero, otherwise as-is. */
internal fun deadzone(value: Float): Float = if (kotlin.math.abs(value) < 0.1f) 0f else value
