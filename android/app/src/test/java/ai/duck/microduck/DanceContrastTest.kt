package ai.duck.microduck

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The choreography's playing tile must be readable. The old white-on-yellow
 * was 1.36:1; body text needs 4.5:1. This runs on the JVM, so it guards the
 * palette without an emulator.
 */
class DanceContrastTest {
    @Test
    fun playingStepTextMeetsWcagBodyText() {
        val ratio = DanceContrast.contrastRatio(
            DanceContrast.ACTIVE_TEXT,
            DanceContrast.ACTIVE_BACKGROUND,
        )
        assertTrue(
            "playing-step contrast is $ratio:1, want >= 4.5:1",
            ratio >= 4.5,
        )
    }

    @Test
    fun playingStepIsBlackOnYellow() {
        assertTrue(DanceContrast.ACTIVE_TEXT == 0xFF000000L)
        assertTrue(DanceContrast.ACTIVE_BACKGROUND == 0xFFFFD54FL)
    }
}
