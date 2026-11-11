package ai.duck.microduck

import android.graphics.Bitmap
import android.opengl.GLSurfaceView
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.viewinterop.AndroidView
import java.io.File
import kotlin.math.roundToInt
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    private var core: DuckCore? = null
    private var loop: DuckLoop? = null
    private var voice: Voice? = null
    private lateinit var input: DuckInput

    override fun onCreate(savedInstanceState: Bundle?) {
        companionReady = false
        // Edge-to-edge: the GL scene runs under the bars, the chrome insets
        // itself (safeDrawing in portrait, systemBarsPadding in landscape).
        enableEdgeToEdge()
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
                        // The duck's own voice: the core decides when it
                        // quacks, this owns the speaker.
                        voice = Voice(created).also { it.start() }
                    },
                    input = input,
                    onQuack = { voice?.play("chirp") },
                    onWheee = { held -> core?.wheee(held) },
                    onFeed = { postDuckCommand("/feed") },
                )
            }
        }
    }

    companion object {
        /** The shell sets this while its restore has had time to land. */
        @Volatile @JvmStatic var companionReady = false
    }

    override fun onPause() {
        super.onPause()
        // Remember what the duck had done by now; onResume's sheet reads the
        // diff as "while you were away".
        Thread {
            val events = fetchLifeEvents()
            if (events.isNotEmpty()) AwayStore.atBackground = events.toSet()
        }.start()
        // The companion state is the duck's, not the activity's: hunger, mood
        // and the bond must survive the process being killed at any moment.
        // Wait for the write: force-stop can follow this callback immediately,
        // and a fire-and-forget thread would die with the process.
        if (core != null && MainActivity.companionReady) {
            val save = Thread { saveCompanionNow(this) }
            save.start()
            try {
                save.join(1500)
            } catch (_: InterruptedException) {
            }
        }
    }

    override fun onDestroy() {
        companionReady = false
        voice?.stop()
        voice = null
        loop?.stop()
        loop = null
        // A held ride must not stick past the shell: let go before the core
        // frees, so the controller never keeps the hold it was left in.
        core?.wheee(false)
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
    onQuack: () -> Unit,
    onWheee: (Boolean) -> Unit,
    onFeed: () -> Unit,
) {
    val context = LocalContext.current
    var core by remember { mutableStateOf(coreProvider()) }
    var error by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf(DoubleArray(0)) }
    var skills by remember { mutableStateOf(emptyList<String>()) }
    var surface by remember { mutableStateOf(Surface.Live) }
    // Director's photo path: the shutter reads the GL frame, CameraX supplies
    // the room, and the preview sheet saves or shares the composite.
    var renderer by remember { mutableStateOf<DuckRenderer?>(null) }
    val cameraCapture = remember { CameraCapture() }
    var photo by remember { mutableStateOf<Bitmap?>(null) }
    var photoInFlight by remember { mutableStateOf(false) }
    var cameraAvailable by remember { mutableStateOf(false) }
    val cameraActive = surface == Surface.Director && cameraAvailable
    LaunchedEffect(cameraActive) { input.cameraActive = cameraActive }

    // MuJoCo scene + ONNX policy loading takes seconds; doing it on the main
    // thread froze the UI (Choreographer: Skipped 40 frames) and stalled the
    // loader. Create the core on a worker thread.
    LaunchedEffect(Unit) {
        if (core != null) return@LaunchedEffect
        val created = withContext(Dispatchers.IO) {
            runCatching {
                val root = AssetStager.stage(context)
                val core = DuckCore(
                    File(root, "robot/microduck/scene_backlash.xml").path,
                    File(root, "policies").path,
                )
                // The pickup classifier: the duck pauses its policy in a hand
                // for the same reason the physical one does.
                val pickup = File(root, "models/pickup_detector.onnx")
                if (pickup.exists()) {
                    core.loadPickup(pickup.path)
                }
                // `robot.do curious` and the other twelve: the official
                // keyframe timelines and their 48 kHz voices, staged as
                // assets/emotions next to the policies.
                val emotions = File(root, "emotions")
                if (emotions.isDirectory) {
                    android.util.Log.i("duck", "emotions loaded: ${core.loadEmotions(emotions.path)}")
                }
                core
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
            // Director: the room, then the duck over it. CameraX renders into
            // a TextureView behind the GL surface; the camera never feeds the
            // control loop.
            if (surface == Surface.Director) {
                CameraBackdrop(
                    active = true,
                    onAvailability = { cameraAvailable = it },
                    capture = cameraCapture,
                    modifier = Modifier.fillMaxSize(),
                )
            }
            AndroidView(
                factory = { ctx ->
                    GLSurfaceView(ctx).apply {
                        setEGLContextClientVersion(3)
                        // Alpha in the surface: Director clears to alpha 0 so
                        // the camera behind shows through.
                        setEGLConfigChooser(8, 8, 8, 8, 16, 0)
                        holder.setFormat(android.graphics.PixelFormat.TRANSLUCENT)
                        setZOrderMediaOverlay(true)
                        val duckRenderer = DuckRenderer(ctx, active, input)
                        renderer = duckRenderer
                        setRenderer(duckRenderer)
                        renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
            // The fused shell owns the chrome: live duck, instrument sheet,
            // viewfinder.
            Shell(
                core = active,
                skills = skills,
                input = input,
                surface = surface,
                onSurface = { surface = it },
                quack = onQuack,
                onWheee = onWheee,
                onFeed = onFeed,
                onPhoto = {
                    val duckRenderer = renderer
                    if (duckRenderer != null && !photoInFlight) {
                        photoInFlight = true
                        val main = Handler(Looper.getMainLooper())
                        duckRenderer.captureFrame { duck ->
                            main.post {
                                if (duck == null) {
                                    photoInFlight = false
                                    return@post
                                }
                                if (cameraActive) {
                                    cameraCapture.take(context) { room ->
                                        photo = compositePhoto(room, duck)
                                        photoInFlight = false
                                    }
                                } else {
                                    photo = duck
                                    photoInFlight = false
                                }
                            }
                        }
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
            photo?.let { shot ->
                PhotoPreviewDialog(shot) { photo = null }
            }
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
private fun PhotoPreviewDialog(photo: Bitmap, onClose: () -> Unit) {
    val context = LocalContext.current
    var saved by remember { mutableStateOf(false) }
    var saveFailed by remember { mutableStateOf(false) }
    Dialog(onDismissRequest = onClose) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(Color(0xFF0E131A), RoundedCornerShape(20.dp))
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                Copy.photoTitle,
                color = Color.White,
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold,
            )
            Image(
                bitmap = photo.asImageBitmap(),
                contentDescription = Copy.photoTitle,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(360.dp)
                    .testTag("photo-preview"),
                contentScale = ContentScale.Fit,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Button(
                    onClick = {
                        saved = savePhoto(context, photo)
                        saveFailed = !saved
                    },
                    enabled = !saved,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("photo-save"),
                ) {
                    Text(
                        if (saved) Copy.saved else if (saveFailed) Copy.saveFailed else Copy.savePhoto,
                        fontSize = 13.sp,
                        maxLines = 1,
                    )
                }
                Button(
                    onClick = { sharePhoto(context, photo) },
                    modifier = Modifier
                        .weight(1f)
                        .testTag("photo-share"),
                ) {
                    Text(Copy.share, fontSize = 13.sp)
                }
            }
            Button(
                onClick = onClose,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("photo-close"),
            ) {
                Text(Copy.close, fontSize = 13.sp)
            }
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
