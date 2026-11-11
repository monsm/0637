package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCaptureException
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
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LocalLifecycleOwner
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
    capture: CameraCapture? = null,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    // The activity handles configChanges, so turning the phone does not
    // recreate the window. CameraX still has to be told which way up the
    // display is: reading LocalConfiguration recomposes this branch on a
    // turn, and the preview below is rebound with the new target rotation so
    // the image (and the duck composited over it) stays upright.
    val configuration = LocalConfiguration.current
    val rotation = displayRotation(context)
    var previewUseCase by remember { mutableStateOf<Preview?>(null) }
    LaunchedEffect(configuration, rotation) {
        previewUseCase?.targetRotation = rotation
    }
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

    DisposableEffect(active, granted, rotation) {
        var provider: ProcessCameraProvider? = null
        var metadata: java.util.concurrent.ScheduledExecutorService? = null
        var disposed = false
        // The renderer must not go transparent before there is a camera image;
        // a stale true from a previous visit would clear to black.
        onAvailability(false)
        if (active && granted) {
            val future = ProcessCameraProvider.getInstance(context)
            future.addListener({
                // The provider future can resolve after Director is closed.
                // Binding here then would leak a camera session nothing owns.
                if (disposed) return@addListener
                try {
                    val cameraProvider = future.get()
                    provider = cameraProvider
                    val preview = Preview.Builder()
                        .setTargetRotation(rotation)
                        .build()
                        .also {
                            it.setSurfaceProvider(previewView.surfaceProvider)
                            previewUseCase = it
                        }
                    cameraProvider.unbindAll()
                    // The still capture shares the bound session with the
                    // preview; a second bind would tear the preview down.
                    val still = ImageCapture.Builder()
                        .setTargetRotation(rotation)
                        .build()
                    cameraProvider.bindToLifecycle(
                        lifecycleOwner,
                        CameraSelector.DEFAULT_BACK_CAMERA,
                        preview,
                        still,
                    )
                    capture?.imageCapture = still
                    val resolution = preview.resolutionInfo?.resolution
                    metadata = startMetadata(context, resolution)
                    onAvailability(true)
                } catch (error: Throwable) {
                    // No camera on an emulator, or the provider refused it.
                    android.util.Log.i(
                        "duck-camera",
                        "camera unavailable: ${error.message}",
                    )
                    onAvailability(false)
                }
            }, ContextCompat.getMainExecutor(context))
        }
        onDispose {
            disposed = true
            previewUseCase = null
            capture?.imageCapture = null
            metadata?.shutdownNow()
            provider?.unbindAll()
            onAvailability(false)
        }
    }

    if (active && granted) {
        AndroidView(
            factory = { previewView },
            modifier = modifier,
        )
    }
}

/**
 * The display's rotation, the value CameraX's targetRotation expects. The
 * Activity owns an unspecified-orientation window that handles its own
 * configChanges, so the display is the truth, not the activity's requested
 * orientation.
 */
@Suppress("DEPRECATION")
private fun displayRotation(context: Context): Int {
    val activity = context as? android.app.Activity ?: return android.view.Surface.ROTATION_0
    return activity.windowManager.defaultDisplay.rotation
}

/**
 * The still-capture half of Director. The preview's session owns the camera;
 * this borrows the bound ImageCapture for one photo. No capture has to be
 * active for the duck alone: with no camera the renderer's studio frame is
 * the picture.
 */
class CameraCapture {
    @Volatile var imageCapture: ImageCapture? = null

    fun take(context: Context, onImage: (android.graphics.Bitmap?) -> Unit) {
        val capture = imageCapture
        if (capture == null) {
            onImage(null)
            return
        }
        val directory = java.io.File(context.cacheDir, "captures").apply { mkdirs() }
        val file = java.io.File(directory, "room-${System.currentTimeMillis()}.jpg")
        val options = ImageCapture.OutputFileOptions.Builder(file).build()
        capture.takePicture(
            options,
            ContextCompat.getMainExecutor(context),
            object : ImageCapture.OnImageSavedCallback {
                override fun onImageSaved(output: ImageCapture.OutputFileResults) {
                    val bitmap = android.graphics.BitmapFactory.decodeFile(file.absolutePath)
                    file.delete()
                    onImage(bitmap)
                }

                override fun onError(exception: ImageCaptureException) {
                    file.delete()
                    onImage(null)
                }
            },
        )
    }
}

/** One metadata post per second, on its own thread: not the control loop. */
private fun startMetadata(
    context: Context,
    resolution: android.util.Size?,
): java.util.concurrent.ScheduledExecutorService {
    val executor = Executors.newSingleThreadScheduledExecutor()
    val (width, height) = resolution?.let { it.width to it.height } ?: (0 to 0)
    executor.scheduleAtFixedRate(
        {
            try {
                // Only what the platform actually reports: a fabricated frame
                // rate or exposure would be a number nobody measured.
                val body = if (width > 0 && height > 0) {
                    """{"sensor":"camera","mode":"ar","value":{""" +
                        """"width":$width,"height":$height,"position":"back"}}"""
                } else {
                    """{"sensor":"camera","mode":"ar","value":{"position":"back"}}"""
                }
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
