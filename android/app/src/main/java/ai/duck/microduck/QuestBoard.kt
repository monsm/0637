package ai.duck.microduck

import android.content.Context
import org.json.JSONObject
import java.time.LocalDate

/** Three daily goals, read from GET /quests (the core's own counters). */
data class QuestUi(
    val id: String,
    val title: String,
    val progress: Float,
    val done: Boolean,
)

data class QuestBoardUi(
    val day: String = "",
    val quests: List<QuestUi> = emptyList(),
    val streak: Int = 0,
    val bestStreak: Int = 0,
    val completed: Int = 0,
)

private const val QUEST_PREFS = "duck_quests_v1"

/** The day the core last accepted, so the daily POST fires once per day. */
@Volatile private var lastPostedDay: String? = null

private fun parseQuests(text: String, chinese: Boolean): QuestBoardUi? = try {
    val board = JSONObject(text)
    val array = board.optJSONArray("quests") ?: org.json.JSONArray()
    val quests = (0 until array.length()).mapNotNull { index ->
        val item = array.optJSONObject(index) ?: return@mapNotNull null
        val title = if (chinese) item.optString("title_zh", "") else item.optString("title_en", "")
        QuestUi(
            id = item.optString("id", ""),
            title = title,
            progress = item.optDouble("progress", 0.0).toFloat().coerceIn(0f, 1f),
            done = item.optBoolean("done", false),
        )
    }
    QuestBoardUi(
        day = board.optString("day", ""),
        quests = quests,
        streak = board.optInt("streak", 0),
        bestStreak = board.optInt("best_streak", 0),
        completed = board.optInt("completed_today", 0),
    )
} catch (_: Throwable) {
    null
}

fun fetchQuests(): QuestBoardUi? = try {
    parseQuests(getJson("/quests"), Copy.language == Copy.Language.Chinese)
} catch (_: Throwable) {
    null
}

/** The phone's calendar day, which the core must know before it can pick goals. */
fun postQuestDay(context: Context) {
    val day = LocalDate.now().toString()
    // One post per day: the saver loop calls this every few seconds, and a
    // replayed POST would reset the core's board mid-day. The claim is set
    // before the thread starts so concurrent callers cannot double-fire; a
    // failed post clears it so a later call can retry.
    if (day == lastPostedDay) return
    lastPostedDay = day
    val saved = context.getSharedPreferences(QUEST_PREFS, Context.MODE_PRIVATE)
        .getString("board", null)
    val body = JSONObject()
        .put("day", day)
        .put("board", if (saved != null) JSONObject(saved) else JSONObject())
    Thread {
        try {
            if (!postJson("/quests/day", body.toString())) {
                lastPostedDay = null
            }
        } catch (_: Throwable) {
            lastPostedDay = null
        }
    }.start()
}

/** Keep the core's board across launch. Call from the same saver thread. */
fun saveQuestBoard(context: Context, stateText: String) {
    try {
        val quests = JSONObject(stateText).optJSONObject("quests") ?: return
        if (quests.optJSONArray("quests")?.length() == 0) return
        context.getSharedPreferences(QUEST_PREFS, Context.MODE_PRIVATE)
            .edit().putString("board", quests.toString()).commit()
    } catch (_: Throwable) {
    }
}

/** Parse the quests from the one /state the Shell already fetched. */
fun parseQuestsFromState(stateText: String): QuestBoardUi? =
    parseQuests(JSONObject(stateText).optJSONObject("quests")?.toString() ?: "{}",
        Copy.language == Copy.Language.Chinese)
