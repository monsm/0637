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
private const val RULES_KEY = "duck_rules_v1"

/** One block: when `trigger`, do `action`=`value`. */
data class RuleUi(
    val trigger: String,
    val action: String,
    val value: String,
    val then: List<RuleStep> = emptyList(),
)

data class RuleStep(val action: String, val value: String)

/**
 * The core writes a fall into the life record as "fell" — one word, not a
 * prefix. The diary counted the prefix "fall", which matched nothing, so the
 * fall count was always zero, and a zero in the facts is a claim about the
 * duck that is not true.
 */
fun isFall(event: String): Boolean = event == "fell" || event.startsWith("fall")

/**
 * Trim a value the way the core trims it.
 *
 * The core uses Rust's `trim()`, which knows the whole Unicode White_Space
 * set — the no-break space included. Kotlin's `trim()` does not, so a value
 * the phone thought was fine could be refused by the core, and the core
 * refuses the whole program when one value is empty.
 */
internal fun trimLikeCore(value: String): String =
    // Java's isWhitespace() covers every Unicode space except the
    // non-breaking ones; Rust's trim() also takes U+00A0 and U+202F, so
    // they are named here rather than pulled from an API this toolchain
    // does not resolve.
    value.trim { it.isWhitespace() || it == '\u00A0' || it == '\u202F' }

/** The program the core is running, read from GET /rules. */
fun fetchRules(): List<RuleUi>? = try {
    val summary = JSONObject(getJson("/rules")).optJSONObject("rules") ?: return null
    val array = summary.optJSONArray("rules") ?: return emptyList()
    (0 until array.length()).mapNotNull { index ->
        val item = array.optJSONObject(index) ?: return@mapNotNull null
        val chain = item.optJSONArray("then")
        RuleUi(
            trigger = item.optString("trigger", ""),
            action = item.optString("action", ""),
            value = item.optString("value", ""),
            then = if (chain == null) emptyList() else (0 until chain.length()).mapNotNull { at ->
                val step = chain.optJSONObject(at) ?: return@mapNotNull null
                RuleStep(step.optString("action", ""), step.optString("value", ""))
            },
        )
    }
} catch (_: Throwable) {
    null
}

/**
 * Replace the core's program and keep the shell's copy for the next launch.
 *
 * The core is the authority, and the copy on disk is only updated when the
 * core took the program. Keeping a copy it refused would restore a program
 * the duck is not running — and the editor would keep showing it, so the
 * child would see blocks that do nothing.
 *
 * [onResult] runs on the main thread with the core's answer.
 */
fun saveRules(context: Context, rules: List<RuleUi>, onResult: (Boolean) -> Unit = {}) {
    val array = org.json.JSONArray()
    rules.forEach { rule -> array.put(ruleJson(rule)) }
    val payload = JSONObject().put("rules", array).toString()
    Thread {
        val accepted = try { postJson("/rules", payload) } catch (_: Throwable) { false }
        if (accepted) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit().putString(RULES_KEY, payload).apply()
        }
        android.os.Handler(android.os.Looper.getMainLooper()).post { onResult(accepted) }
    }.start()
}

internal val RULE_TRIGGERS = setOf("clap", "whistle", "pet")
internal val RULE_ACTIONS = setOf("skill", "emotion", "sound")
internal const val RULE_MAX_STEPS = 4
internal const val RULE_MAX_RULES = 8

/** One rule as the core's wire object; save and share both use it. */
internal fun ruleJson(rule: RuleUi): JSONObject {
    val item = JSONObject()
        .put("trigger", rule.trigger)
        .put("action", rule.action)
        .put("value", rule.value)
    if (rule.then.isNotEmpty()) {
        val chain = org.json.JSONArray()
        rule.then.forEach { step ->
            chain.put(JSONObject().put("action", step.action).put("value", step.value))
        }
        item.put("then", chain)
    }
    return item
}

/**
 * The share document: a block program travels between phones as text (share
 * sheet, message, clipboard). The version tag lets a future format change
 * without guessing; the rules themselves are the exact wire shape the core
 * accepts, so an imported program runs unchanged.
 */
internal fun rulesShareText(rules: List<RuleUi>): String {
    val array = org.json.JSONArray()
    rules.forEach { rule -> array.put(ruleJson(rule)) }
    return JSONObject()
        .put("microduck-rules", 1)
        .put("rules", array)
        .toString()
}

/**
 * Read a shared program from pasted text: either the versioned document or a
 * bare rules array. Every rule is validated against the same vocabulary the
 * editor offers; a malformed entry is dropped rather than raining junk on the
 * core, and `null` means the text was not a program at all.
 */
internal fun parseSharedRules(text: String): List<RuleUi>? = try {
    val trimmed = text.trim()
    val array = if (trimmed.startsWith("[")) {
        org.json.JSONArray(trimmed)
    } else {
        val doc = JSONObject(trimmed)
        if (doc.optInt("microduck-rules", 0) != 1) return null
        doc.optJSONArray("rules") ?: return null
    }
    val rules = (0 until array.length()).mapNotNull { index ->
        val item = array.optJSONObject(index) ?: return@mapNotNull null
        val trigger = item.optString("trigger", "")
        val action = item.optString("action", "")
        val value = trimLikeCore(item.optString("value", ""))
        if (trigger !in RULE_TRIGGERS || action !in RULE_ACTIONS || value.isEmpty()) {
            return@mapNotNull null
        }
        val chain = item.optJSONArray("then")
        val then = if (chain == null) {
            emptyList()
        } else {
            (0 until minOf(chain.length(), RULE_MAX_STEPS)).mapNotNull { at ->
                val step = chain.optJSONObject(at) ?: return@mapNotNull null
                val stepAction = step.optString("action", "")
                val stepValue = trimLikeCore(step.optString("value", ""))
                if (stepAction !in RULE_ACTIONS || stepValue.isEmpty()) {
                    null
                } else {
                    RuleStep(stepAction, stepValue)
                }
            }
        }
        RuleUi(trigger, action, value, then)
    }
    rules.ifEmpty { null }
} catch (_: Throwable) {
    null
}

/**
 * Merge an imported program into the child's own: an identical rule (the
 * same trigger, action and value) is not duplicated, the eight-block budget
 * is kept, and everything the child already had survives.
 */
internal fun mergeRules(current: List<RuleUi>, imported: List<RuleUi>): List<RuleUi> {
    val merged = current.toMutableList()
    for (rule in imported) {
        if (merged.size >= RULE_MAX_RULES) break
        val exists = merged.any {
            it.trigger == rule.trigger && it.action == rule.action && it.value == rule.value
        }
        if (!exists) merged.add(rule)
    }
    return merged
}

/** The three lifetime numbers the vet book is about. */
data class LifeUi(
    val metres: Double = 0.0,
    val rolls: Int = 0,
    val pickups: Int = 0,
)

fun fetchLifeRecord(): LifeUi? = try {
    val life = JSONObject(getJson("/state")).optJSONObject("life") ?: return null
    LifeUi(
        metres = life.optDouble("metres", 0.0),
        rolls = life.optJSONObject("skills")?.optInt("roulade", 0) ?: 0,
        pickups = life.optInt("pickups", 0),
    )
} catch (_: Throwable) {
    null
}

/**
 * Write the core's Markdown export to the cache and hand it to the system
 * chooser. The core writes the document; the app only owns the file and the
 * per-URI read grant. Runs off the main thread until the chooser is launched.
 */
fun shareVetbook(context: Context) {
    Thread {
        try {
            val markdown = getJson("/export/vetbook.md")
            if (markdown.isBlank()) return@Thread
            val dir = java.io.File(context.cacheDir, "exports").apply { mkdirs() }
            val file = java.io.File(dir, "MicroDuck-vetbook.md")
            file.writeText(markdown)
            val uri = androidx.core.content.FileProvider.getUriForFile(
                context,
                "${context.packageName}.fileprovider",
                file,
            )
            val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                type = "text/markdown"
                putExtra(android.content.Intent.EXTRA_STREAM, uri)
                putExtra(android.content.Intent.EXTRA_SUBJECT, "MicroDuck · vet book")
                addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            val chooser = android.content.Intent.createChooser(send, Copy.shareVetbook)
                .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(chooser)
        } catch (_: Throwable) {
            // No share target, or the core was down: the sheet stays as it
            // was rather than crashing on a transient failure.
        }
    }.start()
}

fun fetchCare(): CareUi? = fetchRawState()?.let(::parseCare)

fun parseCare(text: String): CareUi? = try {
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
 * The chosen night, from the top-level `night` object of GET /state.
 * `quiet_s` grows while the night runs; an inactive night publishes zeros,
 * so a shell that wants the length must remember the last active reading.
 */
data class NightUi(
    val active: Boolean = false,
    val quietSeconds: Double = 0.0,
    val startedTick: Int = 0,
)

/** The room's chorus, as /state publishes it every tick. */
data class ChorusUi(
    val active: Boolean = false,
    val role: String = "",
    val peers: Int = 0,
    val startedTick: Int = 0,
)

fun parseChorus(text: String): ChorusUi? = try {
    val c = JSONObject(text).optJSONObject("chorus") ?: return null
    ChorusUi(
        active = c.optBoolean("active", false),
        role = c.optString("role", ""),
        peers = c.optInt("peers", 0),
        startedTick = c.optInt("started_tick", 0),
    )
} catch (_: Throwable) {
    null
}

fun fetchNight(): NightUi? = fetchRawState()?.let(::parseNight)

fun parseNight(text: String): NightUi? = try {
    val night = JSONObject(text).optJSONObject("night") ?: return null
    NightUi(
        active = night.optBoolean("active", false),
        quietSeconds = night.optDouble("quiet_s", 0.0),
        startedTick = night.optInt("started_tick", 0),
    )
} catch (_: Throwable) {
    null
}

/** One other duck on the LAN. */
data class PeerUi(
    val id: String,
    val name: String,
    val replied: Boolean,
)

data class PeersUi(
    val name: String = "",
    val peers: List<PeerUi> = emptyList(),
)

fun fetchPeers(): PeersUi? = fetchRawState()?.let(::parsePeers)

fun parsePeers(text: String): PeersUi? = try {
    val peers = JSONObject(text).optJSONObject("peers") ?: return null
    val array = peers.optJSONArray("peers") ?: org.json.JSONArray()
    PeersUi(
        name = peers.optString("name", ""),
        peers = (0 until array.length()).mapNotNull { index ->
            val item = array.optJSONObject(index) ?: return@mapNotNull null
            PeerUi(
                id = item.optString("id", ""),
                name = item.optString("name", ""),
                replied = item.optBoolean("replied", false),
            )
        },
    )
} catch (_: Throwable) {
    null
}

private var multicastLock: android.net.wifi.WifiManager.MulticastLock? = null

/**
 * Open the core's LAN socket and announce this phone. Also takes Android's
 * multicast lock, without which many Wi-Fi stacks drop the peer broadcasts.
 * Called when the user opens Neighbours, never at launch.
 */
fun startPeers(context: Context, name: String) {
    try {
        if (multicastLock == null) {
            val wifi = context.applicationContext
                .getSystemService(Context.WIFI_SERVICE) as? android.net.wifi.WifiManager
            multicastLock = wifi?.createMulticastLock("duck-peers")?.apply {
                setReferenceCounted(false)
                acquire()
            }
        }
    } catch (_: Throwable) {
    }
    Thread {
        try {
            postJson("/peers/start", JSONObject().put("name", name).toString())
        } catch (_: Throwable) {
        }
    }.start()
}

fun greetPeer(context: Context, id: String?) {
    val payload = if (id == null) JSONObject() else JSONObject().put("id", id)
    Thread {
        try {
            postJson("/peers/greet", payload.toString())
        } catch (_: Throwable) {
        }
    }.start()
}

/** The last hello exchanged, formatted for the pill. */
data class PeerEventUi(val tick: Int, val text: String)

fun fetchPeerEvent(): PeerEventUi? = fetchRawState()?.let(::parsePeerEvent)

fun parsePeerEvent(text: String): PeerEventUi? = try {
    val event = JSONObject(text).optJSONObject("peer_event") ?: return null
    val kind = event.optString("kind", "")
    val name = event.optString("name", "Duck")
    val text2 = if (kind == "greeted") Copy.peerGreeted(name) else Copy.peerReplied(name)
    PeerEventUi(event.optInt("tick", -1), text2)
} catch (_: Throwable) {
    null
}

/** The last gift a neighbour sent, formatted for the pill. */
data class GiftEventUi(val tick: Int, val text: String)

fun parseGiftEvent(text: String): GiftEventUi? = try {
    val event = JSONObject(text).optJSONObject("gift_event") ?: return null
    val kind = event.optString("kind", "")
    val name = event.optString("from_name", "Duck")
    // Only the two real gifts raise a pill: an unknown kind is not invented.
    val line = when (kind) {
        "treat" -> Copy.giftReceivedTreat(name)
        "emotion" -> Copy.giftReceivedEmotion(name)
        else -> return null
    }
    GiftEventUi(event.optInt("tick", -1), line)
} catch (_: Throwable) {
    null
}

/** The last room sound the duck answered: kind and the tick it happened. */
data class SoundEvent(val kind: String, val tick: Int)

fun fetchSoundEvent(): SoundEvent? = fetchRawState()?.let(::parseSoundEvent)

fun parseSoundEvent(text: String): SoundEvent? = try {
    val event = JSONObject(text).optJSONObject("sound_event") ?: return null
    SoundEvent(event.optString("kind", ""), event.optInt("tick", -1))
} catch (_: Throwable) {
    null
}

/** The last programmed block that fired, formatted for the pill. */
data class RuleEvent(val tick: Int, val text: String)

fun fetchRuleEvent(): RuleEvent? = fetchRawState()?.let(::parseRuleEvent)

fun parseRuleEvent(text: String): RuleEvent? = try {
    val event = JSONObject(text).optJSONObject("rule_event") ?: return null
    val trigger = event.optString("trigger", "")
    val action = event.optString("action", "")
    val rawValue = event.optString("value", "")
    val shown = if (action == "sound") rawValue else skillLabel(rawValue)
    RuleEvent(
        event.optInt("tick", -1),
        "${Copy.ruleTrigger(trigger)} → $shown",
    )
} catch (_: Throwable) {
    null
}

/** The whole /state document, for the user's model. */
fun fetchRawState(): String? = try {
    getJson("/state")
} catch (_: Throwable) {
    null
}

fun fetchSkillNames(): List<String> = try {
    val array = JSONObject(getJson("/skills")).optJSONArray("skills")
        ?: return emptyList()
    (0 until array.length()).mapNotNull { array.optString(it, "").ifEmpty { null } }
} catch (_: Throwable) {
    emptyList()
}

fun fetchLastHeard(): String? = try {
    JSONObject(getJson("/state")).optJSONObject("last_heard")?.optString("text", "")
} catch (_: Throwable) {
    null
}

/**
 * Save the duck's inner life before Android can kill the process. The whole
 * /state is not kept - only the two objects the core can take back.
 */
fun saveCompanion(context: Context) {
    Thread { saveCompanionNow(context) }.start()
}

/**
 * The write itself. Runs on a caller-chosen thread; onPause waits for it, and
 * the shell also saves every ten seconds so a process killed without a
 * lifecycle callback keeps the duck's tummy.
 */
fun saveCompanionNow(context: Context) {
    val state = fetchRawState() ?: return
    saveCompanionFromState(context, state)
}

/** Persist the exact /state snapshot the care loop just read. */
fun saveCompanionFromState(context: Context, stateText: String) {
    try {
        val state = JSONObject(stateText)
        val saved = JSONObject()
        state.optJSONObject("care")?.let { saved.put("care", it) }
        state.optJSONObject("life")?.let { saved.put("life", it) }
        if (saved.length() > 0) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit().putString(COMPANION_KEY, saved.toString()).commit()
            android.util.Log.i(
                "duck-care",
                "saved care=" + (saved.optJSONObject("care")?.toString() ?: "—"),
            )
        }
    } catch (_: Throwable) {
        // A malformed snapshot is skipped; the next tick tries again.
    }
}

/**
 * Hand the saved numbers back once the core is running. Both routes are
 * restore-only: the core clamps them, and the next /state poll shows what the
 * duck actually accepted.
 */
fun restoreCompanion(context: Context): Boolean {
    val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    val raw = prefs.getString(COMPANION_KEY, null)
    val rules = prefs.getString(RULES_KEY, null)
    android.util.Log.i(
        "duck-care",
        "restore start key=${if (raw != null) "present" else "absent"} rules=${rules != null}",
    )
    if (raw == null && rules == null) return true
    // The intent server starts with the core and may not accept connections
    // yet. Retry until it acknowledges, and keep the save gate closed until
    // then: a fresh duck must not overwrite the saved one.
    val saved = try {
        if (raw != null) JSONObject(raw) else null
    } catch (_: Throwable) {
        null
    }
    val care = saved?.optJSONObject("care")
    val life = saved?.optJSONObject("life")
    val wantedBond = care?.optDouble("bond", -1.0) ?: -1.0
    // Wait until /state is published (the loop is alive), then post and
    // verify the bond: /care is fire-and-forget, so a 2xx only means the
    // HTTP server saw it, not that the tick dequeued it.
    for (attempt in 0 until 40) {
        try {
            if (fetchRawState() == null) throw IllegalStateException("state not ready")
            if (care != null && !postJson("/care", care.toString())) {
                throw IllegalStateException("care not accepted")
            }
            if (life != null && !postJson("/life", life.toString())) {
                throw IllegalStateException("life not accepted")
            }
            if (rules != null && !postJson("/rules", rules)) {
                throw IllegalStateException("rules not accepted")
            }
            if (wantedBond >= 0.0) {
                var seen = false
                for (check in 0 until 8) {
                    Thread.sleep(100)
                    val state = JSONObject(fetchRawState() ?: "{}")
                    val bond = state.optJSONObject("care")?.optDouble("bond", -1.0) ?: -1.0
                    if (bond >= 0.0 && kotlin.math.abs(bond - wantedBond) < 0.02) {
                        seen = true
                        break
                    }
                }
                if (!seen) throw IllegalStateException("care restore not published")
            }
            android.util.Log.i(
                "duck-care",
                "restore ok after attempt $attempt bond=${fetchRawState()?.let { s ->
                    try { JSONObject(s).optJSONObject("care")?.optDouble("bond", -1.0) } catch (_: Throwable) { -1.0 }
                }}",
            )
            return true
        } catch (_: Throwable) {
            try {
                Thread.sleep(250)
            } catch (_: InterruptedException) {
            }
        }
    }
    android.util.Log.w("duck-care", "restore FAILED after retries, wanted bond=$wantedBond")
    return false
}

internal fun getJson(path: String): String {
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

internal fun postJson(path: String, body: String): Boolean {
    val connection = URL("http://127.0.0.1:8765$path").openConnection() as HttpURLConnection
    connection.requestMethod = "POST"
    connection.doOutput = true
    connection.setRequestProperty("Content-Type", "application/json")
    connection.connectTimeout = 2000
    connection.readTimeout = 2000
    return try {
        connection.outputStream.use { it.write(body.toByteArray()) }
        val code = connection.responseCode
        val text = (if (code in 200..299) connection.inputStream else connection.errorStream)
            ?.bufferedReader()?.use { it.readText() } ?: ""
        // Refusals travel as HTTP 200 with {"ok":false}; only a body that
        // did not refuse counts as delivered.
        code in 200..299 && !text.contains("\"ok\":false")
    } finally {
        connection.disconnect()
    }
}
