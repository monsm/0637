package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons as MaterialIcons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** UI_TOKENS v1 literals until B2 grows DuckTheme.kt. */
internal val ShellSurface = Color(0xFF221F1D)   // bg.surface, warm
internal val ActionAccent = Color(0xFFFF8A3D)   // accent.action

/**
 * The one More drawer (B4 §2), mirrored from iOS's MoreSheet.swift: four
 * groups, at most four rows each, every row one tap from Live. The chorus
 * and the gifts are actions inside the neighbours page, not pages of their
 * own; tilt-to-look is a drive mode and stays in the This duck panel.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun MoreSheet(
    board: QuestBoardUi,
    peers: PeersUi,
    aiConfig: AIConfig,
    nightActive: Boolean,
    flashlightOn: Boolean,
    stick: String,
    onPath: () -> Unit,
    onMode: (String) -> Unit,
    onRules: () -> Unit,
    onDance: () -> Unit,
    onPeers: () -> Unit,
    onQuests: () -> Unit,
    onDiary: () -> Unit,
    onPostcards: () -> Unit,
    onDex: () -> Unit,
    onAI: () -> Unit,
    onNight: () -> Unit,
    onFlashlight: () -> Unit,
    onLanguage: () -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val context = LocalContext.current
    var showModes by remember { mutableStateOf(false) }
    var postcardCount by remember { mutableStateOf(0) }
    LaunchedEffect(Unit) {
        postcardCount = withContext(Dispatchers.IO) { PostcardShelf.load(context).size }
    }
    val unfinished = board.quests.count { !it.done }
    val aiReady = aiConfig.enabled && aiConfig.endpoint.isNotBlank()

    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = ShellSurface,
    ) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(top = 4.dp, bottom = 28.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.more,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 20.sp,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("more-title"),
                )
                CloseChip("more-close", onClose)
            }
            Group(Copy.groupPlay, "play") {
                MoreRow(
                    Icons.path, Copy.pathMode,
                    note = Copy.pathRowNote,
                    check = stick == "path",
                    tag = "more-path",
                ) { onPath() }
                MoreRow(Icons.blocks, Copy.blocks, note = Copy.blocksRowNote, tag = "more-blocks") {
                    onRules()
                }
                MoreRow(Icons.dance, Copy.dance, note = Copy.danceRowNote, tag = "more-dance") {
                    onDance()
                }
                MoreRow(
                    Icons.controls, Copy.stickModes,
                    note = Copy.stick(stick),
                    tag = "more-controls",
                ) { showModes = true }
            }
            Group(Copy.groupTogether, "together") {
                MoreRow(
                    Icons.peers, Copy.peers,
                    note = Copy.peersRowNote,
                    badge = "${peers.peers.size}/3",
                    tag = "more-peers",
                ) { onPeers() }
            }
            Group(Copy.groupRecord, "record") {
                MoreRow(
                    Icons.quests, Copy.dailyQuests,
                    note = Copy.questsRowNote,
                    badge = if (board.quests.isEmpty()) null else "$unfinished",
                    tag = "more-quests",
                ) { onQuests() }
                MoreRow(Icons.diary, Copy.diary, tag = "more-diary") { onDiary() }
                MoreRow(
                    Icons.postcards, Copy.postcardShelf,
                    badge = "$postcardCount",
                    tag = "more-postcards",
                ) { onPostcards() }
                MoreRow(Icons.dex, Copy.dex, note = Copy.dexRowNote, tag = "more-dex") {
                    onDex()
                }
            }
            Group(Copy.thisDuck, "duck") {
                MoreRow(
                    Icons.ai, Copy.ai,
                    note = if (aiReady) Copy.aiOn else Copy.aiNeedEndpoint,
                    dot = !aiReady,
                    tag = "more-ai",
                ) { onAI() }
                MoreRow(
                    if (nightActive) Icons.morning else Icons.night,
                    if (nightActive) Copy.goodMorning else Copy.goodNight,
                    note = if (nightActive) Copy.morningRowNote else Copy.nightRowNote,
                    showsChevron = false,
                    tag = "more-night",
                ) { onNight() }
                MoreRow(
                    Icons.flashlight, Copy.flashlight,
                    note = if (flashlightOn) Copy.flashlightOn else Copy.flashlightOff,
                    check = flashlightOn,
                    showsChevron = false,
                    tag = "more-flashlight",
                ) { onFlashlight() }
                MoreRow(
                    Icons.language, Copy.languageRow,
                    note = Copy.language.label,
                    showsChevron = false,
                    tag = "more-language",
                ) { onLanguage() }
            }
        }
    }
    if (showModes) {
        ModeDialog(
            current = stick,
            onPick = { onMode(it); showModes = false },
            onClose = { showModes = false },
        )
    }
}

/** One quest board in its own sheet: the daily entry from the Record group. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun QuestsSheet(board: QuestBoardUi, onClose: () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = ShellSurface,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 20.dp)
                .padding(bottom = 28.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.dailyQuests,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 20.sp,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("quests-sheet-title"),
                )
                CloseChip("quests-close", onClose)
            }
            QuestCard(board)
        }
    }
}

@Composable
private fun Group(
    title: String,
    id: String,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(
            title,
            color = Color.White.copy(alpha = 0.6f),
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier
                .padding(start = 4.dp)
                .padding(bottom = 2.dp)
                .testTag("more-group-$id"),
        )
        Column(verticalArrangement = Arrangement.spacedBy(2.dp), content = content)
    }
}

/**
 * One row: 48 dp tall, a 20 dp icon, a 15 sp title, an optional 12 sp note,
 * and a badge on the right. Text capsules are chips, not actions (B4 §3).
 */
@Composable
private fun MoreRow(
    icon: ImageVector,
    title: String,
    note: String? = null,
    badge: String? = null,
    dot: Boolean = false,
    check: Boolean = false,
    showsChevron: Boolean = true,
    tag: String,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = title,
                role = Role.Button,
                onClick = onClick,
            )
            .testTag(tag)
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint = Color.White.copy(alpha = 0.85f),
            modifier = Modifier.size(20.dp),
        )
        Column(
            Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(
                title,
                color = Color.White,
                fontSize = 15.sp,
                fontWeight = FontWeight.Medium,
            )
            if (note != null) {
                Text(
                    note,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = 12.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (check) {
            Icon(
                MaterialIcons.Filled.Check,
                contentDescription = null,
                tint = ActionAccent,
                modifier = Modifier.size(16.dp),
            )
        }
        if (dot) {
            Box(Modifier.size(8.dp).background(ActionAccent, CircleShape))
        }
        if (badge != null) {
            Text(
                badge,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = 12.sp,
            )
        }
        if (showsChevron) {
            Icon(
                MaterialIcons.Outlined.ChevronRight,
                contentDescription = null,
                tint = Color.White.copy(alpha = 0.65f),
                modifier = Modifier.size(16.dp),
            )
        }
    }
}

/** The three stick modes, one tap each; path has its own row. */
@Composable
private fun ModeDialog(
    current: String,
    onPick: (String) -> Unit,
    onClose: () -> Unit,
) {
    Dialog(onDismissRequest = onClose) {
        Column(
            Modifier
                .background(ShellSurface, RoundedCornerShape(20.dp))
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                Copy.stickModes,
                color = Color.White,
                fontSize = 18.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(bottom = 4.dp),
            )
            listOf("drive", "head", "body").forEach { mode ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            onClickLabel = Copy.stick(mode),
                            role = Role.Button,
                        ) { onPick(mode) }
                        .testTag("mode-option-$mode")
                        .padding(horizontal = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(
                        Icons.stick(mode),
                        contentDescription = null,
                        tint = Color.White.copy(alpha = 0.85f),
                        modifier = Modifier.size(20.dp),
                    )
                    Text(
                        Copy.stick(mode),
                        color = Color.White,
                        fontSize = 15.sp,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier.weight(1f),
                    )
                    if (current == mode) {
                        Icon(
                            MaterialIcons.Filled.Check,
                            contentDescription = null,
                            tint = ActionAccent,
                            modifier = Modifier.size(16.dp),
                        )
                    }
                }
            }
        }
    }
}
