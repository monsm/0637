package ai.duck.microduck

import kotlin.math.sqrt

/** Standard gravity, the bridge between CoreMotion's g and Android's m/s². */
internal const val STANDARD_GRAVITY = 9.80665

/**
 * The iOS shake rule, in one pure place.
 *
 * `DuckController.swift:873-887` reads CoreMotion's `userAcceleration` (in g,
 * gravity already removed), fires when the magnitude is above 1.2, keeps at
 * least 0.4 s between pushes, and sends each axis multiplied by 0.8 to
 * `duck_core_push`. Android's `TYPE_LINEAR_ACCELERATION` publishes m/s², so
 * this detector takes m/s², converts to g with the standard constant, and
 * applies the same threshold, hold-off and scale. The core's push is a trunk
 * velocity impulse (`runtime.rs` push → safety push); keeping the iOS unit
 * keeps the impulse the same size on both phones.
 */
internal class ShakeDetector(
    private val thresholdG: Double = 1.2,
    private val minIntervalS: Double = 0.4,
    private val scale: Double = 0.8,
) {
    private var lastPushS = Double.NEGATIVE_INFINITY

    /**
     * One accelerometer sample in m/s² at [nowS] (a monotonic clock). Null
     * below the threshold or inside the hold-off; otherwise the three values
     * to send to `DuckCore.push`.
     */
    fun onSample(ax: Double, ay: Double, az: Double, nowS: Double): DoubleArray? {
        val x = ax / STANDARD_GRAVITY
        val y = ay / STANDARD_GRAVITY
        val z = az / STANDARD_GRAVITY
        val magnitude = sqrt(x * x + y * y + z * z)
        if (magnitude <= thresholdG) return null
        if (nowS - lastPushS <= minIntervalS) return null
        lastPushS = nowS
        return doubleArrayOf(x * scale, y * scale, z * scale)
    }
}

/**
 * The fallback for a device without `TYPE_LINEAR_ACCELERATION`: Android's
 * documented low-pass estimate of gravity, subtracted from the raw
 * accelerometer. The raw sensor carries the same device axes CoreMotion
 * uses, so no orientation remap is needed.
 */
internal class GravitySeparator(private val alpha: Double = 0.8) {
    private val gravity = DoubleArray(3)

    fun linear(values: FloatArray): DoubleArray {
        if (values.size < 3) return doubleArrayOf(0.0, 0.0, 0.0)
        val out = DoubleArray(3)
        for (axis in 0..2) {
            gravity[axis] = alpha * gravity[axis] + (1 - alpha) * values[axis]
            out[axis] = values[axis] - gravity[axis]
        }
        return out
    }
}
