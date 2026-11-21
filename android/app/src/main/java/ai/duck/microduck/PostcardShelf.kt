package ai.duck.microduck

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * The postcard shelf: a Director frame plus one honest line about what the
 * duck actually did. Everything is local; nothing is sent anywhere unless
 * the user presses Share.
 */
data class Postcard(
    val file: String,
    val caption: String,
    val day: String,
)

object PostcardShelf {
    private const val KEY = "postcards_v1"
    private const val MAX = 30

    fun dir(context: Context): File = File(context.filesDir, "postcards").apply { mkdirs() }

    fun load(context: Context): List<Postcard> {
        val raw = context.getSharedPreferences("duck", Context.MODE_PRIVATE)
            .getString(KEY, null) ?: return emptyList()
        return try {
            val array = JSONArray(raw)
            (0 until array.length()).mapNotNull { index ->
                val item = array.optJSONObject(index) ?: return@mapNotNull null
                Postcard(
                    item.optString("file", ""),
                    item.optString("caption", ""),
                    item.optString("day", ""),
                )
            }.filter { File(dir(context), it.file).exists() }
        } catch (_: Throwable) {
            emptyList()
        }
    }

    fun save(context: Context, card: Postcard) {
        val cards = load(context).toMutableList()
        cards.add(0, card)
        // Evicting the metadata alone would orphan the PNG in filesDir
        // forever; the picture goes with the entry, the way the iOS shelf
        // deletes on eviction.
        while (cards.size > MAX) {
            val evicted = cards.removeAt(cards.size - 1)
            File(dir(context), evicted.file).delete()
        }
        val array = JSONArray()
        cards.forEach { card ->
            array.put(
                JSONObject()
                    .put("file", card.file)
                    .put("caption", card.caption)
                    .put("day", card.day)
            )
        }
        context.getSharedPreferences("duck", Context.MODE_PRIVATE)
            .edit().putString(KEY, array.toString()).commit()
    }
}

/**
 * One deterministic line from the real deltas the shell already computes for
 * the away recap. Empty when nothing happened — a quiet night is allowed to
 * be a quiet line, not a fabricated one.
 */
fun postcardCaption(
    rituals: Int,
    falls: Int,
    feeds: Int,
    pets: Int,
    moods: Int,
    metres: Double,
): String {
    val parts = mutableListOf<String>()
    if (pets > 0) parts.add(Copy.awayPets(pets))
    if (feeds > 0) parts.add(Copy.awayFeeds(feeds))
    if (rituals > 0) parts.add(Copy.awayRituals(rituals))
    if (falls > 0) parts.add(Copy.awayFalls(falls))
    if (moods > 0) parts.add(Copy.awayMoods(moods))
    if (metres >= 0.5) parts.add(Copy.postcardMetres(metres))
    if (parts.isEmpty()) return Copy.postcardQuiet
    return parts.joinToString(" · ")
}
