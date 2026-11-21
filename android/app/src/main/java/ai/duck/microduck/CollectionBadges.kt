package ai.duck.microduck

/**
 * One badge on the collection page: a goal the duck's own lifetime record
 * either meets or does not, with the real count behind it. Progress is always
 * the honest pair current/target — 7/13, 128/1000 — never a percentage this
 * side made up.
 */
internal data class CollectionBadge(
    val id: String,
    val unlocked: Boolean,
    val progress: Int,
    val target: Int,
)

/**
 * The badge shelf above the collection's two lists (G3). Everything is
 * computed from GET /collection's own rows and counts: a mood or trick the
 * duck has never done contributes zero, and no badge can unlock on a number
 * the core did not publish.
 *
 * The badges are deliberately not part of the wire contract: the core owns
 * counts, the shell owns the words. Renaming or reordering a badge is a copy
 * change, not a protocol change.
 */
internal object CollectionBadges {
    const val FIRST_MOOD = "first-mood"
    const val ALL_MOODS = "all-moods"
    const val FIRST_TRICK = "first-trick"
    const val ALL_TRICKS = "all-tricks"
    const val MOODS_10 = "moods-10"
    const val MOODS_100 = "moods-100"
    const val MOODS_1000 = "moods-1000"
    const val TRICKS_10 = "tricks-10"
    const val TRICKS_100 = "tricks-100"

    /**
     * The official release's thirteen moods: the denominator of "All 13
     * moods". Fixed on purpose — the badge's name says thirteen, and the
     * core publishes 13 as its own denominator too.
     */
    const val MOODS_TOTAL = 13

    /**
     * Every badge, in the order the page draws them. The two "all" badges use
     * the rows that were really published: an empty trick list is not a
     * graduation (`all {}` is true and would lie), and thirteen moods means
     * thirteen distinct mood rows with a positive count.
     */
    fun evaluate(book: CollectionUi): List<CollectionBadge> {
        // A duplicated row must not count twice: the core publishes one row
        // per name, but a fixture or a future core must not unlock a badge by
        // repeating the same mood.
        val moods = book.emotions.distinctBy { it.name }
        val tricks = book.skills.distinctBy { it.name }
        val moodsTried = moods.count { it.count > 0 }
        val tricksTried = tricks.count { it.count > 0 }
        val moodPlays = moods.sumOf { it.count }
        val trickPlays = tricks.sumOf { it.count }
        return listOf(
            CollectionBadge(FIRST_MOOD, moodsTried > 0, if (moodsTried > 0) 1 else 0, 1),
            CollectionBadge(ALL_MOODS, moodsTried >= MOODS_TOTAL, moodsTried, MOODS_TOTAL),
            CollectionBadge(FIRST_TRICK, tricksTried > 0, if (tricksTried > 0) 1 else 0, 1),
            CollectionBadge(
                ALL_TRICKS,
                tricks.isNotEmpty() && tricksTried == tricks.size,
                tricksTried,
                tricks.size,
            ),
            CollectionBadge(MOODS_10, moodPlays >= 10, moodPlays, 10),
            CollectionBadge(MOODS_100, moodPlays >= 100, moodPlays, 100),
            CollectionBadge(MOODS_1000, moodPlays >= 1000, moodPlays, 1000),
            CollectionBadge(TRICKS_10, trickPlays >= 10, trickPlays, 10),
            CollectionBadge(TRICKS_100, trickPlays >= 100, trickPlays, 100),
        )
    }
}
