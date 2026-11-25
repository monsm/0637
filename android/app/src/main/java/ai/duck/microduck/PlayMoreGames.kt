package ai.duck.microduck

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Text
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.Random

/**
 * The five small games (2026-11-25), inline on Play more like Echo and RPS.
 *
 * Each one only asks the duck for verbs it actually has and reads only the
 * fields /state actually publishes: the kicks, the 13 official moods, the
 * today-vs-day_started deltas, and the room-sound classifier's events. No
 * game tallies a score the duck did not earn — the counters are the vet
 * book's own.
 */

/** The 13 official robotd moods the core loads. */
internal val officialMoods = listOf(
    "angry", "curious", "defiant", "devastated", "excited", "impatient",
    "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
)

/** Each mood's official timeline length, from upstream's keyframes. */
internal fun moodDurationMs(name: String): Long = when (name) {
    "angry" -> 2500
    "curious" -> 2900
    "defiant" -> 2100
    "devastated" -> 8500
    "excited" -> 3300
    "impatient" -> 2500
    "laugh" -> 2900
    "mmh" -> 1900
    "mock" -> 2500
    "no" -> 1700
    "sad" -> 7500
    "yes" -> 1400
    "yes_fast" -> 1400
    else -> 2500
}

private fun moodSeed(fingerprint: String, round: Int): Long {
    var hash = -3750763034362895579L // 0xcbf29ce484222325
    for (byte in fingerprint.toByteArray(Charsets.UTF_8)) {
        hash = hash xor byte.toLong()
        hash *= 0x100000001b3L
    }
    return hash xor (round.toLong() * 2_654_435_761L)
}

private fun playingEmotion(state: String?): Boolean = try {
    state != null && JSONObject(state).optJSONObject("emotion") != null
} catch (_: Throwable) {
    false
}

/** 左右开弓: alternate feet on the count; each correct tap is a real kick. */
@Composable
fun KickRally(core: DuckCore?) {
    var step by remember { mutableStateOf(0) }
    var notice by remember { mutableStateOf("") }
    val leftTurn = step % 2 == 0
    val status = when {
        step >= 6 -> Copy.rallyDone
        else -> Copy.rallyStep(step + 1) + " · " +
            if (leftTurn) Copy.rallyLeftTurn else Copy.rallyRightTurn
    }
    fun tap(left: Boolean) {
        if (left != leftTurn || step >= 6) {
            notice = Copy.rallyWrong
            return
        }
        val started = core?.skill(if (left) "kick_left" else "kick_right") ?: 0
        if (started == 0) {
            notice = Copy.rallyWrong
            return
        }
        notice = ""
        step += 1
        if (step >= 6) core?.skill("excited")
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.rallyTitle,
                color = Color.White,
                fontSize = DuckTheme.Type.Label,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.weight(1f))
            Text(
                if (notice.isNotEmpty()) notice else status,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier
                    .testTag("rally-status")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(Copy.rallyLeftTurn, enabled = step < 6, testTag = "rally-left") { tap(true) }
            PathChip(Copy.rallyRightTurn, enabled = step < 6, testTag = "rally-right") { tap(false) }
        }
        PathChip(Copy.rallyGo, testTag = "rally-start") { step = 0; notice = "" }
    }
}

/** 情绪哑剧: the duck performs a real mood; the child names it. */
@Composable
fun MoodCharade(core: DuckCore?) {
    var answer by remember { mutableStateOf("") }
    var choices by remember { mutableStateOf(emptyList<String>()) }
    var phase by remember { mutableStateOf(0) } // 0 idle, 1 performing, 2 guessing, 3 answered
    var outcome by remember { mutableStateOf("") }
    var round by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()
    val fingerprint = core?.fingerprint().orEmpty()

    fun start() {
        round += 1
        val seed = moodSeed(fingerprint, round)
        val index = Math.floorMod(seed, officialMoods.size.toLong()).toInt()
        answer = officialMoods[index]
        val others = officialMoods.filter { it != answer }
        val pick = others.shuffled(Random(seed)).take(3)
        choices = (pick + answer).shuffled(Random(seed xor 0x5DEECE66DL))
        outcome = ""
        if (core?.skill(answer) ?: 0 == 0) {
            phase = 0
            outcome = Copy.charadesWrong
            return
        }
        phase = 1
        // The guess opens when the mood's own timeline ends. The published
        // durations are the honest clock; polling /state would let a slow
        // fetch leave the chips disabled after the smoke already saw idle.
        scope.launch {
            delay(moodDurationMs(answer) + 300L)
            phase = 2
        }
    }
    fun guess(mood: String) {
        if (phase != 2) return
        val right = mood == answer
        core?.skill(if (right) "yes" else "no")
        outcome = if (right) Copy.charadesRight else Copy.charadesWrong
        phase = 3
    }
    val status = when (phase) {
        1 -> "…"
        2 -> Copy.charadesPrompt
        3 -> outcome
        else -> outcome
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.charadesTitle,
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
                    .testTag("charade-status")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            choices.take(2).forEach { mood ->
                PathChip(
                    skillLabel(mood),
                    enabled = phase == 2,
                    testTag = "charade-$mood",
                ) { guess(mood) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            choices.drop(2).forEach { mood ->
                PathChip(
                    skillLabel(mood),
                    enabled = phase == 2,
                    testTag = "charade-$mood",
                ) { guess(mood) }
            }
        }
        PathChip(
            Copy.charadeGo,
            enabled = phase == 0 || phase == 3,
            testTag = "charade-start",
        ) { start() }
    }
}

/** 表情木头人: the duck calls the round with its own timelines. */
@Composable
fun MoodFreeze(core: DuckCore?) {
    val sequence = listOf("excited", "angry", "excited", "yes")
    var playing by remember { mutableStateOf(false) }
    var banner by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    fun start() {
        if (playing) return
        playing = true
        scope.launch {
            // The round clock is each mood's own timeline length (excited
            // 3.3 s, angry 2.5 s, yes 1.7 s) plus a breath. Polling /state
            // would let a slow fetch skip a mood and let other games'
            // emotions bleed in; the published durations are the honest
            // schedule.
            for (mood in sequence) {
                banner = if (mood == "excited") Copy.freezeMove else Copy.freezeStop
                val started = core?.skill(mood) ?: 0
                delay(
                    when (mood) {
                        "excited" -> 3600
                        "angry" -> 2800
                        else -> 2000
                    }
                )
                if (started == 0) continue
            }
            banner = Copy.freezeDone
            playing = false
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.freezeTitle,
                color = Color.White,
                fontSize = DuckTheme.Type.Label,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.weight(1f))
            Text(
                banner,
                color = Color.White.copy(alpha = 0.75f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier
                    .testTag("freeze-banner")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        PathChip(Copy.freezeGo, enabled = !playing, testTag = "freeze-start") { start() }
    }
}

/** 它今天有没有: the duck answers from its own day facts, with the number. */
@Composable
fun DidItHappenToday(core: DuckCore?) {
    var fact by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    fun ask(field: String, tag: String) {
        scope.launch {
            val state = withContext(Dispatchers.IO) { fetchRawState() } ?: return@launch
            val life = try {
                JSONObject(state).optJSONObject("life") ?: return@launch
            } catch (_: Throwable) {
                return@launch
            }
            val started = life.optJSONObject("day_started")
            val today = life.optDouble(field, 0.0)
            val base = started?.optDouble(field, 0.0) ?: 0.0
            val delta = (today - base).coerceAtLeast(0.0)
            val yes = delta > 0.0
            core?.skill(if (yes) "yes" else "no")
            fact = if (field == "metres") {
                (if (yes) Copy.todayYes else Copy.todayNo) + " · " + Copy.todayMetres(delta)
            } else {
                (if (yes) Copy.todayYes else Copy.todayNo) + " · " + Copy.todayCount(delta.toInt())
            }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.todayTitle,
                color = Color.White,
                fontSize = DuckTheme.Type.Label,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.weight(1f))
            Text(
                fact,
                color = DuckTheme.Brass,
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier
                    .testTag("today-fact")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(Copy.todayWalk, testTag = "today-walk") { ask("metres", "walk") }
            PathChip(Copy.todayPets, testTag = "today-pets") { ask("pets", "pets") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(Copy.todayFalls, testTag = "today-falls") { ask("falls", "falls") }
            PathChip(Copy.todayFed, testTag = "today-fed") { ask("fed", "fed") }
        }
    }
}

/** 声音指挥官: the room answers with a clap or a whistle; the duck reacts. */
@Composable
fun SoundSimon(core: DuckCore?) {
    var status by remember { mutableStateOf("") }
    var listening by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun listen(kind: String) {
        if (listening) return
        listening = true
        status = if (kind == "clap") Copy.soundPromptLaugh else Copy.soundPromptCurious
        scope.launch {
            val start = try {
                JSONObject(withContext(Dispatchers.IO) { fetchRawState() } ?: "{}")
                    .optLong("tick", 0L)
            } catch (_: Throwable) {
                0L
            }
            var heard = false
            val deadline = System.currentTimeMillis() + 25_000
            while (System.currentTimeMillis() < deadline) {
                delay(300)
                val state = withContext(Dispatchers.IO) { fetchRawState() } ?: continue
                val event = try {
                    JSONObject(state).optJSONObject("sound_event")
                } catch (_: Throwable) {
                    null
                }
                if (event != null && event.optString("kind") == kind &&
                    event.optLong("tick", 0L) > start
                ) {
                    heard = true
                    break
                }
            }
            status = if (heard) Copy.soundHeard else Copy.soundMissed
            listening = false
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.soundTitle,
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
                    .testTag("soundgame-status")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(Copy.soundPromptLaugh, enabled = !listening, testTag = "sound-clap") {
                listen("clap")
            }
            PathChip(Copy.soundPromptCurious, enabled = !listening, testTag = "sound-whistle") {
                listen("whistle")
            }
        }
    }
}

/** 反向指令: the prompt names a mood; the child must perform its opposite.
 * Emotions always start fresh (no idempotent already-in-state answer like
 * sit/stand), so every correct press is a real mood with a real counter. */
@Composable
fun ReverseCommand(core: DuckCore?) {
    // prompt -> the opposite the child must press.
    val pairs = listOf(
        "laugh" to "sad",
        "sad" to "laugh",
        "excited" to "curious",
        "curious" to "excited",
    )
    var round by remember { mutableStateOf(0) }
    var notice by remember { mutableStateOf("") }
    val prompt = pairs[round % pairs.size].first
    val answer = pairs[round % pairs.size].second
    val faces = listOf("laugh", "sad", "excited", "curious")
    fun press(mood: String) {
        if (round >= 4) return
        if (mood != answer) {
            notice = Copy.reverseWrong
            return
        }
        if (core?.skill(mood) ?: 0 == 0) {
            notice = Copy.reverseWrong
            return
        }
        notice = ""
        round += 1
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.reverseTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                if (notice.isNotEmpty()) notice
                else if (round >= 4) Copy.rallyDone else skillLabel(prompt),
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("reverse-status").semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            faces.take(2).forEach { mood ->
                PathChip(skillLabel(mood), testTag = "reverse-$mood") { press(mood) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            faces.drop(2).forEach { mood ->
                PathChip(skillLabel(mood), testTag = "reverse-$mood") { press(mood) }
            }
            PathChip(Copy.echoStart, testTag = "reverse-start") { round = 0; notice = "" }
        }
    }
}

/** 照我做: show one real pose at a time; the child makes the duck do it. */
@Composable
fun PostureMirror(core: DuckCore?) {
    val poses = listOf("sit", "stand", "ground_pick", "roulade")
    var round by remember { mutableStateOf(0) }
    var notice by remember { mutableStateOf("") }
    fun press(name: String) {
        if (round >= poses.size) return
        if (name != poses[round]) {
            notice = Copy.reverseWrong
            return
        }
        val started = if (name == "ground_pick") {
            if (core?.groundPick() == 0) 1 else 0
        } else {
            core?.skill(name) ?: 0
        }
        if (started == 0) {
            notice = Copy.reverseWrong
            return
        }
        notice = ""
        round += 1
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.postureTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                if (notice.isNotEmpty()) notice
                else if (round >= poses.size) Copy.rallyDone else Copy.posturePrompt + " · " + skillLabel(poses[round]),
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("posture-status").semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(skillLabel("sit"), testTag = "posture-sit") { press("sit") }
            PathChip(skillLabel("stand"), testTag = "posture-stand") { press("stand") }
            PathChip(skillLabel("ground_pick"), testTag = "posture-pick") { press("ground_pick") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(skillLabel("roulade"), testTag = "posture-roll") { press("roulade") }
            PathChip(Copy.echoStart, testTag = "posture-start") { round = 0; notice = "" }
        }
    }
}

/** 红灯停绿灯行: green keeps a real forward command going; red stops dead. */
@Composable
fun RedLightGreenLight(core: DuckCore?, input: DuckInput) {
    var green by remember { mutableStateOf(false) }
    LaunchedEffect(green) {
        while (green) {
            input.vx = 0.4
            input.wz = 0.0
            delay(100)
        }
        input.vx = 0.0
        input.wz = 0.0
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.lightTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                if (green) Copy.lightGreen else Copy.lightRed,
                color = if (green) DuckTheme.StateGood else Color.White.copy(alpha = 0.7f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("light-status").semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            PathChip(Copy.lightGreen, testTag = "light-green") { green = true }
            PathChip(Copy.lightRed, testTag = "light-red") { green = false }
        }
    }
}

/** 快进一天: the duck's own care clock at 60x for a minute of real life. */
@Composable
fun FastForwardDay() {
    var fast by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.fastTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                if (fast) Copy.fastOn else Copy.fastNote,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("fast-status").semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        PathChip(if (fast) Copy.fastOff else Copy.fastOn, testTag = "fast-toggle") {
            fast = !fast
            postDuckCommand("/config", if (fast) "{\"care_time_scale\":60}" else "{\"care_time_scale\":1}")
        }
    }
}

/** 找一找: the prompt names a body part; the touch is the real reaction. */
@Composable
fun FindIt(core: DuckCore?) {
    val parts = listOf("head", "belly", "feet")
    var round by remember { mutableStateOf(0) }
    var notice by remember { mutableStateOf("") }
    val prompts = mapOf("head" to Copy.findHead, "belly" to Copy.findBelly, "feet" to Copy.findFeet)
    fun press(part: String) {
        if (round >= parts.size || part != parts[round]) {
            notice = Copy.reverseWrong
            return
        }
        when (part) {
            "head" -> core?.skill("curious")
            "belly" -> postDuckCommand("/pet")
            else -> core?.skill("kick_left")
        }
        notice = ""
        round += 1
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.findTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                if (notice.isNotEmpty()) notice
                else if (round >= parts.size) Copy.rallyDone else Copy.findPrompt + " · " + prompts[parts[round]],
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("find-status").semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            parts.forEach { part ->
                PathChip(prompts.getValue(part), testTag = "find-$part") { press(part) }
            }
        }
        PathChip(Copy.echoStart, testTag = "find-start") { round = 0; notice = "" }
    }
}

/** 小账本: today's real deltas, read straight from the duck's own book. */
@Composable
fun LittleLedger(core: DuckCore?) {
    var lines by remember { mutableStateOf(listOf<String>()) }
    val scope = rememberCoroutineScope()
    fun read() {
        scope.launch {
            val state = withContext(Dispatchers.IO) { fetchRawState() } ?: return@launch
            val life = try { JSONObject(state).optJSONObject("life") } catch (_: Throwable) { null } ?: return@launch
            val day = life.optJSONObject("day_started")
            fun delta(name: String) = (life.optDouble(name, 0.0) - (day?.optDouble(name, 0.0) ?: 0.0)).coerceAtLeast(0.0)
            val skills = life.optJSONObject("skills")
            var tricks = 0
            if (skills != null) {
                for (key in skills.keys()) tricks += skills.optInt(key, 0)
            }
            lines = listOf(
                Copy.ledgerWalk + " " + Copy.todayMetres(delta("metres")),
                Copy.ledgerPets + " " + Copy.todayCount(delta("pets").toInt()),
                Copy.ledgerFalls + " " + Copy.todayCount(delta("falls").toInt()),
                Copy.ledgerFed + " " + Copy.todayCount(delta("fed").toInt()),
                Copy.ledgerSkills + " " + Copy.todayCount(tricks),
            )
            core?.skill("yes")
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.ledgerTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            PathChip(Copy.ledgerRefresh, testTag = "ledger-read") { read() }
        }
        lines.forEach { line ->
            Text(line, color = DuckTheme.Brass, fontSize = DuckTheme.Type.Caption, modifier = Modifier.testTag("ledger-line"))
        }
    }
}

/** 传悄悄话: the duck whispers one of its voices; the child names it. */
@Composable
fun WhisperGame(core: DuckCore?) {
    val tags = listOf("chirp", "coo", "inquire", "peck")
    var target by remember { mutableStateOf("") }
    var phase by remember { mutableStateOf(0) } // 0 idle, 1 guessing, 2 answered
    var notice by remember { mutableStateOf("") }
    fun listen() {
        val tick = System.currentTimeMillis() / 1000
        target = tags[(tick % tags.size).toInt()]
        notice = ""
        phase = 1
        core?.queueSound(target)
    }
    fun guess(tag: String) {
        if (phase != 1) return
        val right = tag == target
        core?.skill(if (right) "yes" else "no")
        notice = if (right) Copy.charadesRight else Copy.charadesWrong
        phase = 2
    }
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(Copy.whisperTitle, color = Color.White, fontSize = DuckTheme.Type.Label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                when (phase) {
                    1 -> Copy.whisperPrompt
                    2 -> notice
                    else -> ""
                },
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Caption,
                modifier = Modifier.testTag("whisper-status").semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            tags.take(2).forEach { tag ->
                PathChip(Copy.echoSound(tag), enabled = phase == 1, testTag = "whisper-$tag") { guess(tag) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
            tags.drop(2).forEach { tag ->
                PathChip(Copy.echoSound(tag), enabled = phase == 1, testTag = "whisper-$tag") { guess(tag) }
            }
            PathChip(if (phase == 0) Copy.whisperListen else Copy.whisperAgain, testTag = "whisper-start") { listen() }
        }
    }
}
