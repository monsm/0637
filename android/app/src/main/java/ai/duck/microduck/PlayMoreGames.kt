package ai.duck.microduck

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
        scope.launch {
            while (playingEmotion(withContext(Dispatchers.IO) { fetchRawState() })) delay(300)
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
