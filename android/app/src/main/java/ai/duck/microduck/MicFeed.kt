package ai.duck.microduck

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.math.sqrt

/**
 * The phone's microphone as a *room* reading.
 *
 * The duck's own 61-D observation has no microphone in it; feeding room sound
 * straight into the policy would make it a different robot. So this is a twin
 * sensor, the same boundary `CameraBackdrop` draws: the level and the dominant
 * frequency are published to `/sensor` at 1 Hz for a client to read, and they
 * never reach the control loop.
 *
 * One `AudioRecord` on its own thread (`duck-mic`), mono 16 kHz PCM, read in
 * 2048-sample blocks. Each scheduled second it computes an RMS level and a
 * zero-crossing peak-frequency estimate over a short window and posts them. A
 * device without a microphone, or a denial, never throws: every call is guarded
 * and `stop()` releases the recorder.
 */
class MicFeed {
    private val scheduler = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "duck-mic").apply { priority = Thread.NORM_PRIORITY }
    }

    @Volatile private var recorder: AudioRecord? = null
    @Volatile private var running = false

    fun start() {
        if (running) return
        val buffer = openRecorder() ?: return
        running = true
        scheduler.scheduleAtFixedRate(
            {
                try {
                    measureAndPost(buffer)
                } catch (_: Throwable) {
                    // The intent server may not be up yet; next second retries.
                }
            },
            0,
            250,
            TimeUnit.MILLISECONDS,
        )
    }

    fun stop() {
        running = false
        try {
            recorder?.apply {
                if (recordingState == AudioRecord.RECORDSTATE_RECORDING) stop()
                release()
            }
        } catch (_: Throwable) {
            // A recorder that refuses to release must not crash the shell.
        }
        recorder = null
        scheduler.shutdownNow()
    }

    /** Open a 16 kHz mono PCM recorder; null when the platform refuses it. */
    private fun openRecorder(): ShortArray? {
        val bytes = AudioRecord.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        // The read is 2048 samples (4096 bytes); a buffer at or below that
        // would make `read` return short blocks that never fill a window.
        val bufferBytes = bytes.coerceAtLeast(READ_SAMPLES * 2)
        val recorder = try {
            AudioRecord.Builder()
                .setAudioSource(MediaRecorder.AudioSource.MIC)
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .setSampleRate(SAMPLE_RATE)
                        .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                        .build()
                )
                .setBufferSizeInBytes(bufferBytes)
                .build()
        } catch (_: Throwable) {
            return null
        }
        // Construction can succeed with a valid state but a dead source; only
        // start when the recorder reports it can actually record.
        if (recorder.state != AudioRecord.STATE_INITIALIZED) {
            runCatching { recorder.release() }
            return null
        }
        val samples = ShortArray(READ_SAMPLES)
        try {
            recorder.startRecording()
        } catch (_: Throwable) {
            runCatching { recorder.release() }
            return null
        }
        this.recorder = recorder
        return samples
    }

    /**
     * Accumulate a short window (a few 2048-sample reads), then compute the
     * level and dominant frequency and post them. Reading more than one block
     * per tick gives an estimate over a few hundred milliseconds rather than a
     * single 128 ms slice, while still leaving the thread idle between ticks.
     */
    private fun measureAndPost(samples: ShortArray) {
        val recorder = this.recorder ?: return
        if (recorder.recordingState != AudioRecord.RECORDSTATE_RECORDING) return
        val window = ShortArray(READ_SAMPLES * WINDOW_READS)
        var total = 0
        for (block in 0 until WINDOW_READS) {
            val read = recorder.read(samples, 0, READ_SAMPLES)
            if (read <= 0) break
            System.arraycopy(samples, 0, window, total, read)
            total += read
        }
        if (total <= 0) return
        var sumSq = 0.0
        var crossings = 0
        var previous = window[0].toInt()
        for (index in 1 until total) {
            val current = window[index].toInt()
            sumSq += current * current
            if ((previous < 0 && current >= 0) || (previous >= 0 && current < 0)) {
                crossings++
            }
            previous = current
        }
        // RMS normalized to -1..1; a 16-bit PCM sample is 1/32768 of full scale.
        val rms = sqrt(sumSq / total) / FULL_SCALE
        // Zero-crossing peak frequency: each full cycle crosses zero twice.
        val seconds = total.toDouble() / SAMPLE_RATE
        val peakHz = if (seconds > 0) crossings.toDouble() / (2.0 * seconds) else 0.0
        val body =
            """{"sensor":"microphone","mode":"twin","value":{"rms":${"%.4f".format(rms)},"peak_hz":${"%.1f".format(peakHz)}},"tick_ms":${System.currentTimeMillis()}}"""
        post(body)
    }

    private fun post(body: String) {
        val connection = URL("http://127.0.0.1:8765/sensor")
            .openConnection() as HttpURLConnection
        connection.requestMethod = "POST"
        connection.doOutput = true
        connection.setRequestProperty("Content-Type", "application/json")
        connection.connectTimeout = 2000
        connection.readTimeout = 2000
        connection.outputStream.use { it.write(body.toByteArray()) }
        connection.responseCode
        connection.disconnect()
    }

    private companion object {
        const val SAMPLE_RATE = 16_000
        const val READ_SAMPLES = 2048
        const val WINDOW_READS = 1
        const val FULL_SCALE = 32768.0
    }
}
