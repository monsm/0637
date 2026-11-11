package ai.duck.microduck

import android.content.Context
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * The companion layer's shell side.
 *
 * The core owns hunger, mood and bond; the phone only reads them for the
 * sheet and hands them back across launches. Nothing here invents a value:
 * an unreachable core returns null and the UI keeps showing the last number
 * the duck actually published.
 */
data class CareUi(
    val hunger: Double = 0.15,
    val mood: Double = 0.35,
    val bond: Double = 0.1,
    val asleep: Boolean = false,
) {
    /** Fullness fills the bar left-to-right: full means fed. */
    val fullness: Float get() = (1.0 - hunger).toFloat().coerceIn(0f, 1f)
    val moodFraction: Float get() = ((mood + 1.0) / 2.0).toFloat().coerceIn(0f, 1f)
    val bondFraction: Float get() = bond.toFloat().coerceIn(0f, 1f)

    fun line(): String = Copy.careLine(hunger, mood, bond, asleep)
}

private const val PREFS = "duck"
private const val COMPANION_KEY = "duck_companion_v1"

fun fetchCare(): CareUi? = try {
    val text = getJson("/state")
    val care = JSONObject(text).optJSONObject("care") ?: return null
    CareUi(
        hunger = care.optDouble("hunger", 0.15),
        mood = care.optDouble("mood", 0.35),
        bond = care.optDouble("bond", 0.1),
        asleep = care.optBoolean("asleep", false),
    )
} catch (_: Throwable) {
    null
}

/**
 * Save the duck's inner life before Android can kill the process. The whole
 * /state is not kept - only the two objects the core can take back.
 */
fun saveCompanion(context: Context) {
    Thread {
        try {
            val state = JSONObject(getJson("/state"))
            val saved = JSONObject()
            state.optJSONObject("care")?.let { saved.put("care", it) }
            state.optJSONObject("life")?.let { saved.put("life", it) }
            if (saved.length() > 0) {
                context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                    .edit().putString(COMPANION_KEY, saved.toString()).apply()
            }
        } catch (_: Throwable) {
            // The server may not be up yet; the next pause tries again.
        }
    }.start()
}

/**
 * Hand the saved numbers back once the core is running. Both routes are
 * restore-only: the core clamps them, and the next /state poll shows what the
 * duck actually accepted.
 */
fun restoreCompanion(context: Context) {
    val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        .getString(COMPANION_KEY, null) ?: return
    Thread {
        try {
            val saved = JSONObject(raw)
            saved.optJSONObject("care")?.let { postJson("/care", it.toString()) }
            saved.optJSONObject("life")?.let { postJson("/life", it.toString()) }
        } catch (_: Throwable) {
            // A corrupt payload is dropped rather than crashing the launch.
        }
    }.start()
}

private fun getJson(path: String): String {
    val connection = URL("http://127.0.0.1:8765$path").openConnection() as HttpURLConnection
    connection.requestMethod = "GET"
    connection.connectTimeout = 1500
    connection.readTimeout = 1500
    try {
        return connection.inputStream.bufferedReader().use { it.readText() }
    } finally {
        connection.disconnect()
    }
}

private fun postJson(path: String, body: String) {
    val connection = URL("http://127.0.0.1:8765$path").openConnection() as HttpURLConnection
    connection.requestMethod = "POST"
    connection.doOutput = true
    connection.setRequestProperty("Content-Type", "application/json")
    connection.connectTimeout = 2000
    connection.readTimeout = 2000
    try {
        connection.outputStream.use { it.write(body.toByteArray()) }
        connection.responseCode
    } finally {
        connection.disconnect()
    }
}
