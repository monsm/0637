package ai.duck.microduck

import java.util.Calendar
import kotlin.math.max

/** One sample of the room's daylight: a light tint and an exposure. */
data class DaylightSample(val tint: FloatArray, val exposure: Float)

/**
 * The room's time of day, as light.
 *
 * The iOS stage shipped this as a spec-extra: the room is deep warm brown and
 * dim at night, warm gold at dawn, neutral at noon, orange-red at dusk. The
 * Android stage keeps the same curve so the two rooms agree. Two rules from
 * the design review are baked in here rather than left to the shader:
 *
 * - exposure never drops below 0.80 (night readability);
 * - the hour can be pinned, so screenshots and frame checks do not change
 *   with the clock of whoever runs CI.
 *
 * Pure functions: rendering passes an hour in, tests pass a fixed one.
 */
object Daylight {
    /** Set from a launch extra (`daylight_hour`) or a test. Null = local clock. */
    @Volatile var hourOverride: Float? = null

    private data class Stop(
        val hour: Float,
        val r: Float,
        val g: Float,
        val b: Float,
        val exposure: Float,
    )

    private val stops = listOf(
        Stop(0f, 0.95f, 0.72f, 0.55f, 0.80f),
        Stop(5f, 0.95f, 0.72f, 0.55f, 0.80f),
        Stop(6.5f, 1.12f, 0.92f, 0.72f, 0.90f),
        Stop(9f, 1.00f, 1.00f, 1.00f, 1.00f),
        Stop(16f, 1.00f, 1.00f, 1.00f, 1.00f),
        Stop(18f, 1.15f, 0.78f, 0.55f, 0.88f),
        Stop(20f, 1.05f, 0.80f, 0.62f, 0.80f),
        Stop(21.5f, 0.95f, 0.72f, 0.55f, 0.80f),
        Stop(24f, 0.95f, 0.72f, 0.55f, 0.80f),
    )

    private const val EXPOSURE_FLOOR = 0.80f

    @Volatile private var cachedMinute = Long.MIN_VALUE
    @Volatile private var cachedHour = 12f

    /** The local wall-clock hour as a fraction, cached once a minute. */
    fun localHour(nowMillis: Long = System.currentTimeMillis()): Float {
        hourOverride?.let { return it }
        val minute = nowMillis / 60_000L
        if (minute != cachedMinute) {
            // Calendar must be pinned to the requested instant; only using
            // nowMillis as a cache key made the parameter a lie.
            val calendar = Calendar.getInstance().apply { timeInMillis = nowMillis }
            cachedHour = calendar.get(Calendar.HOUR_OF_DAY).toFloat() +
                calendar.get(Calendar.MINUTE).toFloat() / 60f
            cachedMinute = minute
        }
        return cachedHour
    }

    /** Smoothstep between the anchors, so the room does not jump on the hour. */
    fun at(hour: Float): DaylightSample {
        val clamped = hour.coerceIn(0f, 24f)
        for (index in 0 until stops.size - 1) {
            val from = stops[index]
            val to = stops[index + 1]
            if (clamped < from.hour || clamped > to.hour) continue
            val span = to.hour - from.hour
            val raw = if (span > 0f) (clamped - from.hour) / span else 0f
            val t = raw * raw * (3f - 2f * raw)
            return DaylightSample(
                floatArrayOf(
                    from.r + (to.r - from.r) * t,
                    from.g + (to.g - from.g) * t,
                    from.b + (to.b - from.b) * t,
                ),
                max(from.exposure + (to.exposure - from.exposure) * t, EXPOSURE_FLOOR),
            )
        }
        return DaylightSample(floatArrayOf(stops[0].r, stops[0].g, stops[0].b), EXPOSURE_FLOOR)
    }

    /** The clear/backdrop base colour: warm brown horizon × the hour's light. */
    fun roomClear(hour: Float): FloatArray {
        val sample = at(hour)
        val base = floatArrayOf(0.102f, 0.090f, 0.078f)
        return floatArrayOf(
            base[0] * sample.tint[0] * sample.exposure,
            base[1] * sample.tint[1] * sample.exposure,
            base[2] * sample.tint[2] * sample.exposure,
        )
    }
}
