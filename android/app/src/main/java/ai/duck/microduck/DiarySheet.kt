package ai.duck.microduck

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import kotlinx.coroutines.withContext

/**
 * The duck's diary: today's REAL record, and — when the person configured
 * their own model — the paragraph that model writes from those facts.
 * Without a model the facts stand alone; nothing here is fabricated.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DiarySheet(onClose: () -> Unit) {
    val context = LocalContext.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var lines by remember { mutableStateOf(emptyList<String>()) }
    var modelText by remember { mutableStateOf("") }
    var modelError by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(true) }
    // The system voice reading the day aloud — the honest stand-in, not the
    // duck's own formant synth. While it speaks the callback drives the same
    // client intent a robot-side chorus writes (POST /mouth), so the beak
    // opens on the duck itself and /state shows it to any client watching.
    // The core keeps the honest overrides: asleep shuts the beak regardless.
    var ttsSpeaking by remember { mutableStateOf(false) }
    val speaker = remember {
        Speaker(context) { speaking ->
            ttsSpeaking = speaking
            postDuckCommand("/mouth", if (speaking) """{"open":1.0}""" else """{"open":0.0}""")
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            // The engine outlives the sheet; a duck that keeps reading the
            // room after the page is gone would be a wrong mouth too.
            speaker.shutdown()
        }
    }
    LaunchedEffect(Unit) {
        val facts = withContext(Dispatchers.IO) {
            val events = fetchLifeEvents()
            val metres = fetchLifeRecord()?.metres ?: 0.0
            // The same numbers the iOS diary gives the model. One duck, one
            // set of facts: two clients that assemble different facts write
            // two different diaries about the same day.
            val care = fetchCare()
            val lines = buildList {
                val pets = events.count { it == "pet" }
                val feeds = events.count { it == "feed" }
                val rituals = events.count { it == "idle_ritual" }
                val falls = events.count { isFall(it) }
                val moods = events.count { it.startsWith("emotion:") }
                val gifts = events.count { it.startsWith("gift:") }
                if (pets > 0) add(Copy.awayPets(pets))
                if (feeds > 0) add(Copy.awayFeeds(feeds))
                if (rituals > 0) add(Copy.awayRituals(rituals))
                if (falls > 0) add(Copy.awayFalls(falls))
                if (moods > 0) add(Copy.awayMoods(moods))
                if (gifts > 0) add(Copy.diaryGifts(gifts))
                if (metres >= 0.5) add(Copy.postcardMetres(metres))
            }
            val summary = buildString {
                append("events: ")
                append(events.takeLast(40).joinToString(","))
                append("\npets=")
                append(events.count { it == "pet" })
                append(" feeds=")
                append(events.count { it == "feed" })
                append(" rituals=")
                append(events.count { it == "idle_ritual" })
                append(" falls=")
                append(events.count { isFall(it) })
                append(" moods=")
                append(events.count { it.startsWith("emotion:") })
                append(" gifts=")
                append(events.count { it.startsWith("gift:") })
                append("\nmetres=")
                append("%.1f".format(metres))
                append(" hunger=")
                append("%.2f".format(care?.hunger ?: 0.0))
                append(" mood=")
                append("%.2f".format(care?.mood ?: 0.0))
                append("\nWrite in ")
                append(if (Copy.language == Copy.Language.Chinese) "Chinese." else "English.")
            }
            lines to summary
        }
        lines = facts.first
        val config = AIStore.load(context)
        if (!config.enabled || config.endpoint.isBlank()) {
            busy = false
            return@LaunchedEffect
        }
        val (text, error) = withContext(Dispatchers.IO) {
            AIBrain.writeDiary(config, facts.second)
        }
        modelText = text.orEmpty()
        modelError = error
        busy = false
    }
    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.diary,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 18.sp,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("diary-title"),
                )
                val shareText = if (modelText.isNotEmpty()) modelText else lines.joinToString("\n")
                if (shareText.isNotEmpty()) {
                    ButtonChip(
                        if (ttsSpeaking) Copy.stopSpeaking else Copy.speak,
                        "diary-speak",
                    ) {
                        if (ttsSpeaking) {
                            speaker.stop()
                        } else {
                            // Copy.language is Compose state: read it here, on
                            // the main thread, and hand the locale down.
                            val locale = if (Copy.language == Copy.Language.Chinese) {
                                java.util.Locale.SIMPLIFIED_CHINESE
                            } else {
                                java.util.Locale.US
                            }
                            speaker.speak(shareText, locale)
                        }
                    }
                    ButtonChip(Copy.share, "diary-share") {
                        val send = Intent(Intent.ACTION_SEND)
                            .setType("text/plain")
                            .putExtra(Intent.EXTRA_TEXT, shareText)
                        context.startActivity(Intent.createChooser(send, Copy.diary))
                    }
                }
                CloseChip("diary-close", onClose)
            }
            Text(
                Copy.diaryFactsTitle,
                color = Color.White.copy(alpha = 0.55f),
                fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold,
            )
            if (lines.isEmpty()) {
                Text(Copy.diaryEmpty, color = Color.White.copy(alpha = 0.6f), fontSize = 12.sp)
            } else {
                LazyColumn(Modifier.height(160.dp)) {
                    items(lines) { line ->
                        Text(
                            "· $line",
                            color = Color.White.copy(alpha = 0.85f),
                            fontSize = 13.sp,
                            modifier = Modifier.padding(vertical = 2.dp),
                        )
                    }
                }
            }
            Text(
                Copy.diaryModelTitle,
                color = Color.White.copy(alpha = 0.55f),
                fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold,
            )
            when {
                busy -> Text(Copy.diaryBusy, color = Color.White.copy(alpha = 0.6f), fontSize = 12.sp)
                modelText.isNotEmpty() -> Text(
                    modelText,
                    color = Color.White,
                    fontSize = 14.sp,
                    modifier = Modifier.testTag("diary-model-text"),
                )
                modelError != null -> Text(
                    modelError.orEmpty(),
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 11.sp,
                )
                else -> Text(
                    Copy.diaryNeedModel,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 12.sp,
                )
            }
            Text(
                Copy.speakNote,
                color = Color.White.copy(alpha = 0.55f),
                fontSize = 11.sp,
            )
        }
    }
}
