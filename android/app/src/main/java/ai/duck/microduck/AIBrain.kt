package ai.duck.microduck

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * The user's own model. The phone makes the call and executes the answer
 * through the same intent API a person uses; the core never talks to the
 * internet. Any OpenAI-compatible endpoint works, including a local Ollama.
 */
data class AIConfig(
    val enabled: Boolean = false,
    val endpoint: String = "",
    val model: String = "",
    val key: String = "",
    val interval: Int = 30,
    val persona: String = "",
)

object AIStore {
    private const val PREFS = "duck"
    private const val KEY = "duck_ai_config_v1"

    fun load(context: Context): AIConfig {
        val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY, null) ?: return AIConfig()
        return try {
            val json = JSONObject(raw)
            AIConfig(
                enabled = json.optBoolean("enabled", false),
                endpoint = json.optString("endpoint", ""),
                model = json.optString("model", ""),
                key = json.optString("key", ""),
                interval = json.optInt("interval", 30).coerceIn(10, 300),
                persona = json.optString("persona", ""),
            )
        } catch (_: Throwable) {
            AIConfig()
        }
    }

    fun save(context: Context, config: AIConfig) {
        val json = JSONObject()
            .put("enabled", config.enabled)
            .put("endpoint", config.endpoint)
            .put("model", config.model)
            .put("key", config.key)
            .put("interval", config.interval)
            .put("persona", config.persona)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY, json.toString()).apply()
    }
}

/** One validated decision: only the duck's own verbs, never free-form code. */
data class AIDecision(
    val action: String,
    val value: String = "",
    val vx: Double = 0.0,
    val vz: Double = 0.0,
    val seconds: Double = 1.0,
    val x: Double = 1.0,
    val y: Double = 0.0,
    val z: Double = 0.15,
    val why: String = "",
)

object AIBrain {
    private fun systemPrompt(persona: String, skills: List<String>): String = buildString {
        append("You are the brain of MicroDuck, a small two-legged robot duck living in a phone. ")
        append("You decide one action at a time from the duck's real state. ")
        append("Available skills: ${skills.joinToString(", ")}. ")
        append("Reply with ONE JSON object and nothing else: ")
        append("""{"action":"skill|sound|sit|stand|stop|look|move","value":"...","vx":0,"vz":0,"seconds":1,"x":1,"y":0,"z":0.15,"why":"short reason"}. """)
        append("skill/sound use value (a skill above, or chirp/coo/greet/inquire/peck). ")
        append("move uses vx (m/s, -0.6..0.6), vz (rad/s, -1..1) and seconds (0.2..2). ")
        append("look aims the head at x/y/z in the duck's own frame. sit/stand/stop need no value. ")
        append("Be gentle: never move for more than two seconds without looking again.")
        if (persona.isNotBlank()) append(" The person asks you to behave like: $persona")
    }

    /**
     * Ask the model once. Returns (decision, error) and never throws; network
     * and parsing failures are answers, not crashes. Call from a worker
     * thread.
     */
    fun think(
        config: AIConfig,
        stateJson: String,
        skills: List<String>,
        heard: String,
    ): Pair<AIDecision?, String?> {
        val endpointText = config.endpoint.trim()
        if (endpointText.isEmpty()) return null to "Set a model endpoint first"
        val endpoint = try {
            val url = URL(endpointText)
            if (url.path.contains("chat/completions")) url
            else URL(endpointText.trimEnd('/') + "/chat/completions")
        } catch (_: Throwable) {
            return null to "The endpoint is not a URL"
        }
        val state = try {
            val json = JSONObject(stateJson)
            val compact = JSONObject()
            for (key in listOf(
                "label", "mode", "fallen", "carrying", "powered_off", "care", "life",
                "emotion", "last_sound", "pose", "peers", "tick",
            )) {
                json.opt(key)?.let { compact.put(key, it) }
            }
            compact.put("last_heard", heard)
            compact.toString()
        } catch (_: Throwable) {
            "{}"
        }
        val body = JSONObject()
            .put("model", config.model.ifBlank { "gpt-4o-mini" })
            .put("temperature", 0.4)
            .put(
                "messages",
                JSONArray()
                    .put(
                        JSONObject()
                            .put("role", "system")
                            .put("content", systemPrompt(config.persona, skills))
                    )
                    .put(
                        JSONObject()
                            .put("role", "user")
                            .put("content", "Current duck state: $state")
                    ),
            )
        return try {
            val connection = endpoint.openConnection() as HttpURLConnection
            connection.requestMethod = "POST"
            connection.doOutput = true
            connection.connectTimeout = 8000
            connection.readTimeout = 25000
            connection.setRequestProperty("Content-Type", "application/json")
            if (config.key.isNotEmpty()) {
                connection.setRequestProperty("Authorization", "Bearer ${config.key}")
            }
            connection.outputStream.use { it.write(body.toString().toByteArray()) }
            val code = connection.responseCode
            val text = (if (code in 200..299) connection.inputStream else connection.errorStream)
                ?.bufferedReader()?.use { it.readText() } ?: ""
            connection.disconnect()
            if (code !in 200..299) return null to "HTTP $code"
            val json = JSONObject(text)
            val content = json.optJSONArray("choices")
                ?.optJSONObject(0)
                ?.optJSONObject("message")
                ?.optString("content")
                ?: json.optJSONObject("message")?.optString("content")
                ?: return null to "The model did not answer with a message"
            val decision = parse(content)
            decision to (
                if (decision == null) "The model's answer was not a valid action" else null
                )
        } catch (error: Throwable) {
            null to (error.message ?: "The model did not answer")
        }
    }

    private fun finite(json: JSONObject, key: String, fallback: Double): Double? {
        val value = json.optDouble(key, fallback)
        return value.takeIf { it.isFinite() }
    }

    /** The model is asked for bare JSON; models still like fences and prose. */
    fun parse(text: String): AIDecision? {
        val start = text.indexOf('{')
        val end = text.lastIndexOf('}')
        if (start < 0 || end <= start) return null
        return try {
            val json = JSONObject(text.substring(start, end + 1))
            val action = json.optString("action", "")
            when (action) {
                "skill", "sound" -> {
                    val value = json.optString("value", "")
                    if (value.isEmpty()) null
                    else AIDecision(action, value = value, why = json.optString("why", ""))
                }
                "sit", "stand", "stop" -> AIDecision(action, why = json.optString("why", ""))
                // org.json parses the string "NaN" into Double.NaN, and
                // coerceIn happily returns NaN; only finite numbers may steer.
                "look" -> {
                    val x = finite(json, "x", 1.0) ?: return null
                    val y = finite(json, "y", 0.0) ?: return null
                    val z = finite(json, "z", 0.15) ?: return null
                    AIDecision(
                        action,
                        x = x.coerceIn(-2.0, 2.0),
                        y = y.coerceIn(-2.0, 2.0),
                        z = z.coerceIn(0.0, 1.5),
                        why = json.optString("why", ""),
                    )
                }
                "move" -> {
                    val vx = finite(json, "vx", 0.0) ?: return null
                    val vz = finite(json, "vz", 0.0) ?: return null
                    val seconds = finite(json, "seconds", 1.0) ?: return null
                    AIDecision(
                        action,
                        vx = vx.coerceIn(-0.6, 0.6),
                        vz = vz.coerceIn(-1.0, 1.0),
                        seconds = seconds.coerceIn(0.2, 2.0),
                        why = json.optString("why", ""),
                    )
                }
                else -> null
            }
        } catch (_: Throwable) {
            null
        }
    }
}

/** The generation of the newest decision; an older move chain must stop. */
object AIMotion {
    @Volatile var generation = 0

    fun next(): Int = synchronized(this) {
        generation += 1
        generation
    }
}

/**
 * Run one validated decision through the same intent API a person uses.
 * Returns the line the shell shows. Only the duck's own verbs are possible;
 * a move is bounded and always ends with a zero twist.
 */
fun executeAIDecision(decision: AIDecision): String {
    val shown = if (decision.value.isEmpty()) decision.action else "${decision.action} · ${decision.value}"
    val generation = AIMotion.next()
    if (decision.action != "move") {
        // Cancel any bounded walk before a new verb owns the duck; its last
        // twist otherwise keeps the 500 ms lease alive.
        postDuckCommand(
            "/twist",
            JSONObject().put("vx", 0.0).put("vy", 0.0).put("wz", 0.0).toString(),
        )
    }
    when (decision.action) {
        "skill" -> postDuckCommand(
            "/skill",
            JSONObject().put("name", decision.value).toString(),
        )
        "sound" -> postDuckCommand(
            "/sound",
            JSONObject().put("tag", decision.value).toString(),
        )
        "sit" -> postDuckCommand(
            "/posture",
            JSONObject().put("sit", true).toString(),
        )
        "stand" -> postDuckCommand(
            "/posture",
            JSONObject().put("sit", false).toString(),
        )
        "stop" -> postDuckCommand(
            "/twist",
            JSONObject().put("vx", 0.0).put("vy", 0.0).put("wz", 0.0).toString(),
        )
        "look" -> postDuckCommand(
            "/look",
            JSONObject()
                .put("x", decision.x)
                .put("y", decision.y)
                .put("z", decision.z)
                .toString(),
        )
        "move" -> Thread {
            val frames = maxOf(1, (decision.seconds * 10).toInt())
            val body = JSONObject()
                .put("vx", decision.vx)
                .put("vy", 0.0)
                .put("wz", decision.vz)
                .toString()
            for (frame in 0 until frames) {
                if (AIMotion.generation != generation) return@Thread
                postDuckCommand("/twist", body)
                try {
                    Thread.sleep(100)
                } catch (_: InterruptedException) {
                    break
                }
            }
            if (AIMotion.generation == generation) {
                postDuckCommand(
                    "/twist",
                    JSONObject().put("vx", 0.0).put("vy", 0.0).put("wz", 0.0).toString(),
                )
            }
        }.start()
    }
    return if (decision.why.isEmpty()) "AI · $shown" else "AI · $shown — ${decision.why}"
}
