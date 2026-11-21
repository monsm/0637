package ai.duck.microduck

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sqrt

/**
 * The phone's microphone as a *room* reading.
 *
 * The duck's own 61-D observation has no microphone in it; feeding room sound
 * straight into the policy would make it a different robot. So this is a twin
 * sensor, the same boundary `CameraBackdrop` draws: the level and the dominant
 * frequency are published to `/sensor` at 4 Hz for a client to read, and they
 * never reach the control loop.
 *
 * One `AudioRecord` on its own reader thread (`duck-mic-read`), mono 16 kHz
 * PCM, read continuously into a 2-second ring. A scheduler on `duck-mic` keeps
 * the old cadence: every 250 ms it computes an RMS level and a zero-crossing
 * peak-frequency estimate over the newest 2048 samples and posts them.
 *
 * When [petSound] is on (the official `pet_detect.onnx` is staged), the same
 * continuous stream also feeds the sound-stroke contract: once a second the
 * newest exactly-16 000-sample frame (32 000 bytes) joins the very same
 * `/sensor` envelope as `value.pcm16` + `value.rate`, so the phone still makes
 * one request per tick. Its base64 and HTTP work run on `duck-pet-sound`,
 * never on the capture thread. One recorder, two readers: Android commonly
 * allows only one active AudioRecord, and the ticket's "coexist with RMS"
 * means exactly this sharing. If a previous frame is still in flight the
 * next tick posts RMS only instead of queueing; a slow network must not turn
 * into growing latency.
 *
 * A device without a microphone, or a denial, never throws: every call is
 * guarded and `stop()` releases the recorder.
 */
class MicFeed(private val petSound: Boolean = false) {
    private val scheduler = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "duck-mic").apply { priority = Thread.NORM_PRIORITY }
    }
    private val uplink = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "duck-pet-sound").apply {
            priority = Thread.NORM_PRIORITY
            isDaemon = true
        }
    }

    @Volatile private var recorder: AudioRecord? = null
    @Volatile private var running = false
    private var reader: Thread? = null

    private val lock = Any()
    private val pcm = ShortRing(Pcm.FRAME_SAMPLES * 2)
    private val analysis = ShortRing(READ_SAMPLES * 2)
    private val uploading = AtomicBoolean(false)
    @Volatile private var lastUploadAt = 0L

    fun start() {
        if (running) return
        val buffer = openRecorder() ?: return
        running = true
        reader = Thread({ readLoop(buffer) }, "duck-mic-read").apply {
            priority = Thread.NORM_PRIORITY
            isDaemon = true
            start()
        }
        scheduler.scheduleAtFixedRate(
            {
                try {
                    measureAndPost()
                } catch (_: Throwable) {
                    // The intent server may not be up yet; next tick retries.
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
        reader?.let { runCatching { it.join(500) } }
        reader = null
        scheduler.shutdownNow()
        uplink.shutdown()
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
     * Continuous capture. The old feed read one 2048-sample block per 250 ms
     * tick (~8 k samples/s), which starves a 16 kHz stream; the reader now
     * hands every block to the ring and the timed work is only analysis.
     */
    private fun readLoop(samples: ShortArray) {
        while (running) {
            val recorder = this.recorder ?: return
            if (recorder.recordingState != AudioRecord.RECORDSTATE_RECORDING) return
            val read = try {
                recorder.read(samples, 0, samples.size)
            } catch (_: Throwable) {
                -1
            }
            if (read > 0) {
                synchronized(lock) {
                    pcm.write(samples, read)
                    analysis.write(samples, read)
                }
            } else if (read < 0) {
                // A dead source ends the loop; the shell's toggle still owns
                // the visible state, and stop() releases the recorder.
                return
            }
        }
    }

    /**
     * Compute the level and dominant frequency over the newest window and
     * post the reading. Once a second the same envelope also carries the
     * PCM window (see [Pcm.sensorBody]).
     */
    private fun measureAndPost() {
        val recorder = this.recorder ?: return
        if (recorder.recordingState != AudioRecord.RECORDSTATE_RECORDING) return
        val window = synchronized(lock) { analysis.snapshot(READ_SAMPLES) }
        if (window.isEmpty()) return
        var sumSq = 0.0
        var crossings = 0
        var previous = window[0].toInt()
        for (index in 1 until window.size) {
            val current = window[index].toInt()
            sumSq += current * current
            if ((previous < 0 && current >= 0) || (previous >= 0 && current < 0)) {
                crossings++
            }
            previous = current
        }
        // RMS normalized to -1..1; a 16-bit PCM sample is 1/32768 of full scale.
        val rms = sqrt(sumSq / window.size) / FULL_SCALE
        // Zero-crossing peak frequency: each full cycle crosses zero twice.
        val seconds = window.size.toDouble() / SAMPLE_RATE
        val peakHz = if (seconds > 0) crossings.toDouble() / (2.0 * seconds) else 0.0
        val now = System.currentTimeMillis()
        // One request per tick. The ordinary 4 Hz reading is the same
        // envelope J1 routes on; once a second that envelope also carries
        // the pet-detect window. The base64 + HTTP for that larger body run
        // on `duck-pet-sound`, never on this scheduler or the capture thread.
        val frame = if (petSound && now - lastUploadAt >= UPLOAD_INTERVAL_MS) {
            synchronized(lock) { pcm.last(Pcm.FRAME_SAMPLES) }
        } else {
            null
        }
        if (frame != null && uploading.compareAndSet(false, true)) {
            lastUploadAt = now
            uplink.execute {
                try {
                    post(Pcm.sensorBody(frame, rms, peakHz, now))
                } catch (_: Throwable) {
                    // The core may not be listening yet; the next second retries.
                } finally {
                    uploading.set(false)
                }
            }
        } else {
            // RMS-only tick: either the turn is not due, or the previous
            // PCM upload is still in flight and this second's window is
            // dropped on purpose — the ring already holds the next one, so
            // a slow network delays the stream instead of growing it. The
            // level reading itself is never dropped.
            post(Pcm.sensorBody(null, rms, peakHz, now))
        }
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
        const val FULL_SCALE = 32768.0
        const val UPLOAD_INTERVAL_MS = 1000L
    }
}