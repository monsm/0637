package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The room's light is a pure function of an hour, so tests pin the hour
 * instead of the wall clock: the design review asked for an injectable time.
 */
class DaylightTest {
    @Test
    fun nightNeverFallsBelowTheReadabilityFloor() {
        for (hour in 0..24) {
            val sample = Daylight.at(hour.toFloat())
            assertTrue(
                "exposure at ${hour}h is ${sample.exposure}, want >= 0.80",
                sample.exposure >= 0.80f,
            )
        }
    }

    @Test
    fun noonIsNeutralAndFullyExposed() {
        val sample = Daylight.at(12f)
        assertEquals(1.0f, sample.tint[0], 1e-4f)
        assertEquals(1.0f, sample.tint[1], 1e-4f)
        assertEquals(1.0f, sample.tint[2], 1e-4f)
        assertEquals(1.0f, sample.exposure, 1e-4f)
    }

    @Test
    fun anchorsAreSmoothAtTheBoundaries() {
        // Straddle the 6.5 anchor: 6.499 is the last sample of the pre-dawn
        // segment, 6.501 the first of the morning one. 5.999/6.001 would
        // both sit inside the same segment and pass even if the curve jumped.
        val before = Daylight.at(6.499f)
        val after = Daylight.at(6.501f)
        assertEquals(before.tint[0], after.tint[0], 0.02f)
        assertEquals(before.exposure, after.exposure, 0.02f)
    }

    @Test
    fun localHourUsesTheInstantItIsGiven() {
        // 2026-11-13 00:00:00 UTC, but Calendar reads the local zone; build
        // the instant from the local fields so the assertion is zone-free.
        val calendar = java.util.Calendar.getInstance().apply {
            set(2026, java.util.Calendar.NOVEMBER, 13, 14, 30, 0)
            set(java.util.Calendar.MILLISECOND, 0)
        }
        assertEquals(14.5f, Daylight.localHour(calendar.timeInMillis), 1e-3f)
    }
}
