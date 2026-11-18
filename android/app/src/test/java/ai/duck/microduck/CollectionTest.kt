package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The collection page's pure half (F3): the F1 contract parses, an unknown
 * first day stays unknown, a zero count is the locked state, and the rows
 * sort by the real count. No Android APIs are touched here.
 */
class CollectionTest {

    private val contract = """
        {"ok":true,
         "emotions":[
           {"name":"curious","count":3,"first_day":"2026-11-18"},
           {"name":"laugh","count":0,"first_day":""}],
         "skills":[
           {"name":"roulade","count":5,"first_day":""},
           {"name":"sit","count":1,"first_day":"2026-11-01"}],
         "moods":{"unlocked":1,"total":13},
         "tricks":{"unlocked":2,"total":4}}
    """.trimIndent()

    @Test
    fun `collection parses the F1 contract`() {
        val ui = parseCollection(contract)!!
        assertEquals(
            listOf(
                CollectionEntry("curious", 3, "2026-11-18"),
                CollectionEntry("laugh", 0, ""),
            ),
            ui.emotions,
        )
        assertEquals(1, ui.moodsUnlocked)
        assertEquals(13, ui.moodsTotal)
        assertEquals(2, ui.tricksUnlocked)
        assertEquals(4, ui.tricksTotal)
        assertEquals(CollectionEntry("roulade", 5, ""), ui.skills.first())
    }

    @Test
    fun `an empty first day shows a dash and is never invented`() {
        val entry = CollectionEntry("roulade", 5, "")
        assertEquals("—", collectionFirstDay(entry))
        assertEquals("2026-11-18", collectionFirstDay(entry.copy(firstDay = "2026-11-18")))
        // A missing key parses as empty, which the UI also shows as —.
        val ui = parseCollection(
            """{"emotions":[{"name":"curious","count":2}],"skills":[]}"""
        )!!
        assertEquals("—", collectionFirstDay(ui.emotions.first()))
        // A JSON null is unknown too, not the word "null".
        val nullDay = parseCollection(
            """{"emotions":[{"name":"curious","count":2,"first_day":null}],"skills":[]}"""
        )!!
        assertEquals("", nullDay.emotions.first().firstDay)
        assertEquals("—", collectionFirstDay(nullDay.emotions.first()))
    }

    @Test
    fun `a zero count is locked and a real one is not`() {
        assertTrue(collectionLocked(CollectionEntry("laugh", 0, "")))
        assertFalse(collectionLocked(CollectionEntry("curious", 1, "")))
        val ui = parseCollection(contract)!!
        assertEquals(listOf("curious", "laugh"), ui.emotions.map { it.name })
        assertTrue(collectionLocked(ui.emotions.last()))
    }

    @Test
    fun `a locked row has no first-day line`() {
        // Even a stale core that still sends a day for a zero count must not
        // put a date on the locked row: only "Not tried yet" is shown.
        val locked = collectionRow(CollectionEntry("laugh", 0, "2026-11-01"))
        assertTrue(locked.locked)
        assertEquals(0, locked.count)
        assertNull(locked.firstDay)

        // An unlocked row still shows the day, or — when the core does not
        // know it; the display model is what the UI renders.
        assertEquals("2026-11-18", collectionRow(CollectionEntry("curious", 3, "2026-11-18")).firstDay)
        assertEquals("—", collectionRow(CollectionEntry("curious", 3, "")).firstDay)
    }

    @Test
    fun `rows sort by count descending then by name`() {
        val sorted = sortCollection(
            listOf(
                CollectionEntry("curious", 3, ""),
                CollectionEntry("laugh", 0, ""),
                CollectionEntry("roulade", 5, ""),
                CollectionEntry("kick_left", 3, ""),
            )
        )
        assertEquals(
            listOf("roulade", "curious", "kick_left", "laugh"),
            sorted.map { it.name },
        )
        // The parsed document is sorted too, not just the helper.
        assertEquals(listOf("roulade", "sit"), parseCollection(contract)!!.skills.map { it.name })
    }

    @Test
    fun `missing totals are counted from the core's own rows`() {
        val ui = parseCollection(
            """{"emotions":[{"name":"curious","count":3},{"name":"laugh","count":0}],
                "skills":[{"name":"roulade","count":5}]}"""
        )!!
        assertEquals(1, ui.moodsUnlocked)
        assertEquals(2, ui.moodsTotal)
        assertEquals(1, ui.tricksUnlocked)
        assertEquals(1, ui.tricksTotal)
    }

    @Test
    fun `a document that is not a collection answer is null`() {
        assertNull(parseCollection("""{"ok":true}"""))
        assertNull(parseCollection("""{"ok":false,"emotions":[],"skills":[]}"""))
        assertNull(parseCollection("not json"))
    }
}
