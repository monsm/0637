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

/** robotd's AxisBinding::evaluate: inside 0.1 the axis reads zero, otherwise as-is. */
private fun deadzone(value: Float): Float = if (kotlin.math.abs(value) < 0.1f) 0f else value
