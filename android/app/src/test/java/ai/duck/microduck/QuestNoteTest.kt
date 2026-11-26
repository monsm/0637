package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The game cards' quest note: a pure read of the real board. The mapping is
 * the core's own blueprint ids (`quests.rs` BLUEPRINTS); a game with no such
 * quest on the board says nothing, and a done quest says so in its own words.
 */
class QuestNoteTest {
    private fun board(vararg quests: QuestUi) = QuestBoardUi(quests = quests.toList())

    @Test
    fun eachGameNamesItsOwnQuest() {
        val board = board(
            QuestUi("skills", "做 2 个动作", 0.5f, false),
            QuestUi("moods", "做 2 个表情", 0f, false),
            QuestUi("clap", "拍手两次", 0f, false),
            QuestUi("walk", "一起走 2 米", 1f, true),
        )
        assertEquals(
            Copy.questNotePrefix + "做 2 个动作",
            gameQuestNote(board, "kick-rally"),
        )
        assertEquals(
            Copy.questNotePrefix + "做 2 个表情",
            gameQuestNote(board, "mood-charade"),
        )
        assertEquals(
            Copy.questNotePrefix + "拍手两次",
            gameQuestNote(board, "sound-simon"),
        )
        assertEquals(
            Copy.questNoteDonePrefix + "一起走 2 米",
            gameQuestNote(board, "red-light"),
        )
    }

    @Test
    fun anAbsentQuestOrAnUnmappedGameSaysNothing() {
        assertNull(gameQuestNote(QuestBoardUi(), "kick-rally"))
        val board = board(QuestUi("skills", "做 2 个动作", 0f, false))
        assertNull(gameQuestNote(board, "mood-charade"))
        assertNull(gameQuestNote(board, "ledger"))
        assertNull(gameQuestNote(board, "kick-rally-extra"))
    }

    @Test
    fun everyMappedIdIsACoreBlueprintId() {
        val blueprintIds = setOf(
            "walk", "roll", "pet", "hold", "feed", "clap",
            "whistle", "moods", "skills", "peck", "bond",
        )
        for (game in listOf("kick-rally", "mood-charade", "sound-simon", "red-light")) {
            val id = gameQuestId(game)
            assertEquals("$game must map to a blueprint id", true, id in blueprintIds)
        }
    }

    @Test
    fun theNoteFollowsTheBoardAsItChanges() {
        // The same card recomputes from the new board: no local copy.
        val before = board(QuestUi("skills", "做 2 个动作", 0.5f, false))
        val after = board(QuestUi("skills", "做 2 个动作", 1f, true))
        assertEquals(Copy.questNotePrefix + "做 2 个动作", gameQuestNote(before, "kick-rally"))
        assertEquals(Copy.questNoteDonePrefix + "做 2 个动作", gameQuestNote(after, "kick-rally"))
    }
}
