package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * One official mood or trick and the duck's own lifetime record of it.
 * `firstDay` is copied verbatim from the core; an empty string means the day
 * is unknown and the page shows — rather than inventing one.
 */
internal data class CollectionEntry(
    val name: String,
    val count: Int,
    val firstDay: String,
)

/** GET /collection: the duck's lifetime record, sectioned as it publishes it. */
internal data class CollectionUi(
    val emotions: List<CollectionEntry> = emptyList(),
    val skills: List<CollectionEntry> = emptyList(),
    val moodsUnlocked: Int = 0,
    val moodsTotal: Int = 0,
    val tricksUnlocked: Int = 0,
    val tricksTotal: Int = 0,
)

/** The first-day cell: unknown stays unknown, never a guessed date. */
internal fun collectionFirstDay(entry: CollectionEntry): String =
    entry.firstDay.ifBlank { "—" }

/** A zero count is the locked state: the duck has never done it. */
internal fun collectionLocked(entry: CollectionEntry): Boolean = entry.count == 0

/**
 * What one row shows. A locked row has NO date line at all: the core does not
 * publish a first day for something it never did, and a stale `first_day`
 * from an older core is not rendered either.
 */
internal data class CollectionRowUi(
    val name: String,
    val count: Int,
    val locked: Boolean,
    /** The value after "First unlocked", or null when the row is locked. */
    val firstDay: String?,
)

internal fun collectionRow(entry: CollectionEntry): CollectionRowUi {
    val locked = collectionLocked(entry)
    return CollectionRowUi(
        name = entry.name,
        count = entry.count,
        locked = locked,
        firstDay = if (locked) null else collectionFirstDay(entry),
    )
}

/** Most-used first; equal counts are alphabetical so the list is stable. */
internal fun sortCollection(entries: List<CollectionEntry>): List<CollectionEntry> =
    entries.sortedWith(
        compareByDescending<CollectionEntry> { it.count }.thenBy { it.name }
    )

private fun parseEntries(array: JSONArray?): List<CollectionEntry> {
    if (array == null) return emptyList()
    return (0 until array.length()).mapNotNull { index ->
        val item = array.optJSONObject(index) ?: return@mapNotNull null
        val rawName = item.opt("name")
        val name = if (rawName == null || rawName == JSONObject.NULL) {
            ""
        } else {
            rawName.toString().trim()
        }
        if (name.isEmpty()) return@mapNotNull null
        // A JSON null and a missing key both mean "the day is not known";
        // neither may become the word "null" in the first-day cell.
        val rawDay = item.opt("first_day")
        CollectionEntry(
            name = name,
            count = item.optInt("count", 0),
            firstDay = if (rawDay == null || rawDay == JSONObject.NULL) "" else rawDay.toString(),
        )
    }
}

private fun JSONObject.intOr(key: String, fallback: Int): Int =
    if (has(key)) optInt(key, fallback) else fallback

/**
 * The F1 contract:
 * `{"ok":true,"emotions":[{"name","count","first_day"}],
 *   "skills":[...],"moods":{"unlocked","total"},"tricks":{...}}`.
 * A document without the two arrays is not a collection answer and reads as
 * null, so the page can stay honest instead of showing an empty collection
 * the core never sent.
 */
internal fun parseCollection(text: String): CollectionUi? = try {
    val root = JSONObject(text)
    if (!root.optBoolean("ok", true)) {
        null
    } else if (!root.has("emotions") && !root.has("skills")) {
        null
    } else {
        val emotions = sortCollection(parseEntries(root.optJSONArray("emotions")))
        val skills = sortCollection(parseEntries(root.optJSONArray("skills")))
        val moods = root.optJSONObject("moods")
        val tricks = root.optJSONObject("tricks")
        CollectionUi(
            emotions = emotions,
            skills = skills,
            // The totals are the core's; a missing block can still be counted
            // from the rows it did send, never from a number this side made up.
            moodsUnlocked = moods?.intOr("unlocked", emotions.count { it.count > 0 })
                ?: emotions.count { it.count > 0 },
            moodsTotal = moods?.intOr("total", emotions.size) ?: emotions.size,
            tricksUnlocked = tricks?.intOr("unlocked", skills.count { it.count > 0 })
                ?: skills.count { it.count > 0 },
            tricksTotal = tricks?.intOr("total", skills.size) ?: skills.size,
        )
    }
} catch (_: Throwable) {
    null
}

internal fun fetchCollection(): CollectionUi? = try {
    parseCollection(getJson("/collection"))
} catch (_: Throwable) {
    null
}

/**
 * The collection page (COMPANION.md item 5): every official mood and trick,
 * the real lifetime count and the first day it was unlocked. Tapping a row
 * tries it through the same /skill intent the AI and the skill ring use, so
 * the page is also the way to unlock what is still locked.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CollectionSheet(onClose: () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    var data by remember { mutableStateOf<CollectionUi?>(null) }
    var loaded by remember { mutableStateOf(false) }
    // A skill's own count changes after a real execution; re-read once it has
    // had time to land. A failed refresh keeps the last honest numbers.
    var reload by remember { mutableStateOf(0) }
    LaunchedEffect(reload) {
        val fresh = withContext(Dispatchers.IO) { fetchCollection() }
        if (fresh != null) data = fresh
        loaded = true
    }
    val trySkill: (String) -> Unit = { name ->
        postDuckCommand("/skill", JSONObject().put("name", name).toString())
        scope.launch {
            delay(1200)
            reload += 1
        }
    }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = ShellSurface,
    ) {
        Column(
            Modifier.padding(DuckTheme.Space.xl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.collectionTitle,
                    color = Color.White,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("collection-title").duckSheetFirstFocus(),
                )
                CloseChip("collection-close", onClose)
            }
            val ui = data
            if (ui != null && ui.emotions.isEmpty() && ui.skills.isEmpty()) {
                DuckEmptyState(
                    title = Copy.collectionEmpty,
                    actionTitle = Copy.emptyCollectionAction,
                    onAction = {
                        postDuckCommand("/skill", JSONObject().put("name", "curious").toString())
                        scope.launch {
                            delay(1200)
                            reload += 1
                        }
                    },
                    testTag = "collection-empty",
                )
            } else if (ui != null) {
                Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.md)) {
                    Text(
                        "${Copy.collectionMoods} ${ui.moodsUnlocked}/${ui.moodsTotal}",
                        color = Color.White.copy(alpha = 0.75f),
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.testTag("collection-moods-count"),
                    )
                    Text(
                        "${Copy.collectionTricks} ${ui.tricksUnlocked}/${ui.tricksTotal}",
                        color = Color.White.copy(alpha = 0.75f),
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.testTag("collection-tricks-count"),
                    )
                }
                LazyColumn(
                    modifier = Modifier.heightIn(max = 460.dp),
                    verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs),
                ) {
                    collectionBadges(ui)
                    collectionSection(Copy.collectionMoods, "moods", ui.emotions, trySkill)
                    collectionSection(Copy.collectionTricks, "tricks", ui.skills, trySkill)
                }
            } else if (loaded) {
                // The core did not answer /collection: say so rather than draw
                // an empty collection that claims the duck never did anything.
                Text(
                    Copy.aiOffline,
                    color = Color.White.copy(alpha = 0.6f),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.testTag("collection-offline"),
                )
            }
        }
    }
}

/**
 * The badge shelf: the words come from Copy, the progress from the core's
 * own counts. A locked badge stays greyed out but keeps its real number, so
 * "7/13" is visible before it is earned.
 */
private fun LazyListScope.collectionBadges(book: CollectionUi) {
    item {
        Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) {
            Text(
                Copy.badgeSection,
                color = Color.White.copy(alpha = 0.55f),
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier
                    .padding(top = DuckTheme.Space.xs, bottom = DuckTheme.Space.xxs)
                    .testTag("collection-section-badges"),
            )
            CollectionBadges.evaluate(book).forEach { badge ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .background(Color.White.copy(alpha = 0.05f), RoundedCornerShape(DuckTheme.Radius.tiny))
                        .padding(horizontal = DuckTheme.Space.sm, vertical = DuckTheme.Space.xs)
                        .testTag("collection-badge-${badge.id}"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
                ) {
                    Text(
                        if (badge.unlocked) "✓" else "·",
                        color = if (badge.unlocked) {
                            DuckTheme.StateGood
                        } else {
                            Color.White.copy(alpha = 0.45f)
                        },
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = FontWeight.Bold,
                    )
                    Text(
                        badgeTitle(badge.id),
                        // Locked badges are greyed out, not hidden: the goal
                        // and its true progress stay readable.
                        color = if (badge.unlocked) Color.White else Color.White.copy(alpha = 0.5f),
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.weight(1f),
                    )
                    Text(
                        // The book's real number, capped at the goal for the
                        // cell only: 1001 moods still reads 1000/1000.
                        "${badge.progress.coerceIn(0, badge.target)}/${badge.target}",
                        color = if (badge.unlocked) {
                            DuckTheme.AccentData
                        } else {
                            Color.White.copy(alpha = 0.45f)
                        },
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.testTag("collection-badge-progress-${badge.id}"),
                    )
                }
            }
        }
    }
}

/** The badge's name from the one Copy dictionary; ids never reach the screen. */
private fun badgeTitle(id: String): String = when (id) {
    CollectionBadges.FIRST_MOOD -> Copy.badgeFirstMood
    CollectionBadges.ALL_MOODS -> Copy.badgeAllMoods
    CollectionBadges.FIRST_TRICK -> Copy.badgeFirstTrick
    CollectionBadges.ALL_TRICKS -> Copy.badgeAllTricks
    CollectionBadges.MOODS_10 -> Copy.badgeMoods10
    CollectionBadges.MOODS_100 -> Copy.badgeMoods100
    CollectionBadges.MOODS_1000 -> Copy.badgeMoods1000
    CollectionBadges.TRICKS_10 -> Copy.badgeTricks10
    CollectionBadges.TRICKS_100 -> Copy.badgeTricks100
    else -> id
}

private fun LazyListScope.collectionSection(
    title: String,
    id: String,
    entries: List<CollectionEntry>,
    onTry: (String) -> Unit,
) {
    item {
        Text(
            title,
            color = Color.White.copy(alpha = 0.55f),
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier
                .padding(top = DuckTheme.Space.xs, bottom = DuckTheme.Space.xxs)
                .testTag("collection-section-$id"),
        )
    }
    items(entries, key = { it.name }) { entry ->
        val row = collectionRow(entry)
        Row(
            Modifier
                .fillMaxWidth()
                .background(Color.White.copy(alpha = 0.05f), RoundedCornerShape(DuckTheme.Radius.tiny))
                .clickable(onClickLabel = skillLabel(row.name)) { onTry(row.name) }
                .padding(horizontal = DuckTheme.Space.sm, vertical = DuckTheme.Space.sm)
                .testTag("collection-item-${row.name}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
        ) {
            Text(
                if (row.locked) "·" else "✓",
                color = if (row.locked) Color.White.copy(alpha = 0.45f) else DuckTheme.StateGood,
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
            )
            Text(
                skillLabel(row.name),
                color = if (row.locked) Color.White.copy(alpha = 0.5f) else Color.White,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.weight(1f),
            )
            Column(horizontalAlignment = Alignment.End) {
                Text(
                    "× ${row.count}",
                    color = Color.White.copy(alpha = 0.65f),
                    style = MaterialTheme.typography.labelSmall,
                    fontFamily = FontFamily.Monospace,
                )
                // A locked row has no first day; only an unlocked row gets
                // the date line (or — when the day is unknown).
                row.firstDay?.let { day ->
                    Text(
                        "${Copy.collectionFirst} $day",
                        color = Color.White.copy(alpha = 0.4f),
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
            if (row.locked) {
                Text(
                    Copy.collectionLocked,
                    color = Color.White.copy(alpha = 0.45f),
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }
    }
}
