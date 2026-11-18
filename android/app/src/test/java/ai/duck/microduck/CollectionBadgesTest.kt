package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The badge shelf's pure half (G3): nine badges computed only from the real
 * /collection rows. These tests pin the all-locked, partial and all-unlocked
 * states, the exact thresholds (10 / 100 / 1000 plays), and the empty-list
 * edge where a plain `all {}` would graduate a duck that has no tricks.
 */
class CollectionBadgesTest {

    private fun moods(vararg pairs: Pair<String, Int>): List<CollectionEntry> =
        pairs.map { (name, count) -> CollectionEntry(name, count, "") }

    private fun tricks(vararg pairs: Pair<String, Int>): List<CollectionEntry> =
        pairs.map { (name, count) -> CollectionEntry(name, count, "") }

    private fun book(
        emotions: List<CollectionEntry> = emptyList(),
        skills: List<CollectionEntry> = emptyList(),
    ) = CollectionUi(
        emotions = emotions,
        skills = skills,
        moodsUnlocked = emotions.count { it.count > 0 },
        moodsTotal = CollectionBadges.MOODS_TOTAL,
        tricksUnlocked = skills.count { it.count > 0 },
        tricksTotal = skills.size,
    )

    private fun badge(book: CollectionUi, id: String): CollectionBadge =
        CollectionBadges.evaluate(book).first { it.id == id }

    @Test
    fun `a duck that has done nothing has every badge locked at zero`() {
        val badges = CollectionBadges.evaluate(CollectionUi())
        assertEquals(9, badges.size)
        assertTrue(badges.none { it.unlocked })

        val firstMood = badge(CollectionUi(), CollectionBadges.FIRST_MOOD)
        assertFalse(firstMood.unlocked)
        assertEquals(0, firstMood.progress)
        assertEquals(1, firstMood.target)

        val allMoods = badge(CollectionUi(), CollectionBadges.ALL_MOODS)
        assertFalse(allMoods.unlocked)
        assertEquals(0, allMoods.progress)
        assertEquals(13, allMoods.target)

        // `all {}` on an empty list is true; a duck with no tricks listed has
        // not graduated from trick school.
        val allTricks = badge(CollectionUi(), CollectionBadges.ALL_TRICKS)
        assertFalse("an empty trick list is not a graduation", allTricks.unlocked)
        assertEquals(0, allTricks.progress)
        assertEquals(0, allTricks.target)
    }

    @Test
    fun `a partial record unlocks only what it really reached`() {
        val partial = book(
            moods("curious" to 3, "laugh" to 0),
            tricks("roulade" to 5, "sit" to 0),
        )
        val firstMood = badge(partial, CollectionBadges.FIRST_MOOD)
        assertTrue(firstMood.unlocked)
        assertEquals(1, firstMood.progress)
        assertEquals(1, firstMood.target)

        val allMoods = badge(partial, CollectionBadges.ALL_MOODS)
        assertFalse(allMoods.unlocked)
        assertEquals(1, allMoods.progress)
        assertEquals(13, allMoods.target)

        val firstTrick = badge(partial, CollectionBadges.FIRST_TRICK)
        assertTrue(firstTrick.unlocked)
        assertEquals(1, firstTrick.progress)

        val allTricks = badge(partial, CollectionBadges.ALL_TRICKS)
        assertFalse(allTricks.unlocked)
        assertEquals(1, allTricks.progress)
        assertEquals(2, allTricks.target)

        assertEquals(3, badge(partial, CollectionBadges.MOODS_10).progress)
        assertFalse(badge(partial, CollectionBadges.MOODS_10).unlocked)
        assertEquals(5, badge(partial, CollectionBadges.TRICKS_10).progress)
        assertFalse(badge(partial, CollectionBadges.TRICKS_10).unlocked)
    }

    @Test
    fun `a full record unlocks all nine badges with real totals`() {
        // 988 + 12 x 1 = 1000 mood plays, 4 x 25 = 100 trick plays.
        val thirteen = (1..13).map { index ->
            CollectionEntry("mood$index", if (index == 1) 988 else 1, "")
        }
        val full = book(
            thirteen,
            tricks("roulade" to 25, "kick_left" to 25, "kick_right" to 25, "crouch" to 25),
        )
        val badges = CollectionBadges.evaluate(full)
        assertTrue(badges.all { it.unlocked })
        assertEquals(13, badge(full, CollectionBadges.ALL_MOODS).progress)
        assertEquals(13, badge(full, CollectionBadges.ALL_MOODS).target)
        assertEquals(4, badge(full, CollectionBadges.ALL_TRICKS).progress)
        assertEquals(4, badge(full, CollectionBadges.ALL_TRICKS).target)
        assertEquals(1000, badge(full, CollectionBadges.MOODS_1000).progress)
        assertEquals(1000, badge(full, CollectionBadges.MOODS_1000).target)
        assertEquals(100, badge(full, CollectionBadges.TRICKS_100).progress)
        assertEquals(100, badge(full, CollectionBadges.TRICKS_100).target)
    }

    @Test
    fun `mood milestones unlock exactly at their threshold`() {
        assertFalse(badge(book(moods("curious" to 9)), CollectionBadges.MOODS_10).unlocked)
        assertTrue(badge(book(moods("curious" to 10)), CollectionBadges.MOODS_10).unlocked)
        assertFalse(badge(book(moods("curious" to 99)), CollectionBadges.MOODS_100).unlocked)
        assertTrue(badge(book(moods("curious" to 100)), CollectionBadges.MOODS_100).unlocked)
        assertFalse(badge(book(moods("curious" to 999)), CollectionBadges.MOODS_1000).unlocked)
        assertTrue(badge(book(moods("curious" to 1000)), CollectionBadges.MOODS_1000).unlocked)
        // The progress is the real count, not a capped percentage.
        assertEquals(128, badge(book(moods("curious" to 128)), CollectionBadges.MOODS_1000).progress)
    }

    @Test
    fun `trick milestones unlock exactly at their threshold`() {
        assertFalse(badge(book(skills = tricks("roulade" to 9)), CollectionBadges.TRICKS_10).unlocked)
        assertTrue(badge(book(skills = tricks("roulade" to 10)), CollectionBadges.TRICKS_10).unlocked)
        assertFalse(badge(book(skills = tricks("roulade" to 99)), CollectionBadges.TRICKS_100).unlocked)
        assertTrue(badge(book(skills = tricks("roulade" to 100)), CollectionBadges.TRICKS_100).unlocked)
    }

    @Test
    fun `thirteen moods means thirteen distinct tried moods`() {
        val twelve = (1..12).map { CollectionEntry("mood$it", 1, "") }
        val missingOne = badge(book(twelve), CollectionBadges.ALL_MOODS)
        assertFalse(missingOne.unlocked)
        assertEquals(12, missingOne.progress)
        assertEquals(13, missingOne.target)

        val thirteen = twelve + CollectionEntry("mood13", 1, "")
        assertTrue(badge(book(thirteen), CollectionBadges.ALL_MOODS).unlocked)
        assertEquals("7/13", "${badge(book(twelve.take(7)), CollectionBadges.ALL_MOODS).progress}/13")
    }

    @Test
    fun `a duplicated row cannot double-count a badge`() {
        // A fixture (or a future core) repeating one mood must not turn seven
        // real plays into fourteen.
        val duplicated = book(
            moods("curious" to 7, "curious" to 7),
            tricks("roulade" to 6, "roulade" to 6),
        )
        assertEquals(1, badge(duplicated, CollectionBadges.ALL_MOODS).progress)
        assertFalse(badge(duplicated, CollectionBadges.MOODS_10).unlocked)
        assertEquals(7, badge(duplicated, CollectionBadges.MOODS_10).progress)
        assertFalse(badge(duplicated, CollectionBadges.TRICKS_10).unlocked)
        assertEquals(6, badge(duplicated, CollectionBadges.TRICKS_10).progress)
    }

    @Test
    fun `every badge carries a stable id and an honest progress pair`() {
        val partial = book(moods("curious" to 3), tricks("roulade" to 5))
        val badges = CollectionBadges.evaluate(partial)
        assertEquals(
            listOf(
                CollectionBadges.FIRST_MOOD,
                CollectionBadges.ALL_MOODS,
                CollectionBadges.FIRST_TRICK,
                CollectionBadges.ALL_TRICKS,
                CollectionBadges.MOODS_10,
                CollectionBadges.MOODS_100,
                CollectionBadges.MOODS_1000,
                CollectionBadges.TRICKS_10,
                CollectionBadges.TRICKS_100,
            ),
            badges.map { it.id },
        )
        assertTrue(badges.all { it.target > 0 })
        assertTrue(badges.all { it.progress >= 0 })
    }
}
