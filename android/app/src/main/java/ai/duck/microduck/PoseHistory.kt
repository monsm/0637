package ai.duck.microduck

/**
 * Body poses at the last two ticks, for the renderer.
 *
 * MuJoCo steps at 50 Hz; a display draws at 60–120 Hz. Reading the live pose
 * each frame shows the duck in twenty-millisecond stairs. This keeps the two
 * most recent tick snapshots and blends between them with the fraction of a
 * tick that has elapsed.
 *
 * The loop thread writes; the GL thread reads. Both go through the same lock,
 * and MuJoCo itself is never read from the GL thread.
 */
class PoseHistory {
    private val lock = Any()
    private var previous = DoubleArray(0)
    private var current = DoubleArray(0)
    private var capturedAtNanos = 0L
    /// Wall time between the last two captures. The tick interval is 20 ms at
    /// 1x but 40 ms at 0.5x (and longer on a slow frame); blending with a fixed
    /// 20 ms would saturate alpha at 1 halfway through and freeze.
    private var intervalNanos = 20_000_000L
    private var bodyCount = 0

    /** Snapshot every body after a tick. Called from the loop thread. */
    fun capture(core: DuckCore) {
        synchronized(lock) {
            // A mode switch loads a different model with a different body
            // count; the old snapshots describe a body that no longer exists.
            val count = core.bodyCount()
            if (count != bodyCount) {
                bodyCount = count
                previous = DoubleArray(0)
                current = DoubleArray(0)
            }
            if (bodyCount == 0) return
            val next = DoubleArray(bodyCount * 7)
            for (body in 0 until bodyCount) {
                val pose = core.bodyPose(body) ?: continue
                if (pose.size < 7) continue
                System.arraycopy(pose, 0, next, body * 7, 7)
            }
            previous = current
            current = next
            val now = System.nanoTime()
            if (capturedAtNanos != 0L) {
                intervalNanos = (now - capturedAtNanos).coerceIn(1_000_000L, 200_000_000L)
            }
            capturedAtNanos = now
        }
    }

    /**
     * A body's interpolated pose: [x, y, z, qw, qx, qy, qz], or null before
     * the first capture.
     */
    fun interpolated(body: Int, nowNanos: Long = System.nanoTime()): DoubleArray? {
        synchronized(lock) {
            if (body < 0 || (body + 1) * 7 > current.size) return null
            val index = body * 7
            val b = DoubleArray(7)
            System.arraycopy(current, index, b, 0, 7)
            if (previous.size != current.size || capturedAtNanos == 0L) {
                return b
            }
            val elapsed = (nowNanos - capturedAtNanos).coerceAtLeast(0L)
            val alpha = (elapsed.toDouble() / intervalNanos.toDouble()).coerceIn(0.0, 1.0)
            val a = DoubleArray(7)
            System.arraycopy(previous, index, a, 0, 7)
            // Quaternions are double covers: q and -q are the same rotation.
            val dot = a[3] * b[3] + a[4] * b[4] + a[5] * b[5] + a[6] * b[6]
            val sign = if (dot < 0) -1.0 else 1.0
            val result = DoubleArray(7)
            for (axis in 0 until 7) {
                val target = if (axis >= 3) b[axis] * sign else b[axis]
                result[axis] = a[axis] + (target - a[axis]) * alpha
            }
            val norm = kotlin.math.sqrt(
                result[3] * result[3] + result[4] * result[4] +
                    result[5] * result[5] + result[6] * result[6]
            ).coerceAtLeast(1e-9)
            for (axis in 3 until 7) result[axis] /= norm
            return result
        }
    }
}
