package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * The rear camera as the backdrop of Director mode.
 *
 * The 3D duck is composited over the live image — the room it stands in is the
 * real one. This is a *backdrop*, not a sense: no frame is delivered to the
 * app, nothing enters the 61-D observation, and the control loop (a 20 ms
 * scheduled thread) sees no extra work.
 *
 * Only metadata (size, frame rate, field of view) is posted to `/sensor`, at
 * 1 Hz. A device without a camera keeps the studio view and logs once.
 */
@Composable
fun CameraBackdrop(
    active: Boolean,
    onAvailability: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    var granted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { result -> granted = result }

    // Asked only when Director is opened, never at launch.
    LaunchedEffect(active) {
        if (active && !granted) {
            launcher.launch(Manifest.permission.CAMERA)
        }
    }

    val previewView = remember {
        PreviewView(context).apply {
            // PERFORMANCE uses a SurfaceView, which the GL surface (drawn with
            // setZOrderMediaOverlay) can sit above and blend over. A
            // TextureView is part of the window and would cover the duck.
            implementationMode = PreviewView.ImplementationMode.PERFORMANCE
            scaleType = PreviewView.ScaleType.FILL_CENTER
        }
    }

    DisposableEffect(active, granted) {
        var provider: ProcessCameraProvider? = null
        var metadata: java.util.concurrent.ScheduledExecutorService? = null
        if (active && granted) {
            val future = ProcessCameraProvider.getInstance(context)
            future.addListener({
                try {
                    val cameraProvider = future.get()
                    provider = cameraProvider
                    val preview = Preview.Builder().build().also {
                        it.setSurfaceProvider(previewView.surfaceProvider)
                    }
                    cameraProvider.unbindAll()
                    cameraProvider.bindToLifecycle(
                        lifecycleOwner,
                        CameraSelector.DEFAULT_BACK_CAMERA,
                        preview,
                    )
                    onAvailability(true)
                    metadata = startMetadata(context)
                } catch (error: Throwable) {
                    // No camera on an emulator, or the provider refused it.
                    android.util.Log.i(
                        "duck-camera",
                        "camera unavailable: ${error.message}",
                    )
                    onAvailability(false)
                }
            }, ContextCompat.getMainExecutor(context))
        } else {
            onAvailability(false)
        }
        onDispose {
            metadata?.shutdownNow()
            provider?.unbindAll()
        }
    }

    if (active && granted) {
        AndroidView(
            factory = { previewView },
            modifier = modifier,
        )
    }
}

/** One metadata post per second, on its own thread: not the control loop. */
private fun startMetadata(context: Context): java.util.concurrent.ScheduledExecutorService {
    val executor = Executors.newSingleThreadScheduledExecutor()
    executor.scheduleAtFixedRate(
        {
            try {
                val body = """{"sensor":"camera","mode":"ar","value":{""" +
                    """"width":1280,"height":720,"fps":30,"position":"back"}}"""
                val connection = URL("http://127.0.0.1:8765/sensor")
                    .openConnection() as HttpURLConnection
                connection.requestMethod = "POST"
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.connectTimeout = 2000
                connection.readTimeout = 2000
                connection.outputStream.use { it.write(body.toByteArray()) }
                connection.responseCode
                connection.disconnect()
            } catch (_: Throwable) {
                // The intent server may not be up yet; next second retries.
            }
        },
        0,
        1,
        TimeUnit.SECONDS,
    )
    return executor
}
