package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay

/**
 * The choreography sheet: big tiles, Record and Play. The child authors by
 * doing; the replay goes through the same shell buttons the tiles came from.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DanceSheet(
    studio: DanceStudio,
    onSkill: (String) -> Unit,
    onSound: (String) -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var recording by remember { mutableStateOf(studio.recording) }
    var playing by remember { mutableStateOf(studio.playing) }
    var activeStep by remember { mutableStateOf(studio.activeStep) }
    var revision by remember { mutableStateOf(0) }

    fun refresh() {
        recording = studio.recording
        playing = studio.playing
        activeStep = studio.activeStep
        revision += 1
    }

    LaunchedEffect(Unit) {
        // Keep the sheet's record/playback labels in sync with the studio.
        while (true) {
            activeStep = if (studio.playing) activeStep else studio.activeStep
            playing = studio.playing
            delay(120)
        }
    }

    // Replay the recorded steps through the same intents their tiles came
    // from. One coroutine for the whole strip; cancellation via studio.stop().
    LaunchedEffect(playing) {
        if (!playing) return@LaunchedEffect
        val steps = studio.steps.toList()
        steps.forEachIndexed { index, step ->
            activeStep = index
            when (step.kind) {
                "skill" -> onSkill(step.value)
                "sound" -> onSound(step.value)
            }
            delay(800)
        }
        studio.finished()
        activeStep = -1
    }

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(
            Modifier
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.dance,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 18.sp,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("dance-title"),
                )
                CloseChip("dance-close", onClose)
            }
            Text(
                Copy.danceHint,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = 11.sp,
            )
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 88.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 220.dp)
                    .testTag("dance-tiles"),
            ) {
                if (studio.steps.isEmpty()) {
                    item {
                        Text(
                            Copy.danceEmpty,
                            color = Color.White.copy(alpha = 0.6f),
                            fontSize = 12.sp,
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 64.dp),
                        )
                    }
                }
                itemsIndexed(studio.steps) { index, step ->
                    Column(
                        Modifier
                            .background(
                                if (playing && activeStep == index) Color(0xFFFFD54F)
                                else Color.White.copy(alpha = 0.08f),
                                RoundedCornerShape(12.dp),
                            )
                            .padding(8.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(3.dp),
                    ) {
                        Text(
                            "${index + 1}",
                            color = Color.White.copy(alpha = 0.55f),
                            fontSize = 9.sp,
                        )
                        Text(
                            step.label.ifEmpty { step.value },
                            color = Color.White.copy(alpha = 0.9f),
                            fontSize = 11.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            Text(
                Copy.danceTry,
                color = Color.White.copy(alpha = 0.55f),
                fontSize = 10.sp,
            )
            Row(
                Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                listOf("roulade", "kick_left", "laugh", "yes").forEach { name ->
                    ButtonChip(skillLabel(name), "dance-try-$name") {
                        onSkill(name)
                        refresh()
                    }
                }
                ButtonChip(Copy.danceStepSound, "dance-try-chirp") {
                    onSound("chirp")
                    refresh()
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                ButtonChip(
                    if (recording) Copy.danceStopRecord else Copy.danceRecord,
                    "dance-record",
                ) {
                    studio.toggleRecording()
                    refresh()
                }
                ButtonChip(
                    if (playing) Copy.danceStop else Copy.dancePlay,
                    "dance-play",
                ) {
                    if (playing) {
                        studio.stop()
                        refresh()
                    } else {
                        val steps = studio.beginPlay()
                        if (steps != null) {
                            refresh()
                        }
                    }
                }
                ButtonChip(Copy.deleteRule, "dance-clear") {
                    studio.clear()
                    refresh()
                }
            }
        }
    }
}
