package ai.duck.microduck

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Fixture tests for the loopback client's parsers (B8).
 *
 * The eight HTTP calls moved out of Shell.kt verbatim; these tests pin the
 * parsing half against the wire shapes the core really publishes, so the
 * move is provably isomorphic: same bytes in, same reading out.
 */
class IntentClientTest {

    private var previousLanguage = Copy.language

    @Before
    fun pinEnglish() {
        // skillLabel is language-dependent; pin one language so the
        // behaviour-summary assertion is exact rather than locale luck.
        previousLanguage = Copy.language
        Copy.language = Copy.Language.English
    }

    @After
    fun restoreLanguage() {
        Copy.language = previousLanguage
    }

    @Test
    fun `life events parse as JSON - a comma inside one event stays one event`() {
        val text = """{"life":{"events":["pet","walked 1.2m, then sat","gift:treat"]}}"""
        assertEquals(
            listOf("pet", "walked 1.2m, then sat", "gift:treat"),
            parseLifeEvents(text),
        )
        assertEquals(emptyList<String>(), parseLifeEvents("""{"tick":3}"""))
    }

    @Test
    fun `branch count reads the branches array length`() {
        assertEquals(3, parseBranchCount("""{"branches":[{},{},{}]}"""))
        assertEquals(0, parseBranchCount("""{"tick":3}"""))
    }

    @Test
    fun `face reads the leds block and ignores the pack battery`() {
        val text = """
            {"pack":{"battery_percent":5},
             "leds":{"status":"happy","bars":3,"battery_percent":72.4,"color":"warm"}}
        """.trimIndent()
        assertEquals(FaceUi("happy", 3, "72%", "warm"), parseFace(text))
        assertEquals(FaceUi(), parseFace("""{"tick":3}"""))
    }

    @Test
    fun `trick book counts emotion keys and skill executions`() {
        val text = """{"life":{"emotions":{"laugh":1,"sad":2},"skills":{"kick_left":3,"roulade":2}}}"""
        assertEquals("2/13 moods · 5 tricks", parseTrickBook(text))
        assertEquals("—", parseTrickBook("""{"tick":3}"""))
    }

    @Test
    fun `behaviour summary prefers the playing emotion, then the idle takeover`() {
        val emotion = """{"emotion":{"name":"yes_fast","elapsed_s":1.5,"duration_s":2.0}}"""
        // The label comes from skillLabel (locale-dependent by design); what
        // this test pins is that the parser reads the wire name and the two
        // clocks, not a hard-coded English word.
        val actualEmotion = parseBehaviorSummary(emotion)
        assertEquals(
            "behaviour line was '$actualEmotion' (language ${Copy.language})",
            "${skillLabel("yes_fast")} 1.5/2.0s",
            actualEmotion,
        )
        val idle = """{"idle":{"takeover":0.43}}"""
        assertEquals(Copy.lookingAround(43), parseBehaviorSummary(idle))
        assertEquals("—", parseBehaviorSummary("""{"tick":3}"""))
    }

    @Test
    fun `tof strip draws only a complete 8x8 grid`() {
        val cells = List(64) { "0.5" }.joinToString(",")
        val strip = parseTofStrip("""{"tof_grid":[$cells]}""")
        assertNotEquals("—", strip)
        val rows = strip.split(" / ")
        assertEquals(8, rows.size)
        assertTrue(rows.all { it.length == 8 && it.all { ch -> ch == '█' } })

        val short = List(63) { "0.5" }.joinToString(",")
        assertEquals("—", parseTofStrip("""{"tof_grid":[$short]}"""))
    }

    @Test
    fun `microphone summary needs both rms and peak`() {
        assertEquals(
            "rms 0.12 · peak_hz 940.5",
            parseMicSummary("""{"microphone":{"rms":0.12,"peak_hz":940.5}}"""),
        )
        assertEquals("—", parseMicSummary("""{"microphone":{"rms":0.12}}"""))
        assertEquals("—", parseMicSummary("""{"tick":3}"""))
    }
}
