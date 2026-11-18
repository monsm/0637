package ai.duck.microduck

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * The words the person taught the duck (G3): a phrase -> wire target table.
 *
 * The core's `POST /config {"voice_aliases": {...}}` is a whole-table
 * replacement and lives in process memory, so the phone keeps its copy in
 * SharedPreferences and replays it once the core is ready — the same shape as
 * WalkPolicyStore. The table never touches the duck's 61-D observation or the
 * safety layer: it only names an existing intent.
 *
 * The key string is the same one iOS's `VoiceAliasStore.defaultsKey` uses, so
 * the pair is greppable; the stores themselves are separate (SharedPreferences
 * and UserDefaults).
 */
internal const val VOICE_ALIASES_KEY = "duck_voice_aliases_v1"

/** The protocol says a phrase is at most 24 Unicode scalars — the core's
 *  `chars` (`speech.rs` `MAX_ALIAS_CHARS`), not UTF-16 units and not
 *  grapheme clusters. */
internal const val ALIAS_MAX_CHARS = 24

/** The official release's thirteen moods, in the wire names the core accepts. */
internal val ALIAS_OFFICIAL_MOODS = listOf(
    "angry", "curious", "defiant", "devastated", "excited", "impatient", "laugh",
    "mmh", "mock", "no", "sad", "yes", "yes_fast",
)

/** The loop's own verbs the ticket puts after the moods and manifest tricks. */
internal val ALIAS_VERBS = listOf("sit", "stand", "ground_pick")

/** One taught row, as the page lists it. */
internal data class VoiceAlias(val phrase: String, val target: String)

/** Trimmed and lowercased, so a phrase that differs only by spaces or by
 *  case is one phrase — the same key the core's speech table keeps. */
internal fun normalizeAliasPhrase(phrase: String): String = phrase.trim().lowercase()

/**
 * The normalized phrase counted in Unicode scalars (`codePointCount`), the
 * unit the core's `chars` uses (`speech.rs` `MAX_ALIAS_CHARS`): one letter
 * is one unit, and an emoji's joiners and modifiers count too — so a family
 * emoji is seven units, however it clusters on screen.
 */
internal fun aliasPhraseLength(phrase: String): Int {
    val normalized = normalizeAliasPhrase(phrase)
    return normalized.codePointCount(0, normalized.length)
}

internal fun aliasPhraseTooLong(phrase: String): Boolean =
    aliasPhraseLength(phrase) > ALIAS_MAX_CHARS

/** A phrase must say something and fit the core's 24-character limit. */
internal fun aliasPhraseValid(phrase: String): Boolean {
    val trimmed = normalizeAliasPhrase(phrase)
    return trimmed.isNotEmpty() && aliasPhraseLength(trimmed) <= ALIAS_MAX_CHARS
}

/** The target must be one of the names this build actually offers. */
internal fun aliasTargetValid(target: String, options: List<String>): Boolean =
    target.isNotBlank() && options.contains(target)

/**
 * The picker: thirteen moods, the manifest's tricks, then sit / stand / pick.
 * `distinct` keeps a name that is both a mood and a trick from appearing twice.
 */
internal fun aliasOptions(emotions: List<String>, skills: List<String>): List<String> =
    (emotions + skills + ALIAS_VERBS).distinct()

internal data class AliasCatalog(
    val emotions: List<String> = ALIAS_OFFICIAL_MOODS,
    val skills: List<String> = emptyList(),
) {
    val targets: List<String> get() = aliasOptions(emotions, skills)
}

/**
 * The catalog from real published data: /collection's skills when it answers
 * (exactly the manifest tricks), and otherwise /skills, which mixes moods,
 * tricks and the loop's verbs and is split here. The official thirteen moods
 * are always offered — the ticket names thirteen, and a build still loading
 * must not shrink the picker; the core remains the judge of every target.
 */
internal fun aliasCatalog(book: CollectionUi?, skills: List<String>): AliasCatalog {
    val published = book?.skills?.map { it.name } ?: skills
    val tricks = published.distinct()
        .filter { it !in ALIAS_OFFICIAL_MOODS && it !in ALIAS_VERBS }
    return AliasCatalog(ALIAS_OFFICIAL_MOODS, tricks)
}

internal fun fetchAliasCatalog(): AliasCatalog {
    val book = fetchCollection()
    return if (book != null) aliasCatalog(book, emptyList()) else aliasCatalog(null, fetchSkillNames())
}

/** Rows in display order; the stored JSON object has no promised order. */
internal fun aliasRows(table: Map<String, String>): List<VoiceAlias> =
    table.entries.map { VoiceAlias(it.key, it.value) }.sortedBy { it.phrase }

/** Add or replace on the folded key: the same phrase points at exactly one
 *  target, so re-teaching "Sit" rewrites "sit" instead of joining the table. */
internal fun aliasWith(table: Map<String, String>, phrase: String, target: String): Map<String, String> =
    table + (normalizeAliasPhrase(phrase) to target.trim())

internal fun aliasWithout(table: Map<String, String>, phrase: String): Map<String, String> =
    table - phrase

/** The preferences copy is the bare table; the wire body wraps it. */
internal fun encodeAliasTable(table: Map<String, String>): String {
    val json = JSONObject()
    table.toSortedMap().forEach { (phrase, target) -> json.put(phrase, target) }
    return json.toString()
}

internal fun aliasConfigBody(table: Map<String, String>): String =
    JSONObject().put("voice_aliases", JSONObject(encodeAliasTable(table))).toString()

/**
 * Reads both the stored bare table and a document that wraps it in
 * `voice_aliases` (the same shape /config takes). Anything that is not a
 * string target, and any phrase that could not have been taught, is dropped
 * rather than rendered or sent back. Every key is folded on the way in, so
 * an older install's mixed-case table reads back as the core holds it.
 */
internal fun parseAliasTable(raw: String?): Map<String, String> {
    if (raw.isNullOrBlank()) return emptyMap()
    return try {
        val root = JSONObject(raw)
        val source = root.optJSONObject("voice_aliases") ?: root
        val table = linkedMapOf<String, String>()
        val keys = source.keys()
        while (keys.hasNext()) {
            val phrase = keys.next()
            val value = source.opt(phrase)
            if (value is String && value.isNotBlank() && aliasPhraseValid(phrase)) {
                table[normalizeAliasPhrase(phrase)] = value.trim()
            }
        }
        table
    } catch (_: Throwable) {
        emptyMap()
    }
}

/**
 * The slice of SharedPreferences the alias store needs. `StringPrefs` is the
 * same interface WalkPolicyStore uses, so the JVM tests can hand either store
 * a map-backed implementation.
 */
internal object VoiceAliasStore {
    fun prefs(context: Context): StringPrefs = AliasStringPrefs(context)

    fun load(prefs: StringPrefs): Map<String, String> =
        parseAliasTable(prefs.get(VOICE_ALIASES_KEY))

    /** An empty table forgets the key instead of storing "{}": nothing to replay. */
    fun save(prefs: StringPrefs, table: Map<String, String>) {
        prefs.put(VOICE_ALIASES_KEY, if (table.isEmpty()) null else encodeAliasTable(table))
    }
}

private class AliasStringPrefs(context: Context) : StringPrefs {
    private val prefs = context.getSharedPreferences("duck", Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)

    override fun put(key: String, value: String?) {
        val edit = prefs.edit()
        if (value == null) edit.remove(key) else edit.putString(key, value)
        edit.apply()
    }
}

/** The core's answer to one whole-table save. `reason` is the core's own words. */
internal data class AliasSaveResult(
    val accepted: Boolean,
    val reason: String? = null,
)

/** The one edge the JVM tests substitute: a whole-table save against the core. */
internal interface AliasCoreClient {
    fun save(table: Map<String, String>): AliasSaveResult
}

/**
 * `POST /config {"voice_aliases": {...}}`. A refusal is read from the body
 * (`{"ok":false,"error":"..."}`) and shown verbatim; only "nothing answered"
 * becomes a null reason, which the sheet renders as its own offline words.
 * The core may not have the route yet (a parallel ticket owns it): the 404's
 * "not found" is then the honest answer, never a fake success.
 */
internal object HttpAliasClient : AliasCoreClient {
    override fun save(table: Map<String, String>): AliasSaveResult {
        return try {
            val connection = java.net.URL("http://127.0.0.1:8765/config")
                .openConnection() as java.net.HttpURLConnection
            connection.requestMethod = "POST"
            connection.connectTimeout = 1500
            connection.readTimeout = 2500
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", "application/json")
            connection.outputStream.use { it.write(aliasConfigBody(table).toByteArray()) }
            val code = connection.responseCode
            val text = (if (code in 200..299) connection.inputStream else connection.errorStream)
                ?.bufferedReader()?.use { it.readText() } ?: ""
            connection.disconnect()
            val json = try {
                JSONObject(text)
            } catch (_: Throwable) {
                null
            }
            val refused = code !in 200..299 || json?.optBoolean("ok", true) == false
            if (!refused) {
                AliasSaveResult(true)
            } else {
                val reason = json?.optString("error", "").orEmpty().ifBlank {
                    text.ifBlank { "HTTP $code" }
                }
                AliasSaveResult(false, reason)
            }
        } catch (_: Throwable) {
            AliasSaveResult(false, null)
        }
    }
}

/**
 * Hand a fresh core the table this phone already holds, once, the way
 * WalkPolicyStore replays the walk slot. Waits until /state is published and
 * gives up after about ten seconds rather than pretending; a refusal is
 * final for this launch (the core validates targets and will keep refusing),
 * so it is not retried. Nothing stored means nothing to send.
 *
 * `client`, `ready` and `sleep` are injectable so the JVM test can prove the
 * order without a core or a clock.
 */
internal fun replayVoiceAliases(
    prefs: StringPrefs,
    client: AliasCoreClient = HttpAliasClient,
    ready: () -> Boolean = { fetchRawState() != null },
    sleep: (Long) -> Unit = { Thread.sleep(it) },
): Boolean {
    val table = VoiceAliasStore.load(prefs)
    if (table.isEmpty()) return false
    for (attempt in 0 until 40) {
        if (!ready()) {
            sleep(250)
            continue
        }
        return client.save(table).accepted
    }
    return false
}

/**
 * 教它说话: what the person says on the left, what the duck does on the
 * right. The list is the draft; Save posts the whole table to the core, and
 * only a table the core accepted is remembered on this phone for the next
 * launch's replay. A refusal shows the core's own reason. The target picker
 * is built from /collection, falling back to /skills.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AliasSheet(onClose: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val prefs = remember(context) { VoiceAliasStore.prefs(context) }
    var table by remember { mutableStateOf(VoiceAliasStore.load(prefs)) }
    var catalog by remember { mutableStateOf(AliasCatalog()) }
    var phrase by remember { mutableStateOf("") }
    // A friendly default: the first thing a person teaches is usually a verb.
    var target by remember { mutableStateOf("sit") }
    var menuOpen by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        val fetched = withContext(Dispatchers.IO) { fetchAliasCatalog() }
        catalog = fetched
        if (!fetched.targets.contains(target)) {
            target = fetched.targets.firstOrNull().orEmpty()
        }
    }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = ShellSurface,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 28.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.aliasTitle,
                    color = Color.White,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("alias-title"),
                )
                CloseChip("alias-close", onClose)
            }
            Text(
                Copy.aliasLocalOnly,
                color = Color.White.copy(alpha = 0.6f),
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.testTag("alias-local"),
            )
            OutlinedTextField(
                value = phrase,
                onValueChange = {
                    phrase = it
                    error = null
                },
                singleLine = true,
                label = { Text(Copy.aliasHint) },
                isError = aliasPhraseTooLong(phrase),
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("alias-phrase"),
            )
            if (aliasPhraseTooLong(phrase)) {
                Text(
                    Copy.aliasTooLong,
                    color = DuckTheme.StateWarn,
                    style = MaterialTheme.typography.labelSmall,
                    modifier = Modifier.testTag("alias-too-long"),
                )
            }
            Text(
                Copy.aliasDoes,
                color = Color.White.copy(alpha = 0.6f),
                style = MaterialTheme.typography.labelSmall,
            )
            Box {
                ButtonChip(skillLabel(target.ifBlank { "—" }), "alias-target") {
                    menuOpen = true
                }
                DropdownMenu(
                    expanded = menuOpen,
                    onDismissRequest = { menuOpen = false },
                    modifier = Modifier.background(DuckTheme.BgSurface),
                ) {
                    catalog.targets.forEach { name ->
                        DropdownMenuItem(
                            text = { Text(skillLabel(name), color = Color.White) },
                            onClick = {
                                target = name
                                menuOpen = false
                            },
                            modifier = Modifier.testTag("alias-target-$name"),
                        )
                    }
                }
            }
            ButtonChip(Copy.aliasAdd, "alias-add") {
                val normalized = normalizeAliasPhrase(phrase)
                when {
                    !aliasPhraseValid(normalized) -> {
                        error = if (aliasPhraseTooLong(phrase)) Copy.aliasTooLong else null
                    }
                    !aliasTargetValid(target, catalog.targets) -> error = null
                    else -> {
                        // The draft is the page's; the phone's stored copy is
                        // written only after the core accepts the save, the
                        // same rule WalkPolicyStore follows.
                        table = aliasWith(table, normalized, target)
                        phrase = ""
                        error = null
                    }
                }
            }
            if (table.isEmpty()) {
                Text(
                    Copy.aliasEmpty,
                    color = Color.White.copy(alpha = 0.55f),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.testTag("alias-empty"),
                )
            } else {
                aliasRows(table).forEach { row ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .padding(horizontal = 4.dp)
                            .testTag("alias-row-${row.phrase}"),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                row.phrase,
                                color = Color.White,
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Text(
                                "→ ${skillLabel(row.target)}",
                                color = Color.White.copy(alpha = 0.6f),
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }
                        ButtonChip(Copy.deleteRule, "alias-delete-${row.phrase}") {
                            table = aliasWithout(table, row.phrase)
                            error = null
                        }
                    }
                }
            }
            error?.let { reason ->
                Text(
                    reason,
                    color = DuckTheme.StateBad,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.testTag("alias-error"),
                )
            }
            ButtonChip(if (saving) Copy.aiThinking else Copy.aiSave, "alias-save") {
                if (!saving) {
                    saving = true
                    scope.launch {
                        val result = withContext(Dispatchers.IO) { HttpAliasClient.save(table) }
                        saving = false
                        if (result.accepted) {
                            // The core took the whole table; this phone keeps
                            // it for the next launch's replay.
                            VoiceAliasStore.save(prefs, table)
                            error = null
                        } else {
                            error = result.reason ?: Copy.aiOffline
                        }
                    }
                }
            }
        }
    }
}
