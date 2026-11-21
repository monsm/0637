package ai.duck.microduck

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Dance Studio: record what the child actually did and replay the same
 * actions through the shell's existing buttons/intents. No new core path.
 */
data class DanceStep(
    val kind: String,
    val value: String = "",
    val label: String = "",
)

class DanceStudio(context: Context) {
    private val prefs = context.getSharedPreferences("duck", Context.MODE_PRIVATE)
    private val limit = 12
    var steps = mutableListOf<DanceStep>()
        private set
    var recording = false
        private set
    var playing = false
        private set
    var activeStep = -1
        private set

    init {
        val raw = prefs.getString("dance_v1", null) ?: "[]"
        try {
            val array = JSONArray(raw)
            for (i in 0 until minOf(array.length(), limit)) {
                val item = array.optJSONObject(i) ?: continue
                steps.add(
                    DanceStep(
                        item.optString("kind", ""),
                        item.optString("value", ""),
                        item.optString("label", ""),
                    )
                )
            }
        } catch (_: Throwable) {
        }
    }

    fun toggleRecording(): Boolean {
        if (playing) stop()
        recording = !recording
        if (!recording) save()
        return recording
    }

    fun record(kind: String, value: String = "", label: String = "") {
        if (!recording || playing || steps.size >= limit) return
        steps.add(DanceStep(kind, value, label))
        if (steps.size >= limit) {
            recording = false
            save()
        }
    }

    fun clear() {
        steps.clear()
        recording = false
        activeStep = -1
        prefs.edit().remove("dance_v1").commit()
    }

    fun stop() {
        playing = false
        activeStep = -1
    }

    /** Returns the steps to play; the UI owns the 0.8 s pacing and highlight. */
    fun beginPlay(): List<DanceStep>? {
        if (steps.isEmpty() || playing) return null
        recording = false
        playing = true
        activeStep = -1
        return steps.toList()
    }

    fun finished() {
        playing = false
        activeStep = -1
    }

    private fun save() {
        val array = JSONArray()
        steps.forEach { step ->
            array.put(
                JSONObject()
                    .put("kind", step.kind)
                    .put("value", step.value)
                    .put("label", step.label)
            )
        }
        prefs.edit().putString("dance_v1", array.toString()).commit()
    }
}
