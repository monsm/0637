package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The two settling sounds before the doze: a coo at 600 derived idle seconds,
 * a chirp at 690, each once per spell, and both before care.rs' 720 s doze.
 */
class SleepCuesTest {
    @Test
    fun silenceBeforeTheFirstMark() {
        assertNull(sleepCue(599.9, cooed = false, chirped = false))
    }

    @Test
    fun aSettlingCooAtSixHundred() {
        assertEquals("coo", sleepCue(600.0, cooed = false, chirped = false))
    }

    @Test
    fun theCooIsOncePerSpell() {
        assertNull(sleepCue(650.0, cooed = true, chirped = false))
    }

    @Test
    fun aQuieterChirpAtSixNinety() {
        assertEquals("chirp", sleepCue(690.0, cooed = true, chirped = false))
    }

    @Test
    fun theChirpIsOncePerSpell() {
        assertNull(sleepCue(700.0, cooed = true, chirped = true))
    }

    @Test
    fun aJumpPastBothKeepsTheLaterSound() {
        // The app was away while the clock ran on: the coo's moment has
        // passed, and claiming it late would be a lie about the present.
        assertEquals("chirp", sleepCue(700.0, cooed = false, chirped = false))
    }

    @Test
    fun thePublishedClockIsScaledLikeTheCoreDoze() {
        // /state.care.idle_s is published unscaled; care.rs multiplies it by
        // care_time_scale for the doze. The shell must do the same or a 6x
        // clock dozes before its settling sounds can reach their marks.
        assertEquals("coo", sleepCue(100.0 * 6, cooed = false, chirped = false))
    }

    @Test
    fun bothMarksArriveBeforeTheDoze() {
        assertTrue(SLEEP_CUE_COO_S < SLEEP_CUE_CHIRP_S)
        assertTrue(SLEEP_CUE_CHIRP_S < 720.0)
    }
}
