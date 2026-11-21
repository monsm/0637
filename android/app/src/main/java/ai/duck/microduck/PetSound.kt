package ai.duck.microduck

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.Base64
import java.util.Locale
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * The phone microphone as the official pet-detect input (#4 sound stroke).
 *
 * The core's contract is fixed by J1:
 * `POST /sensor {"sensor":"microphone","mode":"twin","value":{"rms":…,
 * "peak_hz":…,"pcm16":"<base64>","rate":16000},"tick_ms":…}`, one second per
 * audio frame = 16 000 i16 samples (32 000 bytes on the wire), mono 16 kHz
 * little-endian. The core accumulates a
 * window, runs the official `pet_detect.onnx`, and ignores the stream when
 * no model is staged. Nothing here reaches the duck's 61-D observation:
 * this is the room's sound, same boundary as the RMS feed.
 *
 * The recorder is already 16 kHz mono PCM16 (MicFeed), so the real path is
 * an identity copy. [downsample] exists for the contract and for devices or
 * future sources that hand us 48 kHz; it box-averages, which is the cheap
 * anti-alias the ticket asked for, and never changes the frame length after
 * the rate ratio is applied.
 */
internal object Pcm {
    const val SAMPLE_RATE = 16_000

    /** One contract frame: exactly one second, 16 000 samples (32 000 bytes). */
    const val FRAME_SAMPLES = SAMPLE_RATE

    /**
     * Box-average [samples] from [fromRate] to [toRate]. Output length is
     * floor(in.length × toRate / fromRate): a partial trailing source window
     * is dropped, never padded into an invented sample.
     */
    fun downsample(samples: ShortArray, fromRate: Int, toRate: Int): ShortArray {
        require(fromRate > 0) { "fromRate must be positive" }
        require(toRate > 0) { "toRate must be positive" }
        if (samples.isEmpty()) return ShortArray(0)
        if (fromRate == toRate) return samples.copyOf()
        val length = (samples.size.toLong() * toRate / fromRate).toInt()
        val output = ShortArray(length)
        for (index in 0 until length) {
            val start = index.toLong() * fromRate / toRate
            val end = ((index + 1).toLong() * fromRate / toRate)
                .coerceAtMost(samples.size.toLong())
            if (end <= start) continue
            var sum = 0L
            for (source in start.toInt() until end.toInt()) sum += samples[source]
            output[index] = (sum.toDouble() / (end - start))
                .roundToInt()
                .coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt())
                .toShort()
        }
        return output
    }

    /**
     * The newest [FRAME_SAMPLES], or null when the ring does not hold a full
     * second yet. Dropping the partial second (rather than sending 31 999
     * samples labeled 16 kHz) is what keeps the core's window honest.
     */
    fun frame(samples: ShortArray): ShortArray? =
        if (samples.size < FRAME_SAMPLES) {
            null
        } else {
            samples.copyOfRange(samples.size - FRAME_SAMPLES, samples.size)
        }

    /** i16 little-endian bytes, the wire's own sample order. */
    fun littleEndian(samples: ShortArray): ByteArray {
        val bytes = ByteArray(samples.size * 2)
        var index = 0
        for (sample in samples) {
            val value = sample.toInt()
            bytes[index++] = (value and 0xff).toByte()
            bytes[index++] = ((value shr 8) and 0xff).toByte()
        }
        return bytes
    }

    fun base64(samples: ShortArray): String =
        Base64.getEncoder().encodeToString(littleEndian(samples))

    /**
     * The exact POST /sensor envelope J1 reads: the core routes on
     * `sensor:"microphone"` and takes the audio at `value.pcm16`.
     *
     * The RMS tick and the once-a-second sound-stroke window share one
     * request, so the phone makes no second connection and the core sees a
     * reading and its audio atomically. [samples] null is the ordinary 4 Hz
     * RMS tick; a non-null one adds `pcm16` and `rate` inside the same
     * `value` object. [tickMs] is the wall clock the existing feed already
     * publishes.
     */
    fun sensorBody(
        samples: ShortArray?,
        rms: Double,
        peakHz: Double,
        tickMs: Long,
        rate: Int = SAMPLE_RATE,
    ): String {
        val value = buildString {
            append("\"rms\":${String.format(Locale.US, "%.4f", rms)}")
            append(",\"peak_hz\":${String.format(Locale.US, "%.1f", peakHz)}")
            if (samples != null) {
                append(",\"pcm16\":\"${base64(samples)}\"")
                append(",\"rate\":$rate")
            }
        }
        return """{"sensor":"microphone","mode":"twin","value":{$value},"tick_ms":$tickMs}"""
    }

    /** DECODE is not used at runtime; it keeps the JVM round-trip test honest. */
    fun decodeBase64(encoded: String): ShortArray {
        val bytes = Base64.getDecoder().decode(encoded)
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        val samples = ShortArray(bytes.size / 2)
        for (index in samples.indices) samples[index] = buffer.getShort()
        return samples
    }
}

/**
 * A bounded i16 ring. The capture thread only appends; the RMS tick and the
 * PCM uploader copy what they need under the same lock. Overflow drops the
 * oldest samples, so a slow network never grows memory or stalls the mic.
 */
internal class ShortRing(private val capacity: Int) {
    init {
        require(capacity > 0) { "capacity must be positive" }
    }

    private val data = ShortArray(capacity)
    private var start = 0
    private var size = 0

    @Synchronized
    fun write(source: ShortArray, length: Int = source.size) {
        val count = min(length, source.size)
        if (count <= 0) return
        if (count >= capacity) {
            System.arraycopy(source, count - capacity, data, 0, capacity)
            start = 0
            size = capacity
            return
        }
        val drop = (size + count - capacity).coerceAtLeast(0)
        if (drop > 0) {
            start = (start + drop) % capacity
            size -= drop
        }
        var tail = (start + size) % capacity
        val first = min(count, capacity - tail)
        System.arraycopy(source, 0, data, tail, first)
        if (first < count) System.arraycopy(source, first, data, 0, count - first)
        size += count
    }

    /** The newest [count] samples, or null when fewer than [count] exist. */
    @Synchronized
    fun last(count: Int): ShortArray? {
        if (count <= 0) return ShortArray(0)
        if (size < count) return null
        return copyFrom((start + size - count) % capacity, count)
    }

    /** The newest [count] samples, or everything held when there are fewer. */
    @Synchronized
    fun snapshot(count: Int): ShortArray {
        val take = min(count, size)
        if (take <= 0) return ShortArray(0)
        return copyFrom((start + size - take) % capacity, take)
    }

    private fun copyFrom(from: Int, count: Int): ShortArray {
        val output = ShortArray(count)
        val first = min(count, capacity - from)
        System.arraycopy(data, from, output, 0, first)
        if (first < count) System.arraycopy(data, 0, output, first, count - first)
        return output
    }
}