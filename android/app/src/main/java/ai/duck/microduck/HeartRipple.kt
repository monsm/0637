package ai.duck.microduck

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.platform.LocalView
import kotlin.math.exp
import kotlin.math.floor

/**
 * The duck's real pulse, made visible.
 *
 * The Android twin of the iOS `HeartRipple`: `care.heart_bpm` is the core's
 * own reading of the same mood/hunger/bond/asleep numbers the capsules show,
 * and this draws one soft amber ring around the duck that thumps at that real
 * rate — faster when it is excited, slower when it is dozing. A bonus sense
 * like the haptic heart: a zero reading shows nothing.
 *
 * The thump is computed from the wall clock, not a stored timer: the phase is
 * `time × bpm / 60`, so a bpm change lands on the next frame. Frames are
 * driven at ~30 Hz only while the bpm is live.
 */
@Composable
fun HeartRipple(
    bpm: Double,
    modifier: Modifier = Modifier,
) {
    val reduce = readReduceMotion(LocalView.current)
    var nowNanos by remember { mutableLongStateOf(0L) }
    // ~30 Hz while the heart is live; a static ring under Reduce Motion.
    LaunchedEffect(bpm > 0.0, reduce) {
        if (bpm <= 0.0) return@LaunchedEffect
        if (reduce) return@LaunchedEffect
        while (true) {
            nowNanos = System.nanoTime()
            kotlinx.coroutines.delay(33)
        }
    }
    if (bpm <= 0.0) return
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val seconds = nowNanos / 1_000_000_000.0
            val beats = seconds * bpm / 60.0
            val phase = beats - floor(beats)
            // A soft lub-dub: the flash decays from the beat and settles
            // before the next one.
            val pulse = if (reduce) 0.0 else exp(-phase * 5)
            drawHeartRing(pulse)
        }
    }
}

private fun DrawScope.drawHeartRing(pulse: Double) {
    val radius = minOf(size.width, size.height) * (0.30 + 0.03 * pulse)
    val shade = DuckTheme.AccentAction.copy(alpha = (0.08 + 0.42 * pulse).toFloat())
    drawCircle(
        brush = Brush.radialGradient(
            colors = listOf(shade, shade.copy(alpha = 0f)),
            center = Offset(size.width / 2f, size.height / 2f),
            radius = radius.toFloat(),
        ),
        center = Offset(size.width / 2f, size.height / 2f),
        radius = radius.toFloat(),
        style = androidx.compose.ui.graphics.drawscope.Stroke(width = (1.5 + 2.5 * pulse).toFloat()),
    )
}