package ai.duck.microduck

import android.provider.Settings
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween

/**
 * The four motion gears (UI_TOKENS §5, D3) and the haptic map. Every explicit
 * transition picks one of these; a raw duration outside this file is a
 * defect the ratchet counts. Reduce Motion is honoured in one place: state
 * changes still run, the transition just stops animating.
 */
object DuckMotion {
    /** Button feedback, chip states. */
    fun <T> quick(): androidx.compose.animation.core.FiniteAnimationSpec<T> = tween(150)

    /** State switches, capsule fades. */
    fun <T> base(): androidx.compose.animation.core.FiniteAnimationSpec<T> = tween(250)

    /** Sheets, camera switches. */
    fun <T> slow(): androidx.compose.animation.core.FiniteAnimationSpec<T> = tween(400)

    /** Skill ring, the time rail's rise. */
    fun <T> deliberate(): androidx.compose.animation.core.FiniteAnimationSpec<T> =
        spring(dampingRatio = Spring.DampingRatioLowBouncy, stiffness = Spring.StiffnessLow)
}

/** True when the system's animator scale is off — the reduce-motion flag. */
fun readReduceMotion(view: View): Boolean = Settings.Global.getFloat(
    view.context.contentResolver,
    Settings.Global.ANIMATOR_DURATION_SCALE,
    1f,
) == 0f

/** The haptic map (总纲 §3.4): one event, one strength, one entry point.
 * Every entry is a real user-visible moment — nothing fires on polling. */
object DuckHaptics {
    private fun view(view: View, constant: Int) {
        view.performHapticFeedback(constant)
    }

    /** A tap on the duck. */
    fun pet(view: View) = view(view, HapticFeedbackConstants.KEYBOARD_TAP)

    /** A skill leaves the ring, a mode flips. */
    fun selection(view: View) = view(view, HapticFeedbackConstants.CLOCK_TICK)

    /** The duck is fed (a command the child sent). */
    fun medium(view: View) = view(view, HapticFeedbackConstants.VIRTUAL_KEY)

    /** A drawn route finishes on its own / the duck is fed for real. */
    fun success(view: View) = view(view, HapticFeedbackConstants.CONFIRM)

    /** The duck falls. */
    fun error(view: View) = view(view, HapticFeedbackConstants.REJECT)
}
