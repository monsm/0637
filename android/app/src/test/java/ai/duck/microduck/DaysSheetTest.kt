package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The days board's pure half (K3): the /days contract parses, a missing
 * field is a real zero, a missing row (or date) is never invented, the week
 * is the core's own sum, and the share text carries the real dates and
 * numbers in both languages. No Android APIs are touched here.
 */
class DaysSheetTest {

    /** The ticket's contract fixture, verbatim. */
    private val contract = """
        {"ok":true,
         "days":[{"day":"2026-11-18","metres":12.3,"powered_s":600.0,"rolls":2,"pets":5,
                  "pickups":1,"falls":0,"fed":2,"claps":3,"whistles":1,
                  "ground_picks":0,"sit_downs":1}],
         "today":{"day":"2026-11-19","metres":1.2,"powered_s":120.0,"rolls":0,
                  "pets":0,"pickups":0,"falls":0,"fed":0,"claps":0,"whistles":0},
         "week":{"metres":13.5,"powered_s":720.0,"rolls":2,"pets":5,"pickups":1,
                 "falls":0,"fed":2,"claps":3,"whistles":1}}
    """.trimIndent()

    @Test
    fun `the days contract parses into days, today and week`() {
        val board = parseDaysBoard(contract)!!
        assertEquals(
            listOf(
                DaysDay(
                    "2026-11-18", 12.3, 600.0, 2, 5, 1, 0, 2, 3, 1, 0, 1,
                ),
            ),
            board.days,
        )
        assertEquals("2026-11-19", board.today!!.day)
        assertEquals(1.2, board.today!!.metres, 0.0001)
        assertEquals(120.0, board.today!!.poweredS, 0.0001)
        assertFalse(board.isEmpty)
    }

    @Test
    fun `a missing field is a real zero, never a gap to fill in`() {
        // An older save may not carry every counter: the board reads what is
        // there as zero and keeps the day the core did send.
        val board = parseDaysBoard(
            """{"ok":true,"days":[{"day":"2026-11-10","metres":2.5}]}""",
        )!!
        assertEquals(
            DaysDay(
                "2026-11-10", 2.5, 0.0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ),
            board.days.single(),
        )
    }

    @Test
    fun `a row without a date is dropped, never given an invented one`() {
        val board = parseDaysBoard(
            """{"ok":true,"days":[{"metres":1.0},{"day":"  ","metres":2.0},
                {"day":"2026-11-08","metres":3.0}]}""",
        )!!
        assertEquals(listOf("2026-11-08"), board.days.map { it.day })
    }

    @Test
    fun `today missing means no today row, and a dayless today is none`() {
        val without = parseDaysBoard(
            """{"ok":true,"days":[{"day":"2026-11-18","metres":12.3}]}""",
        )!!
        assertNull(without.today)
        // A today object the core could not date is not a today row either.
        val dayless = parseDaysBoard(
            """{"ok":true,"days":[],"today":{"metres":1.0}}""",
        )!!
        assertNull(dayless.today)
    }

    @Test
    fun `a document that is not a days answer is null`() {
        assertNull(parseDaysBoard("not json"))
        assertNull(parseDaysBoard("""{"ok":false,"days":[]}"""))
        assertNull(parseDaysBoard("""{"ok":true}"""))
        // A `days` that is not an array is not an answer either.
        assertNull(parseDaysBoard("""{"ok":true,"days":"2026-11-18"}"""))
    }

    @Test
    fun `an empty board is the empty state and a today-only board is not`() {
        val empty = parseDaysBoard("""{"ok":true,"days":[]}""")!!
        assertTrue(empty.isEmpty)
        // One running day the core did send is real content: it shows, with
        // no week card behind it (today is not a finished day).
        val todayOnly = parseDaysBoard(
            """{"ok":true,"days":[],"today":{"day":"2026-11-19","metres":1.2}}""",
        )!!
        assertFalse(todayOnly.isEmpty)
        assertEquals("2026-11-19", todayOnly.today!!.day)
    }

    @Test
    fun `the week is the core's own sum, never recomputed here`() {
        // The core's 13.5 deliberately disagrees with the days' own 12.3:
        // the board keeps the core's number verbatim.
        val board = parseDaysBoard(contract)!!
        val week = board.week!!
        assertEquals(13.5, week.metres, 0.0001)
        assertEquals(720.0, week.poweredS, 0.0001)
        assertEquals(2, week.rolls)
        // And the share text shows the core's figure, not the daily sum.
        assertTrue(daysShareText(board).contains(daysMetresText(13.5)))
    }

    @Test
    fun `a missing week block means no week card, not an invented one`() {
        val board = parseDaysBoard(
            """{"ok":true,"days":[{"day":"2026-11-18","metres":12.3}]}""",
        )!!
        assertNull(board.week)
    }

    @Test
    fun `the figures use the iOS sheet's own formatting`() {
        assertEquals("2.0 h", daysPoweredText(7200.0))
        assertEquals("0.2 h", daysPoweredText(600.0))
        assertEquals("12.3 m", daysMetresText(12.34))
        assertEquals("0.0 m", daysMetresText(0.0))
    }

    @Test
    fun `the share text carries the real dates and numbers in both languages`() {
        val text = daysShareText(parseDaysBoard(contract)!!)
        // Real dates, the core's week total, and the today marker — in the
        // English and the Chinese words alike.
        assertTrue(text.contains("2026-11-18"))
        assertTrue(text.contains("Today (2026-11-19)"))
        assertTrue(text.contains("今天 (2026-11-19)"))
        assertTrue(text.contains("This week"))
        assertTrue(text.contains("这一周"))
        assertTrue(text.contains(daysMetresText(12.3)))
        assertTrue(text.contains(daysPoweredText(120.0)))
        // Nothing is invented: a day the core did not send appears nowhere.
        assertFalse(text.contains("2026-11-17"))
        assertFalse(text.contains("2026-11-20"))
    }
}
