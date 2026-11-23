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
import androidx.compose.foundation.layout.navigationBarsPadding
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
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
// The shared table lives in DuckTheme.kt (UI_TOKENS §7); these two
// names kept their call sites.
internal val ShellSurface = DuckTheme.BgSurface
internal val ActionAccent = DuckTheme.AccentAction

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
    twoPlayerOn: Boolean,
    stick: String,
    onPath: () -> Unit,
    onOpen: (DuckRoute) -> Unit,
    onNight: () -> Unit,
    onTwoPlayer: () -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val context = LocalContext.current
    var postcardCount by remember { mutableStateOf(0) }
    LaunchedEffect(Unit) {
        postcardCount = withContext(Dispatchers.IO) { PostcardShelf.load(context).size }
    }
    val unfinished = board.quests.count { !it.done }
    val aiReady = aiConfig.enabled && aiConfig.endpoint.isNotBlank()

    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        // Direction A (design-demos/A-complete.html `.drawer`): the drawer is
        // a wooden cabinet — wood gradient panel, brass group words.
        containerColor = DuckTheme.WoodDeep,
    ) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .background(
                    Brush.verticalGradient(
                        listOf(DuckTheme.WoodHi, DuckTheme.Wood, DuckTheme.WoodDeep)
                    )
                )
                .padding(horizontal = DuckTheme.Space.xl)
                // The last row must be able to scroll clear of the system
                // gesture inset: at 2x fonts 每日任务 measured [56,619][169,640]
                // and its tap centre fell in the gesture bar (a real
                // reachability defect, not only a test artefact).
                .navigationBarsPadding()
                .padding(top = DuckTheme.Space.xs, bottom = DuckTheme.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.lg),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.more,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = DuckTheme.Type.Title,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("more-title").duckSheetFirstFocus(),
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
                    onOpen(DuckRoute.Rules)
                }
                MoreRow(Icons.dance, Copy.dance, note = Copy.danceRowNote, tag = "more-dance") {
                    onOpen(DuckRoute.Dance)
                }
                MoreRow(
                    Icons.controls, Copy.playMore,
                    note = Copy.stick(stick),
                    tag = "more-playmore",
                ) { onOpen(DuckRoute.PlayMore) }
            }
            Group(Copy.groupTogether, "together") {
                MoreRow(
                    Icons.peers, Copy.peers,
                    note = Copy.peersRowNote,
                    badge = "${peers.peers.size}/3",
                    tag = "more-peers",
                ) { onOpen(DuckRoute.Peers) }
                MoreRow(Icons.chorus, Copy.chorus, tag = "more-chorus") {
                    onOpen(DuckRoute.Peers)
                }
                MoreRow(Icons.gift, Copy.gifts, tag = "more-gifts") {
                    onOpen(DuckRoute.Peers)
                }
                MoreRow(
                    Icons.twoPlayer, Copy.twoPlayer,
                    check = twoPlayerOn,
                    showsChevron = false,
                    tag = "more-p2",
                ) { onTwoPlayer() }
            }
            Group(Copy.groupRecord, "record") {
                MoreRow(
                    Icons.quests, Copy.dailyQuests,
                    note = Copy.questsRowNote,
                    badge = if (board.quests.isEmpty()) null else "$unfinished",
                    tag = "more-quests",
                ) { onOpen(DuckRoute.Progress) }
                MoreRow(Icons.diary, Copy.diary, tag = "more-diary") {
                    onOpen(DuckRoute.Diary)
                }
                MoreRow(
                    Icons.postcards, Copy.postcardShelf,
                    badge = "$postcardCount",
                    tag = "more-postcards",
                ) { onOpen(DuckRoute.Postcards) }
                MoreRow(
                    Icons.dex, Copy.collectionTitle,
                    note = Copy.dexRowNote,
                    tag = "more-dex",
                ) { onOpen(DuckRoute.Dex) }
            }
            Group(Copy.thisDuck, "duck") {
                MoreRow(
                    Icons.ai, Copy.ai,
                    note = if (aiReady) Copy.aiOn else Copy.aiNeedEndpoint,
                    dot = !aiReady,
                    tag = "more-ai",
                ) { onOpen(DuckRoute.AI) }
                MoreRow(
                    if (nightActive) Icons.morning else Icons.night,
                    if (nightActive) Copy.goodMorning else Copy.goodNight,
                    note = if (nightActive) Copy.morningRowNote else Copy.nightRowNote,
                    showsChevron = false,
                    tag = "more-night",
                ) { onNight() }
                MoreRow(
                    Icons.preferences, Copy.duckFace,
                    note = Copy.heartRowNote,
                    tag = "more-duckpanel",
                ) { onOpen(DuckRoute.DuckPanel) }
                MoreRow(
                    Icons.tools, Copy.tools,
                    note = Copy.roomLampNote,
                    tag = "more-tools",
                ) { onOpen(DuckRoute.Tools) }
            }
        }
    }
}

/**
 * The Record group's daily page: today's quests, the honest growth book
 * (streak, last seven days, ten achievements) and the core's day rows -
 * the IA table's quests page, at three controls.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun QuestsSheet(board: QuestBoardUi, onClose: () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val growth by DuckProgress.snapshot.collectAsState()
    var dayRows by remember { mutableStateOf(emptyList<String>()) }
    LaunchedEffect(Unit) {
        // Crossing midnight while the app slept: recompute, never rewrite.
        DuckProgress.refresh()
        dayRows = withContext(Dispatchers.IO) { fetchLifeRecord()?.days ?: emptyList() }
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
                .padding(horizontal = DuckTheme.Space.xl)
                .padding(bottom = DuckTheme.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.dailyQuests,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = DuckTheme.Type.Title,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("quests-sheet-title").duckSheetFirstFocus(),
                )
                CloseChip("quests-close", onClose)
            }
            QuestCard(board)
            GrowthCard(growth, dayRows)
        }
    }
}

/**
 * The growth book: real counts only. With no history it says so in one
 * line and shows no zeros dressed as achievements; every number below is
 * the ledger's own record (docs/PROGRESS_ANDROID.md).
 */
@Composable
private fun GrowthCard(growth: ProgressSnapshot, dayRows: List<String>) {
    Column(
        Modifier.fillMaxWidth().testTag("growth-book"),
        verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
    ) {
        Text(
            Copy.growthBook,
            color = Color.White,
            style = MaterialTheme.typography.titleMedium,
        )
        if (!growth.hasAnyRecord) {
            Text(
                Copy.growthEmpty,
                color = Color.White.copy(alpha = 0.6f),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.testTag("growth-empty"),
            )
        } else {
            Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.md)) {
                Text(
                    Copy.growthStreak(growth.streak),
                    color = ActionAccent,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.testTag("growth-streak"),
                )
                Text(
                    Copy.growthBest(growth.bestStreak),
                    color = Color.White.copy(alpha = 0.7f),
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(
                    Copy.growthTasks(growth.tasksDoneToday, GrowthTasks.catalog.size),
                    color = Color.White.copy(alpha = 0.7f),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            Text(
                Copy.growthWeek,
                color = Color.White.copy(alpha = 0.55f),
                style = MaterialTheme.typography.labelSmall,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) {
                growth.week.forEach { tile ->
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            Copy.weekday(tile.weekday),
                            color = Color.White.copy(alpha = 0.55f),
                            style = MaterialTheme.typography.labelSmall,
                        )
                        Box(
                            Modifier
                                .size(20.dp)
                                .background(
                                    if (tile.active) ActionAccent.copy(alpha = 0.55f)
                                    else Color.White.copy(alpha = 0.08f),
                                    CircleShape,
                                )
                                .testTag("growth-day-${tile.id}"),
                        )
                    }
                }
            }
        }
        if (dayRows.isNotEmpty()) {
            Text(
                dayRows.takeLast(7).joinToString(" | "),
                color = Color.White.copy(alpha = 0.55f),
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.testTag("day-rows"),
            )
        }
        Text(
            Copy.growthAchievements,
            color = Color.White.copy(alpha = 0.55f),
            style = MaterialTheme.typography.labelSmall,
        )
        GrowthAchievements.catalog.forEach { entry ->
            val unlockedOn = growth.unlocked[entry.id]
            Row(
                Modifier
                    .fillMaxWidth()
                    .testTag("growth-achievement-${entry.id}"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
            ) {
                Box(
                    Modifier
                        .size(10.dp)
                        .background(
                            if (unlockedOn != null) ActionAccent
                            else Color.White.copy(alpha = 0.18f),
                            CircleShape,
                        ),
                )
                Column(Modifier.weight(1f)) {
                    Text(
                        GrowthAchievements.name(entry.id),
                        color = if (unlockedOn != null) Color.White else Color.White.copy(alpha = 0.5f),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        if (unlockedOn != null) Copy.growthUnlockedOn(unlockedOn)
                        else GrowthAchievements.detail(entry.id),
                        color = Color.White.copy(alpha = 0.45f),
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
            }
        }
    }
}

@Composable
private fun Group(
    title: String,
    id: String,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs)) {
        Text(
            title,
            color = DuckTheme.Brass,
            fontSize = DuckTheme.Type.Label,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier
                .padding(start = DuckTheme.Space.xs)
                .padding(bottom = DuckTheme.Space.xxs)
                .testTag("more-group-$id"),
        )
        Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs), content = content)
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
    // One row implementation (D2): the drawer's row is the shared
    // DuckListRow with the drawer's extras mapped on — the badge rides in
    // the value slot and `dot`/`check` both read as "selected".
    DuckListRow(
        title = title,
        onClick = onClick,
        icon = icon,
        note = note,
        value = badge,
        selected = check || dot,
        showChevron = showsChevron,
        testTag = tag,
    )
}

/** The three stick modes, one tap each; path has its own row. */
@Composable
internal fun ModeDialog(
    current: String,
    onPick: (String) -> Unit,
    onClose: () -> Unit,
) {
    Dialog(onDismissRequest = onClose) {
        Column(
            Modifier
                .background(ShellSurface, RoundedCornerShape(DuckTheme.Radius.card))
                .padding(DuckTheme.Space.xl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs),
        ) {
            Text(
                Copy.stickModes,
                color = Color.White,
                fontSize = DuckTheme.Type.Title,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(bottom = DuckTheme.Space.xs),
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
                        .padding(horizontal = DuckTheme.Space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
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
                        fontSize = DuckTheme.Type.Body,
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
