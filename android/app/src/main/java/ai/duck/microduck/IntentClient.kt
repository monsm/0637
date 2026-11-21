package ai.duck.microduck

/**
 * The shell's loopback client (B8, docs/UIUX_REFACTOR_2026-11-15.md §5).
 *
 * These eight calls used to live at the bottom of Shell.kt: one POST helper
 * and seven GET readers, each opening its own 127.0.0.1:8765 connection and
 * parsing the answer inside the UI file. The meeting's rule for the move is
 * literal: **verbatim, zero caching, zero retries, zero semantic change** -
 * the parsing tails are the same expressions, lifted into `parse*` functions
 * only so the JVM tests can feed them fixtures and prove the bytes still
 * mean what they meant before the move.
 *
 * Not moved, by design: PadSender's UDP socket and AIBrain's user-supplied
 * endpoint. The core API is loopback-only; those two are different channels.
 */

/**
 * One command POST on a worker thread: the control that drew it must never
 * block the frame. Both /pet and /feed answer with a refusal when the duck is
 * busy; that refusal has nothing to show on screen.
 */
internal fun postDuckCommand(path: String, body: String = "{}") {
    Thread {
        try {
            val connection = java.net.URL("http://127.0.0.1:8765$path")
                .openConnection() as java.net.HttpURLConnection
            connection.requestMethod = "POST"
            connection.connectTimeout = 2000
            connection.readTimeout = 5000
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", "application/json")
            connection.outputStream.use { it.write(body.toByteArray()) }
            connection.inputStream.readBytes()
            connection.disconnect()
        } catch (_: Exception) {
            // The loop refuses when busy; nothing to show.
        }
    }.start()
}

/**
 * The duck's face, read from GET /state: the status LED's verdict, the
 * four-light battery gauge, the pack percentage and the flashlight's colour.
 * The default is the "no reading yet" face: no verdict, no bars, no percent.
 */
internal data class FaceUi(
    val status: String = "—",
    val bars: Int = 0,
    val percent: String = "—",
    val color: String? = null,
)

/**
 * The face's own numbers, from GET /state: the `leds` object the core
 * publishes once a second. Any failure (server not up yet, no reading) gives
 * the empty face rather than throwing, so the sheet's poll loop never breaks.
 */
internal fun fetchFace(): FaceUi {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseFace(text)
    } catch (_: Throwable) {
        FaceUi()
    }
}

/** The `leds` block of /state as a face reading (fixture-tested). */
internal fun parseFace(text: String): FaceUi {
    // The leds block is flat: no nested braces, so `[^}]*` stops at its
    // own end. Scoping to it keeps the pack's own battery_percent (a
    // different object) out of the reading.
    val leds = Regex(""""leds"\s*:\s*\{[^}]*\}""").find(text)?.value
        ?: return FaceUi()
    val status = Regex(""""status"\s*:\s*"([^"]+)"""")
        .find(leds)?.groupValues?.get(1) ?: "—"
    val bars = Regex(""""bars"\s*:\s*([0-9]+)""")
        .find(leds)?.groupValues?.get(1)?.toIntOrNull() ?: 0
    val raw = Regex(""""battery_percent"\s*:\s*([0-9.]+)""")
        .find(leds)?.groupValues?.get(1)?.toDoubleOrNull()
    val percent = if (raw == null) "—" else "%.0f%%".format(raw)
    val color = Regex(""""color"\s*:\s*"([^"]+)"""")
        .find(leds)?.groupValues?.get(1)
    return FaceUi(status, bars, percent, color)
}

/// The duck's event log, from GET /state -> life.events. Null when the
/// server is not up.
internal fun fetchLifeEvents(): List<String> {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseLifeEvents(text)
    } catch (_: Exception) {
        emptyList()
    }
}

/** life.events of /state as strings (fixture-tested). */
internal fun parseLifeEvents(text: String): List<String> {
    // Parsed as JSON, not scraped with a regular expression and split
    // on commas: a comma inside one event used to split it in two, and
    // the diary counted the halves.
    val events = org.json.JSONObject(text).optJSONObject("life")?.optJSONArray("events")
        ?: return emptyList()
    return (0 until events.length()).mapNotNull { index ->
        if (events.isNull(index)) null else events.optString(index, null)
    }
}

/// The duck's branch count, from GET /branches. Zero when the server is not
/// up.
internal fun fetchBranchCount(): Int {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/branches")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseBranchCount(text)
    } catch (_: Exception) {
        0
    }
}

/** branches[] length of GET /branches (fixture-tested). */
internal fun parseBranchCount(text: String): Int =
    org.json.JSONObject(text).optJSONArray("branches")?.length() ?: 0

/** The trick book, from the same /state the sheet already polls. */
internal fun fetchTrickBook(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseTrickBook(text)
    } catch (_: Throwable) {
        "—"
    }
}

/** "x/13 moods · y tricks" from /state.life (fixture-tested). */
internal fun parseTrickBook(text: String): String {
    // org.json, not a regex: the life object holds nested maps whose
    // braces defeat a flat pattern.
    val life = org.json.JSONObject(text).optJSONObject("life") ?: return "—"
    val emotions = life.optJSONObject("emotions")
    val skills = life.optJSONObject("skills")
    var tricks = 0L
    if (skills != null) {
        val names = skills.keys()
        while (names.hasNext()) tricks += skills.optLong(names.next())
    }
    return "${emotions?.length() ?: 0}/13 moods · $tricks tricks"
}

/** One line from GET /state: the emotion playing, or the idle look-around. */
internal fun fetchBehaviorSummary(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/state")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseBehaviorSummary(text)
    } catch (_: Throwable) {
        "—"
    }
}

/** The behaviour line of /state (fixture-tested). */
internal fun parseBehaviorSummary(text: String): String {
    val emotion = Regex(""""emotion"\s*:\s*\{[^}]*\}""").find(text)?.value
    val name = emotion?.let { Regex(""""name"\s*:\s*"([^"]+)"""").find(it)?.groupValues?.get(1) }
    if (name != null) {
        val elapsed = Regex(""""elapsed_s"\s*:\s*([0-9.]+)""").find(emotion)?.groupValues?.get(1) ?: "0"
        val duration = Regex(""""duration_s"\s*:\s*([0-9.]+)""").find(emotion)?.groupValues?.get(1) ?: "0"
        // The wire name is `yes_fast`; the sheet says what a person calls
        // it. The clock is the instrument's own reading.
        return "${skillLabel(name)} $elapsed/${duration}s"
    }
    val gain = Regex(""""idle"\s*:\s*\{[^}]*\}""").find(text)?.value
        ?.let { Regex(""""takeover"\s*:\s*([0-9.]+)""").find(it)?.groupValues?.get(1)?.toDoubleOrNull() }
    if (gain != null && gain > 0.1) return Copy.lookingAround((gain * 100).toInt())
    return "—"
}

/**
 * The duck's 8x8 head grid as a tiny heat strip: each cell a block whose
 * height follows the official resampler's distance (4 m far, 0 m close).
 * Empty when nothing published depth - the row says so instead of drawing
 * an invented room.
 */
internal fun fetchTofStrip(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/sensors")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseTofStrip(text)
    } catch (_: Throwable) {
        "—"
    }
}

/** The 8-row heat strip from /sensors.tof_grid (fixture-tested). */
internal fun parseTofStrip(text: String): String {
    val array = Regex("\"tof_grid\"\\s*:\\s*\\[([^\\]]*)\\]").find(text)
        ?.groupValues?.get(1)?.split(',')?.mapNotNull { it.trim().toDoubleOrNull() }
        ?: return "—"
    if (array.size != 64) return "—"
    val rows = (0 until 8).map { row ->
        (0 until 8).joinToString("") { col ->
            val value = array[row * 8 + col]
            // Closer is taller. Four blocks of resolution is all a
            // text strip can honestly show.
            when {
                value <= 0.0 -> "·"
                value < 1.0 -> "█"
                value < 2.0 -> "▆"
                value < 3.0 -> "▃"
                else -> "▁"
            }
        }
    }
    return rows.joinToString(" / ")
}

/**
 * The one-line summary the core last published for the microphone, read from
 * GET /sensors. The feed POSTs `{rms, peak_hz}` once a second; this formats
 * them the way the iOS shell does. Any failure (server not up yet, no reading)
 * returns "—" rather than throwing, so the sheet's poll loop never breaks.
 */
internal fun fetchMicSummary(): String {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/sensors")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "GET"
        connection.connectTimeout = 1500
        connection.readTimeout = 1500
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        parseMicSummary(text)
    } catch (_: Throwable) {
        "—"
    }
}

/** "rms x · peak_hz y" from /sensors.microphone (fixture-tested). */
internal fun parseMicSummary(text: String): String {
    val microphone = Regex(""""microphone"\s*:\s*\{[^}]*\}""").find(text)?.value
        ?: return "—"
    val rms = Regex("\"rms\"\\s*:\\s*([0-9.]+)").find(microphone)?.groupValues?.get(1)
        ?: return "—"
    val peak = Regex("\"peak_hz\"\\s*:\\s*([0-9.]+)").find(microphone)?.groupValues?.get(1)
        ?: return "—"
    return "rms $rms · peak_hz $peak"
}
