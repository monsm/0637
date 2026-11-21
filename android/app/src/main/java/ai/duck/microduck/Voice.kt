package ai.duck.microduck

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * The duck's speaker.
 *
 * The sound is the robot's own synth — `sounds/` compiled into the core — so
 * the app plays what the physical duck plays, not a recording. The core
 * decides when (`greet` as the loop comes up, `peck` before the servos go
 * quiet, a `chirp` when asked); this only renders the PCM and writes it to an
 * `AudioTrack`.
 *
 * One track, one thread: a quack is a few hundred milliseconds, and two at
 * once would be two ducks.
 */
class Voice(private val core: DuckCore) {
    private val scheduler = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "duck-voice").apply { priority = Thread.NORM_PRIORITY }
    }

    @Volatile private var track: AudioTrack? = null
    @Volatile private var playing = false

    private val rate: Int by lazy { DuckCore.voiceRate() }

    fun start() {
        scheduler.scheduleAtFixedRate(
            {
                try {
                    poll()
                } catch (_: Throwable) {
                    // A speaker that fails must not stop the control loop.
                }
            },
            0,
            100,
            TimeUnit.MILLISECONDS,
        )
    }

    fun stop() {
        scheduler.shutdownNow()
        release()
    }

    /** Ask for a tag now (the shell's quack). It goes through the core's
     *  queue, so it is the same path as a greet and each press is a new
     *  variant rather than the same sound twice. */
    fun play(tag: String) {
        core.queueSound(tag)
    }

    private fun poll() {
        if (playing) return
        val tag = core.nextSound() ?: return
        val pcm = core.renderSound(tag)
        if (pcm.isNotEmpty()) write(pcm)
    }

    private fun write(pcm: FloatArray) {
        val samples = ShortArray(pcm.size) { index ->
            (pcm[index].coerceIn(-1f, 1f) * Short.MAX_VALUE).toInt().toShort()
        }
        val audio = track ?: open(pcm.size * 2).also { track = it }
        playing = true
        try {
            audio.write(samples, 0, samples.size)
        } finally {
            playing = false
        }
    }

    private fun open(bytes: Int): AudioTrack {
        val size = maxOf(bytes, AudioTrack.getMinBufferSize(
            rate,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        ).coerceAtLeast(4096))
        return AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build()
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(rate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build()
            )
            .setBufferSizeInBytes(size)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
            .also { it.play() }
    }

    private fun release() {
        try {
            track?.stop()
            track?.release()
        } catch (_: Throwable) {
        }
        track = null
    }
}
