package ai.duck.microduck

import android.content.Context
import org.json.JSONObject

data class DexEntryUi(
    val name: String,
    val count: Int,
    val firstSeen: String?,
    val isEmotion: Boolean,
)

/** robotd's thirteen official emotions. */
private val emotionNames = setOf(
    "angry", "curious", "defiant", "devastated", "excited", "impatient",
    "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
)

/**
 * The collection page, built only from /state life counters. The first-seen
 * dates are a local keepsake stamped the first time this phone sees a
 * nonzero count; nothing here invents a move the duck did not do.
 */
object DexStore {
    private const val KEY = "dex_first_v1"

    fun build(
        context: Context,
        stateText: String,
        skills: List<String>,
        chinese: Boolean,
    ): List<DexEntryUi> {
        if (skills.isEmpty()) return emptyList()
        val life = try {
            JSONObject(stateText).optJSONObject("life") ?: return emptyList()
        } catch (_: Throwable) {
            return emptyList()
        }
        val skillCounts = life.optJSONObject("skills")
        val emotionCounts = life.optJSONObject("emotions")
        val prefs = context.getSharedPreferences("duck", Context.MODE_PRIVATE)
        val first = try {
            JSONObject(prefs.getString(KEY, "{}") ?: "{}")
        } catch (_: Throwable) {
            JSONObject()
        }
        val today = java.time.LocalDate.now().toString()
        var changed = false
        val entries = skills.map { name ->
            val skill = skillCounts?.optInt(name, 0) ?: 0
            val emotion = emotionCounts?.optInt(name, 0) ?: 0
            val count = maxOf(skill, emotion)
            if (count > 0 && !first.has(name)) {
                first.put(name, today)
                changed = true
            }
            DexEntryUi(
                name = name,
                count = count,
                firstSeen = if (first.has(name)) first.optString(name, "") else null,
                isEmotion = emotionNames.contains(name),
            )
        }
        if (changed) {
            prefs.edit().putString(KEY, first.toString()).commit()
        }
        return entries.sortedWith(
            compareBy<DexEntryUi> { it.isEmotion }
                .thenByDescending { it.count }
                .thenBy { it.name }
        )
    }
}
