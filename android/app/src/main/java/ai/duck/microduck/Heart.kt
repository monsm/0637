package ai.duck.microduck

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager

/**
 * The duck's heartbeat, felt in the palm.
 *
 * The core publishes `care.heart_bpm` in /state — a pure reading of the
 * same mood/hunger/bond/asleep numbers the capsules show, never a new
 * event. This renders that reading as one short vibration per beat while
 * the drawer row is on; a device without a vibrator leaves the row honest
 * (the bpm still shows) and silent. The mapping lives on both platforms;
 * the duck never feels its own heart.
 *
 * One-shot vibrations on a thread the shell already owns: an
 * [VibrationEffect.createOneShot] per beat is what a toy's heartbeat is —
 * no waveform loop to stop, no pattern to leak past the toggle.
 */
class Heart(context: Context) {
    private val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        val manager = context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager
        manager?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
    }

    /** Main-thread only: whether the palm should feel the duck right now. */
    var enabled = false
        private set

    /** The last bpm polled; the row shows it. 0 before the first /state. */
    var bpm = 0.0
        private set

    private var lastBeatAt = 0L

    fun setEnabled(value: Boolean) {
        enabled = value
        if (!value) {
            bpm = 0.0
            try {
                vibrator?.cancel()
            } catch (_: Throwable) {
            }
        }
        lastBeatAt = 0L
    }

    /** One tick: if the next beat of the polled bpm is due, play it.
     *  Called from the 1 Hz state poll; a slower poll samples the heart
     *  coarsely, so the beat lands within a second of its due time. */
    fun tick(pollBpm: Double, nowMillis: Long) {
        if (!enabled || pollBpm <= 0.0) return
        bpm = pollBpm
        val intervalMillis = (60_000.0 / pollBpm).toLong().coerceAtLeast(300)
        val last = lastBeatAt
        if (last == 0L) {
            lastBeatAt = nowMillis
            return
        }
        if (nowMillis - last < intervalMillis) return
        lastBeatAt = nowMillis
        beat()
    }

    private fun beat() {
        val effect = VibrationEffect.createOneShot(45, 90)
        try {
            vibrator?.vibrate(effect)
        } catch (_: Throwable) {
        }
    }
}
