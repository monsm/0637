package ai.duck.microduck

import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * 日子 (K3, 2026-11-19): the parent's board of the duck's real days.
 *
 * A tools-page row opens it as a local sheet — the route table is pinned by
 * scripts/check-routes.py, so this is the same local-replacement pattern the
 * duck panel's words page uses (MorePages aliasOpen), not a new route case.
 *
 * Every number on the page is the core's own from `GET /days`: the finished
 * days, today's running row, and the week's sums. The contract is "missing
 * key = 0" — an older save may not carry every counter and reads as a real
 * zero; missing ROWS are never invented, a day the core did not finish never
 * becomes a row, and an absent today shows no today row. The week is the
 * core's aggregate read verbatim, never recomputed from the days.
 */

/** One finished day of the duck's life, exactly as `GET /days` publishes it.
 *  `day` is a verbatim "2026-11-18" date; the week block reuses the counters
 *  with an empty day. */
internal data class DaysDay(
    val day: String,
    val metres: Double,
    val poweredS: Double,
    val rolls: Int,
    val pets: Int,
    val pickups: Int,
    val falls: Int,
    val fed: Int,
    val claps: Int,
    val whistles: Int,
    val groundPicks: Int,
    val sitDowns: Int,
)

/** The parsed `GET /days` document: finished days, today's running row (null
 *  when the core sent none) and the week's true totals (null when omitted). */
internal data class DaysBoard(
    val days: List<DaysDay>,
    val today: DaysDay?,
    val week: DaysDay?,
) {
    /**
     * Empty when the core sent no finished day and no today: the page says so
     * in one sentence and shows no zero table. A today-only board is NOT
     * empty — it shows the running day, with no week card behind it (today is
     * not a finished day, so it must not invent a weekly total).
     */
    val isEmpty: Boolean get() = days.isEmpty() && today == null
}

/** The counters shared by a finished day, today's row and the week block.
 *  Every field defaults to 0: the contract sends a counter only when it has
 *  a number, and missing means zero for an older save. */
private fun parseDaysStats(row: JSONObject): DaysDay = DaysDay(
    day = "",
    metres = row.optDouble("metres", 0.0),
    poweredS = row.optDouble("powered_s", 0.0),
    rolls = row.optInt("rolls", 0),
    pets = row.optInt("pets", 0),
    pickups = row.optInt("pickups", 0),
    falls = row.optInt("falls", 0),
    fed = row.optInt("fed", 0),
    claps = row.optInt("claps", 0),
    whistles = row.optInt("whistles", 0),
    groundPicks = row.optInt("ground_picks", 0),
    sitDowns = row.optInt("sit_downs", 0),
)

/**
 * One day row. The day string is kept verbatim — the board never invents,
 * renames or normalizes a date the core did not send; a row whose `day` is
 * missing, blank or not a string is dropped rather than given a made-up date.
 */
internal fun parseDaysRow(row: JSONObject): DaysDay? {
    val raw = row.opt("day")
    val day = if (raw is String) raw.trim() else ""
    if (day.isEmpty()) return null
    return parseDaysStats(row).copy(day = day)
}

/**
 * The week block: counters only, no `day` — it is the core's own aggregate
 * ("这一周"合计卡), read verbatim. This side never recomputes it from the
 * `days` rows, even when the daily sum disagrees: the core is the source.
 * A missing `week` block means no week card at all — the same handling as
 * the iOS sheet (both read the core's block directly).
 */
internal fun parseDaysWeek(row: JSONObject): DaysDay = parseDaysStats(row)

/**
 * The K1 contract:
 * `{"ok":true,"days":[...],"today":{...},"week":{...}}`.
 * Only `days` is required; `today` and `week` may be absent. Broken JSON, an
 * explicit refusal (`ok:false`) and a document without a `days` array all
 * read as null, so the sheet can stay honest instead of drawing a board the
 * core never sent.
 */
internal fun parseDaysBoard(text: String): DaysBoard? = try {
    val root = JSONObject(text)
    if (root.optBoolean("ok", true) == false) {
        null
    } else {
        val array = root.optJSONArray("days") ?: return null
        val days = (0 until array.length()).mapNotNull { index ->
            array.optJSONObject(index)?.let { parseDaysRow(it) }
        }
        DaysBoard(
            days = days,
            today = root.optJSONObject("today")?.let { parseDaysRow(it) },
            week = root.optJSONObject("week")?.let { parseDaysWeek(it) },
        )
    }
} catch (_: Throwable) {
    null
}

internal fun fetchDaysBoard(): DaysBoard? = try {
    parseDaysBoard(getJson("/days"))
} catch (_: Throwable) {
    // The intent server may not be up yet; no board is no board.
    null
}

/** Powered seconds in hours, the iOS figure's own formatting: `%.1f h`. */
internal fun daysPoweredText(seconds: Double): String = "%.1f h".format(seconds / 3600.0)

/** Metres in the iOS figure's own formatting: `%.1f m`. */
internal fun daysMetresText(metres: Double): String = "%.1f m".format(metres)

/**
 * The nine label/value lines, in the order the page and the shared document
 * show them (陪伴小时、米数、翻滚、被摸、被抱起、摔倒、被喂、拍手、口哨) — the
 * same order and words the iOS sheet uses. Labels are Copy's dictionary
 * words; values are the core's own numbers.
 */
internal fun daysLines(day: DaysDay): List<Pair<String, String>> = listOf(
    Copy.daysPowered to daysPoweredText(day.poweredS),
    Copy.daysMetres to daysMetresText(day.metres),
    Copy.daysRolls to "${day.rolls}",
    Copy.daysPets to "${day.pets}",
    Copy.daysPickups to "${day.pickups}",
    Copy.daysFalls to "${day.falls}",
    Copy.daysFed to "${day.fed}",
    Copy.daysClaps to "${day.claps}",
    Copy.daysWhistles to "${day.whistles}",
)

/** The board's export in one language: the week block, then the today row
 *  (Today (date)), then one block per finished day. A line exists only when
 *  the underlying fact does. */
private fun daysShareLines(board: DaysBoard): List<String> = buildList {
    board.week?.let { week ->
        add(Copy.daysWeek)
        daysLines(week).forEach { (label, value) -> add("$label: $value") }
    }
    board.today?.let { today ->
        add("${Copy.daysToday} (${today.day})")
        daysLines(today).forEach { (label, value) -> add("$label: $value") }
    }
    board.days.forEach { day ->
        add(day.day)
        daysLines(day).forEach { (label, value) -> add("$label: $value") }
    }
}

/**
 * The whole board as one bilingual plain-text document: each line carries the
 * English and the Chinese words side by side with the core's own numbers, so
 * the person on the other end of a share reads the facts in either language.
 * Nothing is invented; a date line is a real date the core sent. The labels
 * flip `Copy.language` briefly so both words come from the one dictionary,
 * and the previous language is restored before returning.
 */
internal fun daysShareText(board: DaysBoard): String {
    val saved = Copy.language
    return try {
        Copy.language = Copy.Language.English
        val en = daysShareLines(board)
        Copy.language = Copy.Language.Chinese
        val zh = daysShareLines(board)
        // The structure is language-independent, so the two renders align
        // line for line; a date-only line is identical in both languages.
        en.zip(zh).joinToString("\n") { (english, chinese) ->
            if (english == chinese) english else "$english / $chinese"
        }
    } finally {
        Copy.language = saved
    }
}

/** 日子: the parent's board of the duck's real days. The week's true totals
 *  (when the core sent them), then every finished day, today marked 今天. A
 *  core that did not answer says so (aiOffline); only a real answer with no
 *  days and no today says "no full days yet" — and it shows no zeros. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DaysSheet(onClose: () -> Unit) {
    val context = LocalContext.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var board by remember { mutableStateOf<DaysBoard?>(null) }
    // Three honest surfaces, not one: still loading (nothing yet — the first
    // frame never flashes a fake "no full days yet"), no answer from the core
    // (aiOffline — unreachable, timeout, or a body that is not a days board),
    // and a real answer whose days and today are both empty (daysEmpty).
    var loaded by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        board = withContext(Dispatchers.IO) { fetchDaysBoard() }
        loaded = true
    }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = ShellSurface,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(DuckTheme.Space.xl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.daysTitle,
                    color = Color.White,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("days-title").duckSheetFirstFocus(),
                )
                CloseChip("days-close", onClose)
            }
            val data = board
            when {
                // The fetch is still in flight: draw nothing rather than flash
                // a fake "no full days yet" on the first frame.
                !loaded -> Unit
                data == null -> Text(
                    // The core did not answer /days (unreachable, timeout, or a
                    // body that is not a days board). Say so in the same honest
                    // words the collection page uses — never dress an
                    // unanswered fetch as an empty day record.
                    Copy.aiOffline,
                    color = Color.White.copy(alpha = 0.6f),
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.testTag("days-offline"),
                )
                data.isEmpty -> DuckEmptyState(
                    // A real answer with no finished day and no today: one
                    // true sentence, one real way out.
                    title = Copy.daysEmpty,
                    actionTitle = Copy.emptyPlayAction,
                    onAction = onClose,
                    testTag = "days-empty",
                )
                else -> LazyColumn(
                    modifier = Modifier.heightIn(max = 460.dp),
                    verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs),
                ) {
                    data.week?.let { week ->
                        item { DaysWeekCard(week) }
                    }
                    data.today?.let { today ->
                        item { DaysDayCard(today, title = Copy.daysToday) }
                    }
                    items(data.days) { day ->
                        DaysDayCard(day, title = day.day)
                    }
                    item {
                        ButtonChip(Copy.daysShare, "days-share") {
                            val send = Intent(Intent.ACTION_SEND)
                                .setType("text/plain")
                                .putExtra(Intent.EXTRA_TEXT, daysShareText(data))
                            context.startActivity(Intent.createChooser(send, Copy.daysShare))
                        }
                    }
                }
            }
        }
    }
}

/** 这一周: the core's own sums, one card. A card exists only when the core
 *  sent a week block; the numbers are its verbatim totals. */
@Composable
private fun DaysWeekCard(week: DaysDay) {
    Column(
        Modifier
            .fillMaxWidth()
            .background(Color.White.copy(alpha = 0.04f), RoundedCornerShape(DuckTheme.Radius.control))
            .padding(DuckTheme.Space.md)
            .testTag("days-week"),
        verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
    ) {
        Text(
            Copy.daysWeek,
            color = Color.White,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.SemiBold,
        )
        daysLines(week).forEach { (label, value) ->
            DaysStatLine(label, value)
        }
    }
}

/** One day: the date (or the Today word for the core's running row), then the
 *  same number lines the share text carries. A row exists only when the core
 *  sent it; the shell never lays out a zero day. */
@Composable
private fun DaysDayCard(day: DaysDay, title: String) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(vertical = DuckTheme.Space.sm)
            .testTag("days-row-${day.day}"),
        verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs),
    ) {
        Text(
            title,
            color = Color.White,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
        daysLines(day).forEach { (label, value) ->
            DaysStatLine(label, value)
        }
    }
}

/** One "label value" line, left and right like the iOS card's HStack. */
@Composable
private fun DaysStatLine(label: String, value: String) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            label,
            color = Color.White.copy(alpha = 0.6f),
            style = MaterialTheme.typography.labelSmall,
        )
        Text(
            value,
            color = Color.White.copy(alpha = 0.85f),
            style = MaterialTheme.typography.labelSmall,
            fontFamily = FontFamily.Monospace,
        )
    }
}