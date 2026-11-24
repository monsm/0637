package ai.duck.microduck

import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.liveRegion
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
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

/**
 * The three destination pages the IA reorder added (boss ruling 2026-11-16):
 * the drawer keeps four groups x four rows, and the rows that used to cram
 * the duck group and the instrument wall move into these pages. Each page
 * carries at most six controls - the boss's cap - and the grouping lives in
 * contracts/features.yaml, the single source of truth.
 */

@Composable
private fun PageHeader(title: String, closeTag: String, onClose: () -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            title,
            color = Color.White,
            fontWeight = FontWeight.SemiBold,
            fontSize = DuckTheme.Type.Title,
            modifier = Modifier.weight(1f).testTag("$closeTag-title").duckSheetFirstFocus(),
        )
        CloseChip(closeTag, onClose)
    }
}

/** One row: title, optional note, optional state word, 48 dp tall. */
@Composable
private fun ControlRow(
    title: String,
    note: String? = null,
    state: String? = null,
    tag: String,
    onTap: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .background(Color.White.copy(alpha = 0.06f), RoundedCornerShape(DuckTheme.Radius.thumb))
            .clickable { onTap() }
            .padding(horizontal = DuckTheme.Space.md)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, color = Color.White, fontSize = DuckTheme.Type.Body, fontWeight = FontWeight.Medium)
            if (note != null) {
                Text(
                    note,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = DuckTheme.Type.Caption,
                    maxLines = if (duckLargeText()) 4 else 2,
                )
            }
        }
        if (state != null) {
            Text(
                state,
                color = Color.White.copy(alpha = 0.7f),
                fontSize = DuckTheme.Type.Caption,
                fontWeight = FontWeight.SemiBold,
            )
        }
    }
}

/** One A/B load, shared by the row and its stacked large-type form. */
private fun loadBranchB(folder: String, core: DuckCore?, context: Context, scope: CoroutineScope) {
    core?.fork()
    // The reload is seconds of ONNX work and its verdict is the honest answer
    // for a folder the core refuses: the route returns the reason, and the
    // reason is what shows.
    scope.launch {
        val verdict = withContext(Dispatchers.IO) { loadBranchB(folder) }
        android.widget.Toast.makeText(
            context,
            verdict ?: Copy.abLoaded,
            if (verdict == null) android.widget.Toast.LENGTH_SHORT
            else android.widget.Toast.LENGTH_LONG,
        ).show()
    }
}

/**
 * A turn-taking memory game with the duck's own voice: the duck sings a
 * short sequence of its synth sounds (queueSound), the child answers on the
 * same pads in the same order. Each cleared round adds one sound, up to five;
 * a win lets the duck celebrate with its own excited mood. No score, no timer
 * — play, not a test.
 */
private enum class EchoPhase { Idle, Singing, Listening, Won, Lost }

@Composable
private fun EchoGame(core: DuckCore?) {
    val scope = rememberCoroutineScope()
    val hapticView = LocalView.current
    var phase by remember { mutableStateOf(EchoPhase.Idle) }
    var sequence by remember { mutableStateOf(emptyList<String>()) }
    var answer by remember { mutableStateOf(0) }
    val pads = listOf("chirp", "coo", "peck", "inquire")

    fun play(tag: String) {
        core?.queueSound(tag)
    }
    fun sing(seq: List<String>) {
        phase = EchoPhase.Singing
        scope.launch {
            for (tag in seq) {
                play(tag)
                delay(550)
            }
            answer = 0
            phase = EchoPhase.Listening
        }
    }
    fun tap(tag: String) {
        if (phase != EchoPhase.Listening || answer >= sequence.size) return
        play(tag)
        if (tag != sequence[answer]) {
            phase = EchoPhase.Lost
            DuckHaptics.error(hapticView)
            play("alarm")
            return
        }
        answer += 1
        if (answer < sequence.size) return
        if (sequence.size >= 5) {
            phase = EchoPhase.Won
            DuckHaptics.success(hapticView)
            core?.skill("excited")
        } else {
            sequence = sequence + pads.random()
            sing(sequence)
        }
    }
    val status = when (phase) {
        EchoPhase.Idle -> Copy.echoStart
        EchoPhase.Singing -> "…"
        EchoPhase.Listening -> Copy.echoRound(sequence.size)
        EchoPhase.Won -> Copy.echoWon
        EchoPhase.Lost -> Copy.echoAgain
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.echoTitle,
                color = Color.White,
                fontSize = DuckTheme.Type.Label,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.weight(1f))
            Text(
                status,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                // TalkBack says whose turn it is: the status text is a live
                // region, so every phase change is spoken without a focus move.
                modifier = Modifier
                    .testTag("echo-status")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            pads.forEach { tag ->
                PathChip(
                    Copy.echoSound(tag),
                    enabled = phase == EchoPhase.Listening,
                    testTag = "echo-pad-$tag",
                ) { tap(tag) }
            }
        }
        PathChip(
            if (sequence.isEmpty()) Copy.echoStart else Copy.echoAgain,
            testTag = "echo-start",
        ) {
            sequence = listOf(pads.random())
            answer = 0
            sing(sequence)
        }
    }
}

/**
 * Rock-paper-scissors with the duck's body: sit is the rock, stand the
 * paper, a kick the scissors. The duck's answer is mixed from its own
 * provenance fingerprint (the bytes its voice seed comes from) and the
 * round, so the same duck plays the same way on the same round and two
 * ducks differ — a personality, not a coin. Nothing is scored or stored.
 */
private enum class RpsMove(val word: () -> String, val skill: String) {
    Rock({ Copy.rpsRock }, "sit"),
    Paper({ Copy.rpsPaper }, "stand"),
    Scissors({ Copy.rpsScissors }, "kick_left"),
    ;

    /** The move this one beats — entries cannot reference each other in
     * the constructor, so the table lives here. */
    val beats: RpsMove
        get() = when (this) {
            Rock -> Scissors
            Paper -> Rock
            Scissors -> Paper
        }
}

@Composable
private fun RpsGame(core: DuckCore?, fingerprint: () -> String) {
    val hapticView = LocalView.current
    var round by remember { mutableStateOf(0) }
    var outcome by remember { mutableStateOf("") }
    var duckWord by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }

    fun throwMove(mine: RpsMove) {
        if (busy) return
        busy = true
        round += 1
        // FNV-1a over the fingerprint, mixed with the round.
        var hash = -3750763034362895579L // 0xcbf29ce484222325
        val bytes = fingerprint().toByteArray(Charsets.UTF_8)
        for (b in bytes) {
            hash = hash xor b.toLong()
            hash *= 0x100000001b3L
        }
        hash = hash xor (round.toLong() * 2_654_435_761L)
        val theirs = RpsMove.entries[(hash.mod(3)).toInt()]
        duckWord = "${Copy.rpsDuckThrew} ${theirs.word()}"
        core?.skill(theirs.skill)
        outcome = when {
            theirs == mine -> {
                DuckHaptics.selection(hapticView)
                Copy.rpsResult("draw")
            }
            mine.beats == theirs -> {
                DuckHaptics.success(hapticView)
                core?.skill("laugh")
                Copy.rpsResult("win")
            }
            else -> {
                DuckHaptics.medium(hapticView)
                Copy.rpsResult("lose")
            }
        }
        busy = false
    }

    val status = if (outcome.isEmpty()) Copy.echoStart else outcome
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.rpsTitle,
                color = Color.White,
                fontSize = DuckTheme.Type.Label,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.weight(1f))
            Text(
                status,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier
                    .testTag("rps-status")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            RpsMove.entries.forEach { move ->
                PathChip(move.word(), enabled = !busy, testTag = "rps-${move.name.lowercase()}") {
                    throwMove(move)
                }
            }
        }
        if (duckWord.isNotEmpty()) {
            Text(
                duckWord,
                color = DuckTheme.Brass,
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("rps-duck"),
            )
        }
    }
}

/** 玩一玩: stick mode, A/B, push-and-compare, HUD - four controls. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PlayMoreSheet(
    core: DuckCore?,
    input: DuckInput,
    stick: String,
    surfaceIsDirector: Boolean,
    onToggleDirector: () -> Unit,
    onMode: (String) -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var abFolder by remember { mutableStateOf("") }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = CabinetTray,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = DuckTheme.Space.xl)
                .padding(bottom = DuckTheme.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
        ) {
            PageHeader(Copy.playMore, "play-more-close", onClose)
            // Direction A moved the Director switch off the Live screen onto
            // this page; toggling hands the sheet over to the viewfinder.
            ButtonChip(
                Copy.director,
                testTag = "surface-toggle",
            ) {
                onToggleDirector()
                onClose()
            }
            if (duckLargeText()) {
                Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) {
                    listOf("drive", "head", "body").forEach { mode ->
                        ButtonChip(
                            Copy.stick(mode),
                            testTag = "play-more-mode-$mode",
                        ) { onMode(mode) }
                    }
                }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
                    listOf("drive", "head", "body").forEach { mode ->
                        ButtonChip(
                            Copy.stick(mode),
                            testTag = "play-more-mode-$mode",
                        ) { onMode(mode) }
                    }
                }
            }
            if (duckLargeText()) {
                Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) {
                    Text(Copy.abTry, color = Color.White.copy(alpha = 0.75f), fontSize = DuckTheme.Type.Caption)
                    OutlinedTextField(
                        value = abFolder,
                        onValueChange = { abFolder = it.take(64) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth().testTag("play-more-ab-folder"),
                    )
                    ButtonChip(Copy.stateOn, "play-more-ab-load") { loadBranchB(abFolder, core, context, scope) }
                }
            } else {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(Copy.abTry, color = Color.White.copy(alpha = 0.75f), fontSize = DuckTheme.Type.Caption)
                    Column(Modifier.weight(1f).padding(horizontal = DuckTheme.Space.sm)) {
                        OutlinedTextField(
                            value = abFolder,
                            onValueChange = { abFolder = it.take(64) },
                            singleLine = true,
                            modifier = Modifier.testTag("play-more-ab-folder"),
                        )
                    }
                    ButtonChip(Copy.stateOn, "play-more-ab-load") { loadBranchB(abFolder, core, context, scope) }
                }
            }
            if (duckLargeText()) {
                Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) {
                    ButtonChip(Copy.pushCompare, "play-more-push") {
                        core?.fork()
                        postDuckCommand("/push", """{"vx":0.6,"vy":0.0,"vz":0.0}""")
                        android.widget.Toast.makeText(
                            context, Copy.pushCompareDone, android.widget.Toast.LENGTH_SHORT,
                        ).show()
                    }
                    ButtonChip(
                        if (input.hudOn) Copy.stateOn else Copy.hud,
                        "play-more-hud",
                    ) { input.hudOn = !input.hudOn }
                }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
                    ButtonChip(Copy.pushCompare, "play-more-push") {
                        core?.fork()
                        postDuckCommand("/push", """{"vx":0.6,"vy":0.0,"vz":0.0}""")
                        android.widget.Toast.makeText(
                            context, Copy.pushCompareDone, android.widget.Toast.LENGTH_SHORT,
                        ).show()
                    }
                    ButtonChip(
                        if (input.hudOn) Copy.stateOn else Copy.hud,
                        "play-more-hud",
                    ) { input.hudOn = !input.hudOn }
                }
            }
            Text(Copy.abNote, color = Color.White.copy(alpha = 0.55f), fontSize = DuckTheme.Type.Micro)
            if (stick.isNotBlank()) {
                Text(
                    Copy.stick(stick),
                    color = Color.White.copy(alpha = 0.55f),
                    fontSize = DuckTheme.Type.Micro,
                )
            }
            // Play: the duck sings a short sequence of its own voices and the
            // child answers in the same order — inline, no route case.
            EchoGame(core)
            // Play: rock-paper-scissors with the duck's body — its own
            // sit/stand/kick answers, chosen by its own fingerprint.
            RpsGame(core) { core?.fingerprint().orEmpty() ?: "" }
        }
    }
}

/**
 * Load a policy folder on the forked branch, through the core's own /reload
 * route: the verdict comes back with the reason, so a folder the core refuses
 * names why instead of the old silence-then-toast-nothing. Null means loaded.
 * The old JNI reload's int said only "failed"; the route carries the reason
 * with the same verdict, so the sheet shows the core's own words. A lastError
 * JNI binding (native side) would let the direct call do this too - a wiring
 * point left for the ticket that owns jni.rs and DuckCore.kt.
 */
private fun loadBranchB(folder: String): String? {
    return try {
        val connection = java.net.URL("http://127.0.0.1:8765/reload")
            .openConnection() as java.net.HttpURLConnection
        connection.requestMethod = "POST"
        connection.connectTimeout = 2000
        // Loading a policy set is seconds of ONNX work; the route allows it
        // 30 s.
        connection.readTimeout = 35000
        connection.doOutput = true
        connection.setRequestProperty("Content-Type", "application/json")
        val body = org.json.JSONObject(mapOf("dir" to folder)).toString()
        connection.outputStream.use { it.write(body.toByteArray()) }
        val text = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        val json = org.json.JSONObject(text)
        if (json.optBoolean("ok", false)) null
        else json.optString("error", "reload failed")
    } catch (_: Exception) {
        "reload failed"
    }
}

/** 这只鸭子: heart, flashlight, language, compass, collection, teach words
 *  - six, under the cap. Big buttons moved to the tools page together with
 *  iOS (2026-11-19 H2: the panel was at a seven-control reality; the
 *  contract reordered it to six). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DuckPanelSheet(
    flashlightState: Int,
    onFlashlight: () -> Unit,
    onLanguage: () -> Unit,
    onOpen: (DuckRoute) -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val prefs = remember(context) { context.getSharedPreferences("duck", android.content.Context.MODE_PRIVATE) }
    var heartOn by remember { mutableStateOf(prefs.getBoolean("duck_heart_on", false)) }
    var bpm by remember { mutableStateOf(0.0) }
    val heart = remember(context) { Heart(context) }
    LaunchedEffect(heartOn) {
        heart.setEnabled(heartOn)
        prefs.edit().putBoolean("duck_heart_on", heartOn).apply()
    }
    LaunchedEffect(Unit) {
        while (true) {
            fetchCare()?.let {
                bpm = it.heartBpm
                heart.tick(it.heartBpm, System.currentTimeMillis())
            }
            delay(1000)
        }
    }
    var compassOn by remember { mutableStateOf(prefs.getBoolean("duck_compass_on", false)) }
    // The alias page replaces the panel inside the same sheet rather than
    // stacking a second modal: one sheet at a time, and no new route case
    // (the route table is a frozen contract).
    var aliasOpen by remember { mutableStateOf(false) }
    if (aliasOpen) {
        AliasSheet(onClose = { aliasOpen = false })
        return
    }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = CabinetTray,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = DuckTheme.Space.xl)
                .padding(bottom = DuckTheme.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
        ) {
            PageHeader(Copy.thisDuck, "duck-panel-close", onClose)
            ControlRow(
                Copy.heart,
                note = if (heartOn && bpm > 0) "%.0f bpm".format(bpm) else Copy.heartRowNote,
                state = if (heartOn) Copy.stateOn else Copy.stateOff,
                tag = "duck-panel-heart",
            ) { heartOn = !heartOn }
            ControlRow(
                Copy.flashlight,
                note = if (flashlightState > 0) Copy.flashlightOn else Copy.flashlightOff,
                state = if (flashlightState > 0) Copy.stateOn else Copy.stateOff,
                tag = "duck-panel-flashlight",
            ) { onFlashlight() }
            ControlRow(
                Copy.languageRow,
                note = Copy.language.label,
                tag = "duck-panel-language",
            ) { onLanguage() }
            ControlRow(
                Copy.compass,
                note = Copy.compassNote,
                state = if (compassOn) Copy.stateOn else Copy.stateOff,
                tag = "duck-panel-compass",
            ) {
                compassOn = !compassOn
                prefs.edit().putBoolean("duck_compass_on", compassOn).apply()
            }
            // The collection page: every mood and trick with the duck's own
            // lifetime count and first day. It opens the Dex route, which now
            // draws the core's /collection record.
            ControlRow(
                Copy.collectionTitle,
                note = Copy.dexRowNote,
                tag = "collection-open",
            ) { onOpen(DuckRoute.Dex) }
            // Teach it words: a phrase on this phone, an existing official
            // intent on the duck. The table is local-first and replayed.
            ControlRow(
                Copy.aliasTitle,
                note = Copy.aliasLocalOnly,
                tag = "alias-open",
            ) { aliasOpen = true }
        }
    }
}

/** 工具: parent time, room lamp, backup, ToF, big buttons, days - six
 *  controls, under the cap. Days is the newest (2026-11-19 K3): a local
 *  sheet, the same replacement the duck panel's words page uses, so the
 *  pinned route table does not grow a case. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ToolsSheet(
    input: DuckInput,
    depth: DepthScan?,
    onOpen: (DuckRoute) -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var care by remember { mutableStateOf(CareUi()) }
    var tofStrip by remember { mutableStateOf("—") }
    var backupStatus by remember { mutableStateOf<String?>(null) }
    // The days board replaces the tools page inside the same sheet rather
    // than stacking a second modal: one sheet at a time, and no new route
    // case (the route table is a frozen contract) — the alias page pattern.
    var daysOpen by remember { mutableStateOf(false) }
    if (daysOpen) {
        DaysSheet(onClose = { daysOpen = false })
        return
    }
    val backupPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            backupStatus = Copy.backupWorking
            Thread {
                val ok = restoreBackup(context, uri)
                scope.launch { backupStatus = if (ok) Copy.backupDone else Copy.backupFailed }
            }.start()
        }
    }
    LaunchedEffect(Unit) {
        while (true) {
            care = fetchCare() ?: care
            tofStrip = withContext(Dispatchers.IO) { fetchTofStrip() }
            delay(1000)
        }
    }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = CabinetTray,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = DuckTheme.Space.xl)
                .padding(bottom = DuckTheme.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
        ) {
            PageHeader(Copy.tools, "tools-close", onClose)
            ControlRow(
                Copy.parentTime,
                note = care.parentMinutes?.let { minutes ->
                    Copy.parentLeft(minutes, (care.parentLeftS / 60.0).toInt())
                } ?: Copy.parentOff,
                tag = "tools-parent",
            ) {
                val next = when (care.parentMinutes) {
                    null, 0 -> 15
                    15 -> 30
                    30 -> 60
                    else -> 0
                }
                postDuckCommand("/parent", """{"minutes":$next}""")
            }
            ControlRow(
                Copy.roomLamp,
                note = if (input.ambientLux >= 0f) "${input.ambientLux.roundToInt()} lx"
                else "${(input.ambientScale * 100).roundToInt()}%",
                tag = "tools-lamp",
            ) {
                if (input.ambientLux < 0f) {
                    val stops = listOf(0.55f, 0.7f, 0.85f, 1.0f, 1.15f)
                    input.ambientScale =
                        stops[(stops.indexOf(input.ambientScale.coerceIn(0.55f, 1.15f)) + 1) % stops.size]
                }
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .background(Color.White.copy(alpha = 0.06f), RoundedCornerShape(DuckTheme.Radius.thumb))
                    .padding(horizontal = DuckTheme.Space.md),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
            ) {
                Column(Modifier.weight(1f)) {
                    Text(Copy.backup, color = Color.White, fontSize = DuckTheme.Type.Body)
                    Text(
                        backupStatus ?: Copy.backupNote,
                        color = Color.White.copy(alpha = 0.6f),
                        fontSize = DuckTheme.Type.Micro,
                        maxLines = 2,
                    )
                }
                ButtonChip(Copy.backupExport, "tools-backup-export") {
                    backupStatus = Copy.backupWorking
                    exportBackup(context)
                }
                ButtonChip(Copy.backupImport, "tools-backup-import") {
                    backupPicker.launch(arrayOf("application/json", "text/plain"))
                }
            }
            // The depth sensor rides the existing ToF row, the same line iOS shows:
            // the core's grid when it publishes one, then the one-shot scan's
            // honest note and box count, then "no depth sensor" or "denied".
            // No new control, no fabricated room.
            val depthNote = when {
                tofStrip != "—" -> tofStrip
                depth == null -> "—"
                depth.floorDistance != null -> if (
                    depth.boxes.isEmpty() || DepthStatus.obstaclesWritten == false
                ) {
                    // The scan note is still true when staging failed; the
                    // box count would claim furniture the core never loaded.
                    Copy.depthScanNote
                } else {
                    "${Copy.depthScanNote} · ${depth.boxes.size}"
                }
                depth.sensorPresent && depth.reason == DepthScan.PERMISSION ->
                    Copy.denied
                !depth.sensorPresent && depth.reason == DepthScan.NO_SENSOR ->
                    Copy.depthUnsupported
                else -> "—"
            }
            ControlRow(
                Copy.tof,
                note = depthNote,
                tag = "tools-tof",
            ) {
                scope.launch {
                    tofStrip = withContext(Dispatchers.IO) { fetchTofStrip() }
                }
            }
            // Big buttons lives here now: the duck panel was over the
            // six-control cap, and both phones moved the row together
            // (2026-11-19 H2). The tag keeps its duck-panel name so the
            // ledger anchor and the UI tests do not churn.
            ControlRow(
                Copy.bigButtons,
                note = Copy.bigButtonsNote,
                tag = "duck-panel-big",
            ) { onOpen(DuckRoute.Big) }
            // Days, the sixth control: the parent's board of the duck's real
            // days. A local sheet (no route case), the same replacement the
            // duck panel's words page uses on both phones.
            ControlRow(
                Copy.daysTitle,
                tag = "days-open",
            ) { daysOpen = true }
        }
    }
}
