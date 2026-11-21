package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the vet book card's data mapping.
 *
 * The pinned rules are the honesty ones the iOS ticket's parser keeps too
 * (ios/DuckApp/VetbookCard.swift, core export.rs is the wire contract):
 * a field the document does not carry stays absent — it never becomes a 0;
 * a real zero the document does carry shows as 0; a life with nothing in
 * it says "no records yet" instead of dressing zeros as achievements; and
 * a document the core did not write maps to no card at all.
 */
class VetbookCardTest {

    /** A document the core could have written, with only the given blocks. */
    private fun doc(highlights: String = "", life: String = "", care: String = "", fingerprint: String = ""): String {
        val build = if (fingerprint.isEmpty()) "" else """, "build": {"fingerprint": "$fingerprint"}"""
        val lifeBlock = if (life.isEmpty()) "" else """, "life": {$life}"""
        val careBlock = if (care.isEmpty()) "" else """, "care": {$care}"""
        return """{"format": "microduck-vetbook/1", "highlights": {$highlights}$build$lifeBlock$careBlock}"""
    }

    /** Fail with the document in the message, so a red test says what broke. */
    private fun parsed(text: String): VetbookCardModel =
        requireNotNull(parseVetbookCard(text)) { "not a vet book: $text" }

    @Test
    fun `the export's highlights map onto the card numbers`() {
        val model = parsed(
            doc(
                highlights = """"metres":123.4,"rolls":3,"pickups":5,"falls":1,"pets":7,"fed":2,"claps":4,"whistles":1""",
                fingerprint = "0123abcd4567ef89",
            ),
        )
        assertEquals(123.4, model.metres!!, 0.0001)
        assertEquals(3, model.rolls)
        assertEquals(5, model.pickups)
        assertEquals(1, model.falls)
        assertEquals(7, model.pets)
        assertEquals(2, model.fed)
        assertEquals(4, model.claps)
        assertEquals(1, model.whistles)
        // The hash is cut to the first eight digits, as the footer shows.
        assertEquals("0123abcd", model.fingerprint)
    }

    @Test
    fun `every counter present at zero is a fact and shows as zero`() {
        val model = parsed(
            doc(highlights = """"metres":0.0,"rolls":0,"pickups":0,"falls":0,"pets":0,"fed":0,"claps":0,"whistles":0"""),
        )
        // An all-zero life is honestly empty: the card says so in words.
        assertTrue(model.isEmpty)
        // But the facts the document carried are kept — the empty state is
        // chosen on the whole model, not by dropping fields.
        assertEquals(0, model.rolls)
        assertEquals(0.0, model.metres!!, 0.0001)
    }

    @Test
    fun `half a centimetre of drift is not a walk`() {
        assertTrue(parsed(doc(highlights = """"metres":0.04""")).isEmpty)
    }

    @Test
    fun `one real event breaks the empty state and renders its tile`() {
        val model = parsed(doc(highlights = """"rolls":2"""))
        assertFalse(model.isEmpty)
        val tiles = vetbookTiles(model)
        assertEquals(listOf<Pair<String, String>>("2" to Copy.rolls), tiles)
    }

    @Test
    fun `a field the document does not carry never becomes a zero`() {
        val model = parsed(doc(highlights = """"metres":2.5"""))
        assertEquals(2.5, model.metres!!, 0.0001)
        // Absent: no tile, no invented 0.
        assertNull(model.rolls)
        assertNull(model.fed)
        assertNull(model.whistles)
        val tiles = vetbookTiles(model)
        assertEquals(1, tiles.size)
        assertEquals("2.5 m", tiles[0].first)
    }

    @Test
    fun `metres render one decimal and the life record's order holds`() {
        val model = parsed(
            doc(highlights = """"metres":12.34,"rolls":1,"pickups":2,"falls":3,"pets":4,"fed":5,"claps":6,"whistles":7"""),
        )
        assertEquals(
            listOf(
                "12.3 m" to Copy.walked,
                "1" to Copy.rolls,
                "2" to Copy.pickups,
                "3" to Copy.vetbookFalls,
                "4" to Copy.vetbookPets,
                "5" to Copy.vetbookFed,
                "6" to Copy.vetbookClaps,
                "7" to Copy.vetbookWhistles,
            ),
            vetbookTiles(model),
        )
    }

    @Test
    fun `the care percentages are the core markdown's own derivation`() {
        val model = parsed(doc(care = """"hunger":0.15,"mood":0.5,"bond":0.35"""))
        // fullness = (1 - hunger) * 100; mood rescaled from -1…1; bond as-is.
        // The values avoid a binary tie: both platforms round the same
        // product the same way, so the cards cannot disagree.
        assertEquals(85, model.fullness)
        assertEquals(75, model.mood)
        assertEquals(35, model.bond)
    }

    @Test
    fun `a bad care reading cannot print 140 percent`() {
        val model = parsed(doc(care = """"hunger":-0.9,"mood":9.0,"bond":1.7"""))
        assertEquals(100, model.fullness)
        assertEquals(100, model.mood)
        assertEquals(100, model.bond)
    }

    @Test
    fun `powered seconds become hours`() {
        val model = parsed(doc(life = """"powered_s":7200.0"""))
        assertEquals(2.0, model.poweredHours!!, 0.0001)
    }

    @Test
    fun `a document the core did not write maps to no card`() {
        assertNull(parseVetbookCard("not json at all"))
        // Parseable but foreign: an empty model the empty-state covers —
        // never a card of invented numbers.
        val foreign = parseVetbookCard("""{"format":"other/9"}""")
        assertTrue(foreign != null)
        assertTrue(foreign!!.isEmpty)
    }

    @Test
    fun `an absent build block leaves the hash absent, not fake`() {
        val bare = parsed(doc(highlights = """"metres":1.0"""))
        assertNull(bare.fingerprint)
    }
}
