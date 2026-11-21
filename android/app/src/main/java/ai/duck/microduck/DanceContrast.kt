package ai.duck.microduck

import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/**
 * The choreography tile's own colours, and the WCAG maths that keeps them
 * honest.
 *
 * The playing tile used to be white text on #FFD54F: 1.36:1, far under the
 * 4.5:1 body-text floor. iOS already had it right (black on yellow). These
 * constants are pure (no Compose/Android imports) so a JVM unit test can
 * assert the ratio without an emulator.
 */
internal object DanceContrast {
    /** The playing step's background, #FFD54F. */
    const val ACTIVE_BACKGROUND = 0xFFFFD54FL
    /** Black text on that yellow: 10.98:1. */
    const val ACTIVE_TEXT = 0xFF000000L

    private fun channel(value: Int): Double {
        val s = value / 255.0
        return if (s <= 0.03928) s / 12.92 else ((s + 0.055) / 1.055).pow(2.4)
    }

    private fun luminance(argb: Long): Double {
        val r = ((argb shr 16) and 0xFF).toInt()
        val g = ((argb shr 8) and 0xFF).toInt()
        val b = (argb and 0xFF).toInt()
        return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    /** WCAG 2.x contrast ratio, 1.0 … 21.0. */
    fun contrastRatio(foreground: Long, background: Long): Double {
        val a = luminance(foreground)
        val b = luminance(background)
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }
}
