package ai.duck.microduck

import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.platform.LocalView

/**
 * Direction-A play: a ripple over the stage whenever the duck is sounding.
 *
 * The Android twin of the iOS `VoiceRipple`. A quack, an emotion's voice, a
 * greet — the duck is speaking, and the room should see it as much as hear it.
 * One expanding, fading amber ring. It is a picture over the scene, never a
 * hit target, never a control. The motion goes through the DuckMotion slow
 * gear, and Reduce Motion collapses the animation so the ring settles instantly.
 */
@Composable
fun VoiceRipple(
    isSpeaking: Boolean,
    modifier: Modifier = Modifier,
) {
    // 0 = quiet (no ring), 1 = sounding (ring out). Animated with the slow
    // gear so the wave follows the mouth timeline, not a guessed timer.
    val progress by animateFloatAsState(
        targetValue = if (isSpeaking) 1f else 0f,
        animationSpec = rippleSpec(),
        label = "voice-ripple",
    )
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            drawVoiceRipple(progress)
        }
    }
}

/** The slow gear for the wave; reduce-motion collapses the animation. */
@Composable
private fun rippleSpec(): FiniteAnimationSpec<Float> =
    if (readReduceMotion(LocalView.current)) tween(durationMillis = 0) else DuckMotion.slow()

/** The ring in the stage's own space: a soft amber radial from the centre. */
private fun DrawScope.drawVoiceRipple(progress: Float) {
    if (progress <= 0f) return
    val max = minOf(size.width, size.height) / 2f
    val radius = 0.55f * max + 0.45f * max * progress
    val shade = DuckTheme.AccentAction.copy(alpha = 0.5f * progress)
    val brush = Brush.radialGradient(
        colors = listOf(shade, shade.copy(alpha = 0f)),
        center = Offset(size.width / 2f, size.height / 2f),
        radius = radius,
    )
    drawCircle(
        brush = brush,
        center = Offset(size.width / 2f, size.height / 2f),
        radius = radius,
    )
}
