package ai.duck.microduck

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The shake rule is the iOS one: |a| > 1.2 g, at most one push per 0.4 s,
 * axes scaled by 0.8 (DuckController.swift:873-887). Android samples arrive
 * in m/s², so the conversion is pinned here too.
 */
class ShakeDetectorTest {

    private val g = STANDARD_GRAVITY

    @Test
    fun `a resting phone pushes nothing`() {
        val detector = ShakeDetector()
        assertNull(detector.onSample(0.3, 0.2, 0.1, 1.0))
    }

    @Test
    fun `a shake becomes the iOS impulse in g`() {
        val detector = ShakeDetector()
        // 2 g along x, nothing else.
        val push = detector.onSample(2 * g, 0.0, 0.0, 1.0)
        assertNotNull(push)
        // 2 g * 0.8 = 1.6, exactly what CoreMotion's userAcceleration path
        // sends from an identical shake.
        assertArrayEquals(doubleArrayOf(1.6, 0.0, 0.0), push!!, 1e-9)
    }

    @Test
    fun `the threshold is strict, exactly 1_2 g does not fire`() {
        val detector = ShakeDetector()
        assertNull(detector.onSample(1.2 * g, 0.0, 0.0, 1.0))
        assertNotNull(detector.onSample(1.2001 * g, 0.0, 0.0, 1.0))
    }

    @Test
    fun `the magnitude is over all three axes`() {
        val detector = ShakeDetector()
        // 0.8 g per axis is 1.385 g overall: above the threshold even though
        // no single axis is.
        val push = detector.onSample(0.8 * g, 0.8 * g, 0.8 * g, 1.0)
        assertNotNull(push)
        assertEquals(3, push!!.size)
    }

    @Test
    fun `pushes are rate limited to one per 0_4 s`() {
        val detector = ShakeDetector()
        assertNotNull(detector.onSample(2 * g, 0.0, 0.0, 1.0))
        assertNull("inside the hold-off", detector.onSample(2 * g, 0.0, 0.0, 1.2))
        assertNull("exactly at the boundary", detector.onSample(2 * g, 0.0, 0.0, 1.4))
        assertNotNull("after the hold-off", detector.onSample(2 * g, 0.0, 0.0, 1.4001))
    }

    @Test
    fun `a quiet interval does not consume the hold-off`() {
        val detector = ShakeDetector()
        assertNotNull(detector.onSample(2 * g, 0.0, 0.0, 1.0))
        assertNull(detector.onSample(0.0, 0.0, 0.0, 1.1))
        assertNull(detector.onSample(2 * g, 0.0, 0.0, 1.3))
        assertNotNull(detector.onSample(2 * g, 0.0, 0.0, 1.5))
    }

    @Test
    fun `the gravity fallback settles and then reports the shove`() {
        val separator = GravitySeparator()
        // A phone at rest: the filter converges on the measured gravity.
        repeat(100) { separator.linear(floatArrayOf(0f, g.toFloat(), 0f)) }
        val rest = separator.linear(floatArrayOf(0f, g.toFloat(), 0f))
        assertTrue("rest reads near zero, was ${rest.toList()}", rest.all { kotlin.math.abs(it) < 0.01 })

        // A sharp lateral shove leaves gravity on y and adds acceleration on x.
        val shove = separator.linear(floatArrayOf(2 * g.toFloat(), g.toFloat(), 0f))
        assertTrue("the shove appears on x", shove[0] > 1.0)
        assertTrue("gravity stays cancelled on y", kotlin.math.abs(shove[1]) < 0.01)
    }
}
