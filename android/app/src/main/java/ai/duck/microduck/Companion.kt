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
data class RuleUi(val trigger: String, val action: String, val value: String)

/** The program the core is running, read from GET /rules. */
fun fetchRules(): List<RuleUi>? = try {
    val summary = JSONObject(getJson("/rules")).optJSONObject("rules") ?: return null
    val array = summary.optJSONArray("rules") ?: return emptyList()
    (0 until array.length()).mapNotNull { index ->
        val item = array.optJSONObject(index) ?: return@mapNotNull null
        RuleUi(
            trigger = item.optString("trigger", ""),
            action = item.optString("action", ""),
            value = item.optString("value", ""),
        )
    }
} catch (_: Throwable) {
    null
}

/** Replace the core's program and keep the shell's copy for the next launch. */
fun saveRules(context: Context, rules: List<RuleUi>) {
    val array = org.json.JSONArray()
    rules.forEach { rule ->
        array.put(
            JSONObject()
                .put("trigger", rule.trigger)
                .put("action", rule.action)
                .put("value", rule.value)
        )
    }
    val payload = JSONObject().put("rules", array).toString()
    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        .edit().putString(RULES_KEY, payload).apply()
    Thread { try { postJson("/rules", payload) } catch (_: Throwable) {} }.start()
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
fun restoreCompanion(context: Context) {
    val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        .getString(COMPANION_KEY, null)
    Thread {
        // Let the core's launched-effect restore take the queue first: a
        // restore posted in the same tick would be overwritten by the fresh
        // default care this function is about to persist.
        try {
            Thread.sleep(700)
        } catch (_: InterruptedException) {
        }
        if (raw != null) {
            try {
                val saved = JSONObject(raw)
                saved.optJSONObject("care")?.let { postJson("/care", it.toString()) }
                saved.optJSONObject("life")?.let { postJson("/life", it.toString()) }
            } catch (_: Throwable) {
                // A corrupt payload is dropped rather than crashing the launch.
            }
        }
        // The child's block program is configuration, saved separately.
        try {
            val rules = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .getString(RULES_KEY, null)
            if (rules != null) postJson("/rules", rules)
        } catch (_: Throwable) {
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
