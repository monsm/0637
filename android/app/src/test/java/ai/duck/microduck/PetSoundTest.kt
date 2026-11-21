package ai.duck.microduck

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the J3 sound-stroke half: the 48k -> 16k length contract,
 * the one-second framing boundary, the i16 little-endian base64 round trip,
 * and the ring that keeps the newest second under a slow network.
 */
class PetSoundTest {

    @Test
    fun `48k to 16k is one third long and box-averaged`() {
        val input = ShortArray(48_000) { 1_200 }
        val output = Pcm.downsample(input, 48_000, 16_000)
        assertEquals(16_000, output.size)
        assertTrue(output.all { it.toInt() == 1_200 })
    }

    @Test
    fun `downsample output length is floor of the rate ratio`() {
        assertEquals(0, Pcm.downsample(ShortArray(0), 48_000, 16_000).size)
        assertEquals(5_333, Pcm.downsample(ShortArray(16_000), 48_000, 16_000).size)
        // A source that is one sample short of a whole second stays short;
        // the missing tail is never padded.
        assertEquals(15_999, Pcm.downsample(ShortArray(47_999), 48_000, 16_000).size)
    }

    @Test
    fun `identity rate keeps every sample including odd lengths`() {
        val input = ShortArray(16_001) { it.toShort() }
        val output = Pcm.downsample(input, 16_000, 16_000)
        assertEquals(16_001, output.size)
        assertEquals(input.toList(), output.toList())
    }

    @Test
    fun `a frame is exactly one second or nothing`() {
        // 16 kHz mono i16: one second is 16 000 samples = 32 000 bytes on
        // the wire; the contract's 32000 is that byte count.
        assertNull(Pcm.frame(ShortArray(0)))
        assertNull(Pcm.frame(ShortArray(15_999)))
        assertEquals(16_000, Pcm.frame(ShortArray(16_000))!!.size)
        val longer = ShortArray(16_001) { it.toShort() }
        val frame = Pcm.frame(longer)!!
        assertEquals(16_000, frame.size)
        // The oldest sample was dropped, not the newest.
        assertEquals(1, frame[0].toInt())
        assertEquals(16_000, frame[15_999].toInt())
    }

    @Test
    fun `base64 round trip preserves every i16 including the extremes`() {
        val samples = shortArrayOf(
            Short.MIN_VALUE, -1, 0, 1, Short.MAX_VALUE, 1_234, -2_345,
        )
        val encoded = Pcm.base64(samples)
        assertEquals(samples.toList(), Pcm.decodeBase64(encoded).toList())
    }

    @Test
    fun `little endian bytes are the wire order`() {
        val bytes = Pcm.littleEndian(shortArrayOf(0x0102))
        assertArrayEquals(byteArrayOf(0x02, 0x01), bytes)
    }

    @Test
    fun `sensor body wraps the PCM in the microphone envelope J1 reads`() {
        val samples = shortArrayOf(1, -2, 3)
        val body = Pcm.sensorBody(
            samples,
            rms = 0.125,
            peakHz = 440.5,
            tickMs = 1_700_000_000_000L,
        )
        val json = JSONObject(body)
        assertEquals("microphone", json.getString("sensor"))
        assertEquals("twin", json.getString("mode"))
        assertEquals(1_700_000_000_000L, json.getLong("tick_ms"))
        val value = json.getJSONObject("value")
        assertEquals(0.125, value.getDouble("rms"), 1e-9)
        assertEquals(440.5, value.getDouble("peak_hz"), 1e-9)
        assertEquals(16_000, value.getInt("rate"))
        assertEquals(Pcm.base64(samples), value.getString("pcm16"))
        assertEquals(
            samples.toList(),
            Pcm.decodeBase64(value.getString("pcm16")).toList(),
        )
    }

    @Test
    fun `an ordinary RMS tick has no PCM fields`() {
        val json = JSONObject(Pcm.sensorBody(null, rms = 0.02, peakHz = 300.0, tickMs = 42L))
        assertEquals("microphone", json.getString("sensor"))
        assertEquals("twin", json.getString("mode"))
        val value = json.getJSONObject("value")
        assertEquals(0.02, value.getDouble("rms"), 1e-9)
        assertEquals(300.0, value.getDouble("peak_hz"), 1e-9)
        assertFalse(value.has("pcm16"))
        assertFalse(value.has("rate"))
    }

    @Test
    fun `ring keeps the newest samples when it overflows`() {
        val ring = ShortRing(4)
        ring.write(shortArrayOf(1, 2, 3), 3)
        assertNull(ring.last(4))
        ring.write(shortArrayOf(4, 5), 2)
        assertEquals(shortArrayOf(2, 3, 4, 5).toList(), ring.last(4)!!.toList())
        assertEquals(shortArrayOf(5).toList(), ring.last(1)!!.toList())
        assertEquals(shortArrayOf(2, 3, 4, 5).toList(), ring.snapshot(10).toList())
    }

    @Test
    fun `ring handles a write larger than its capacity`() {
        val ring = ShortRing(4)
        ring.write(shortArrayOf(1, 2, 3, 4, 5, 6), 6)
        assertEquals(shortArrayOf(3, 4, 5, 6).toList(), ring.last(4)!!.toList())
    }
}