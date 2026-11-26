package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The one-duck show: a long idle earns one three-beat routine, and every
 * beat is a verb the duck already has.
 */
class ShowCuesTest {
    @Test
    fun theShowHoldsUntilTheLongIdle() {
        assertFalse(showDue(239.9, enabled = true, done = false))
    }

    @Test
    fun aLongIdleEarnsOneShow() {
        assertTrue(showDue(240.0, enabled = true, done = false))
        assertFalse("a second show needs a new long idle", showDue(300.0, enabled = true, done = true))
    }

    @Test
    fun theSwitchMustBeOn() {
        assertFalse(showDue(300.0, enabled = false, done = false))
    }

    @Test
    fun theBeatsAreTheDucksOwnStandingVerbs() {
        assertEquals(listOf("roulade", "curious", "kick_left"), SHOW_BEATS)
    }
}
