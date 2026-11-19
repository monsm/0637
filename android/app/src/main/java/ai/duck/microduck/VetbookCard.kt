package ai.duck.microduck

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

/**
 * The vet book as one shareable card: the duck's real numbers, drawn on
 * this phone (boss ruling 2026-11-17 — the card is generated locally and
 * never posted anywhere; Share hands the PNG to the system chooser).
 *
 * Data comes from the core's own `GET /export/vetbook.json` (core
 * `export.rs`): `{ format, build.fingerprint, care.summary(), life,
 * highlights{…} }`. The mapping is pure and unit-tested
 * (VetbookCardTest.kt); the dialog only renders what the parser kept.
 * The mapping mirrors the iOS ticket's `VetbookCardParser` field for field,
 * so the two phones draw the same card from the same document.
 */

/**
 * The card's numbers, parsed from the core's document. A field the
 * document does not carry stays null — an absent fact must not become a 0
 * on the card (the zero-fabrication rule REPORT.md lives by). The parser
 * fills the fields it finds, so the properties are vars; nothing mutates
 * them after parsing.
 */
data class VetbookCardModel(
    var metres: Double? = null,
    var rolls: Int? = null,
    var pickups: Int? = null,
    var falls: Int? = null,
    var pets: Int? = null,
    var fed: Int? = null,
    var claps: Int? = null,
    var whistles: Int? = null,
    /** `life.powered_s`, in hours. */
    var poweredHours: Double? = null,
    /** The three care percentages the core's own Markdown page derives. */
    var fullness: Int? = null,
    var mood: Int? = null,
    var bond: Int? = null,
    /** The build's provenance hash, cut to the first eight digits. */
    var fingerprint: String? = null,
) {
    /**
     * A life with nothing in it — every counter at zero and no walk —
     * shows the honest "no records yet" line instead of a grid of zeroes
     * that would claim things happened. Half a centimetre of drift is not
     * a walk either. Pure, so the JVM tests pin the honesty rule.
     */
    val isEmpty: Boolean
        get() {
            val anyCount = (rolls ?: 0) > 0 || (pickups ?: 0) > 0 || (falls ?: 0) > 0 ||
                (pets ?: 0) > 0 || (fed ?: 0) > 0 || (claps ?: 0) > 0 || (whistles ?: 0) > 0
            return !anyCount && (metres ?: 0.0) < 0.05
        }
}

/**
 * The one place the wire document becomes the card's model. Pure: the
 * same JSON gives the same card, and nothing here touches a view, so the
 * tests can pin the mapping without a core running. A field the document
 * does not carry stays null; a real zero the document does carry shows
 * as 0.
 */
fun parseVetbookCard(text: String): VetbookCardModel? = try {
    val root = org.json.JSONObject(text)
    val model = VetbookCardModel()
    val highlights = root.optJSONObject("highlights")
    if (highlights != null) {
        if (highlights.has("metres")) model.metres = highlights.optDouble("metres", 0.0)
        if (highlights.has("rolls")) model.rolls = highlights.optInt("rolls", 0)
        if (highlights.has("pickups")) model.pickups = highlights.optInt("pickups", 0)
        if (highlights.has("falls")) model.falls = highlights.optInt("falls", 0)
        if (highlights.has("pets")) model.pets = highlights.optInt("pets", 0)
        if (highlights.has("fed")) model.fed = highlights.optInt("fed", 0)
        if (highlights.has("claps")) model.claps = highlights.optInt("claps", 0)
        if (highlights.has("whistles")) model.whistles = highlights.optInt("whistles", 0)
    }
    val life = root.optJSONObject("life")
    if (life != null && life.has("powered_s")) {
        model.poweredHours = life.optDouble("powered_s", 0.0) / 3600.0
    }
    val care = root.optJSONObject("care")
    if (care != null) {
        // The same three percentages the core's own Markdown page derives:
        // fullness not hunger, mood rescaled from -1…1, bond as-is.
        // Clamped, so a bad reading cannot print 140%.
        if (care.has("hunger")) {
            model.fullness = vetbookPercent((1.0 - care.optDouble("hunger", 0.0)) * 100.0)
        }
        if (care.has("mood")) {
            model.mood = vetbookPercent(((care.optDouble("mood", 0.0) + 1.0) / 2.0) * 100.0)
        }
        if (care.has("bond")) {
            model.bond = vetbookPercent(care.optDouble("bond", 0.0) * 100.0)
        }
    }
    val build = root.optJSONObject("build")
    val raw = build?.optString("fingerprint", "") ?: ""
    if (!raw.isEmpty()) {
        model.fingerprint = raw.take(8)
    }
    model
} catch (_: Throwable) {
    null
}

/**
 * Clamp to 0…100 and round the way the iOS parser's `.rounded()` does:
 * ties away from zero. On the clamped non-negative range `roundToInt()`
 * (ties toward positive infinity) is exactly that — `kotlin.math.round`
 * would be rint's ties-to-even and drift from iOS on a half-percent.
 */
private fun vetbookPercent(value: Double): Int =
    value.coerceIn(0.0, 100.0).roundToInt()

/** Fetch the core's vet book export; null when the core is unreachable. */
fun fetchVetbookCard(): VetbookCardModel? = try {
    parseVetbookCard(getJson("/export/vetbook.json"))
} catch (_: Throwable) {
    // The intent server may not be up yet; no export is no card.
    null
}

/**
 * The card's tiles: one only for a number the document actually carried; a
 * real zero is a fact and shows as 0. In the life record's own order, the
 * same order the iOS grid runs. Pure, so the JVM tests pin which tiles
 * exist and which words they carry.
 */
internal fun vetbookTiles(model: VetbookCardModel): List<Pair<String, String>> = buildList {
    val metres = model.metres
    if (metres != null) add("${"%.1f".format(metres)} m" to Copy.walked)
    val rolls = model.rolls
    if (rolls != null) add("$rolls" to Copy.rolls)
    val pickups = model.pickups
    if (pickups != null) add("$pickups" to Copy.pickups)
    val falls = model.falls
    if (falls != null) add("$falls" to Copy.vetbookFalls)
    val pets = model.pets
    if (pets != null) add("$pets" to Copy.vetbookPets)
    val fed = model.fed
    if (fed != null) add("$fed" to Copy.vetbookFed)
    val claps = model.claps
    if (claps != null) add("$claps" to Copy.vetbookClaps)
    val whistles = model.whistles
    if (whistles != null) add("$whistles" to Copy.vetbookWhistles)
}

/**
 * Draw the card once, on one Canvas, into the bitmap both the preview and
 * the save/share actions use — what a person sees is exactly what leaves
 * the phone. The palette is the shared token table (bg.surface behind the
 * card, warm white on muted labels); the hours figure uses the same
 * monospace the instrument's percentages use.
 */
fun drawVetbookCard(model: VetbookCardModel, poweredLabel: String, noneLabel: String): Bitmap {
    val scale = 3
    val width = 390 * scale
    val height = 620 * scale
    val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bitmap)
    canvas.drawColor(DuckTheme.BgSurface.toArgb())
    canvas.scale(scale.toFloat(), scale.toFloat())

    val titlePaint = Paint().apply {
        color = DuckTheme.TextPrimary.toArgb()
        textSize = 22f
        isAntiAlias = true
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
    }
    val subtitlePaint = Paint().apply {
        color = (DuckTheme.TextPrimary.copy(alpha = 0.6f)).toArgb()
        textSize = 13f
        isAntiAlias = true
    }
    val numberPaint = Paint().apply {
        color = DuckTheme.TextPrimary.toArgb()
        textSize = 17f
        isAntiAlias = true
    }
    val labelPaint = Paint().apply {
        color = (DuckTheme.TextPrimary.copy(alpha = 0.6f)).toArgb()
        textSize = 10f
        isAntiAlias = true
    }
    val monoPaint = Paint().apply {
        color = (DuckTheme.TextPrimary.copy(alpha = 0.45f)).toArgb()
        textSize = 15f
        isAntiAlias = true
        typeface = Typeface.MONOSPACE
    }
    val monoTiny = Paint().apply {
        color = (DuckTheme.TextPrimary.copy(alpha = 0.45f)).toArgb()
        textSize = 9f
        isAntiAlias = true
        typeface = Typeface.MONOSPACE
    }

    canvas.drawText(Copy.name, 20f, 42f, titlePaint)
    canvas.drawText(Copy.lifeRecord, 300f, 42f, subtitlePaint)

    if (model.isEmpty) {
        canvas.drawText(noneLabel, 20f, 140f, subtitlePaint)
    } else {
        // The highlights, four to a row, each tile on the white-on-10%
        // surface the LifeStat tile uses.
        val tiles = vetbookTiles(model)
        var x = 20f
        var y = 96f
        for ((value, label) in tiles) {
            val tileW = 80f
            if (x + tileW > width / scale) {
                x = 20f
                y += 62f
            }
            canvas.drawRoundRect(
                x, y - 34f, x + tileW, y + 22f, 12f, 12f,
                Paint().apply { color = vetbookTileSurface() },
            )
            canvas.drawText(value, x + 8f, y - 8f, numberPaint)
            canvas.drawText(label, x + 8f, y + 12f, labelPaint)
            x += tileW + 10f
        }
        // The care row: the three percentages, three to a row.
        var cx = 20f
        val careY = y + 88f
        for (pair in listOf(
            model.fullness?.let { "${it}%" to Copy.fullness },
            model.mood?.let { "${it}%" to Copy.mood },
            model.bond?.let { "${it}%" to Copy.bond },
        )) {
            if (pair != null) {
                val (value, label) = pair
                val tileW = careTileWidth(width.toFloat() / scale)
                canvas.drawRoundRect(
                    cx, careY - 34f, cx + tileW, careY + 22f, 12f, 12f,
                    Paint().apply { color = vetbookTileSurface() },
                )
                canvas.drawText(value, cx + 8f, careY - 8f, numberPaint)
                canvas.drawText(label, cx + 8f, careY + 12f, labelPaint)
            }
            cx += careTileWidth(width.toFloat() / scale)
        }
        // The hours together, in the powered row the iOS card shows.
        val hours = model.poweredHours
        if (hours != null) {
            val hoursY = careY + 56f
            canvas.drawText(poweredLabel, 20f, hoursY, subtitlePaint)
            canvas.drawText("%.2f h".format(hours), 300f, hoursY, monoPaint)
        }
    }

    // The build's first eight digits, exactly the iOS footer's words.
    val build = model.fingerprint
    if (build != null) {
        canvas.drawText("build $build", 20f, height / scale - 14f, monoTiny)
    }
    return bitmap
}

/** One card tile's background: white at 10%, the LifeStat surface. */
private fun vetbookTileSurface(): Int = android.graphics.Color.argb(26, 255, 255, 255)

private fun careTileWidth(designWidth: Float): Float = (designWidth - 60f) / 3f

/** The card dialog: one fetch on IO, then the bitmap all three surfaces use. */
@Composable
internal fun VetbookCardDialog(onClose: () -> Unit) {
    val context = LocalContext.current
    var card by remember { mutableStateOf<Bitmap?>(null) }
    var failed by remember { mutableStateOf(false) }
    var saved by remember { mutableStateOf(false) }
    var saveFailed by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        val model = withContext(Dispatchers.IO) { fetchVetbookCard() }
        if (model == null) {
            failed = true
            return@LaunchedEffect
        }
        card = drawVetbookCard(model, Copy.vetbookCardPowered, Copy.vetbookCardNone)
    }
    Dialog(onDismissRequest = onClose) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(DuckTheme.BgRoot, RoundedCornerShape(DuckTheme.Radius.card))
                .padding(DuckTheme.Space.lg),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.vetbookCardShare,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = DuckTheme.Type.Body,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("vetbook-title"),
                )
                CloseChip("vetbook-close", onClose)
            }
            when {
                card != null -> {
                    Image(
                        bitmap = card!!.asImageBitmap(),
                        contentDescription = Copy.vetbookCardShare,
                        modifier = Modifier
                            .fillMaxWidth()
                            .testTag("vetbook-preview"),
                        contentScale = ContentScale.Fit,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
                        androidx.compose.material3.Button(
                            onClick = {
                                val bitmap = card ?: return@Button
                                val ok = savePhoto(context, bitmap)
                                saved = ok
                                saveFailed = !ok
                            },
                            enabled = !saved,
                            modifier = Modifier
                                .weight(1f)
                                .testTag("vetbook-save"),
                        ) {
                            Text(
                                if (saved) Copy.saved else if (saveFailed) Copy.saveFailed else Copy.savePhoto,
                                fontSize = DuckTheme.Type.Label,
                                maxLines = 1,
                            )
                        }
                        androidx.compose.material3.Button(
                            onClick = {
                                val bitmap = card ?: return@Button
                                sharePhoto(context, bitmap)
                            },
                            modifier = Modifier
                                .weight(1f)
                                .testTag("vetbook-share"),
                        ) {
                            Text(Copy.share, fontSize = DuckTheme.Type.Label, maxLines = 1)
                        }
                    }
                }
                failed -> Text(
                    Copy.rulesNotReady,
                    color = Color.White.copy(alpha = 0.6f),
                    fontSize = DuckTheme.Type.Caption,
                    modifier = Modifier.testTag("vetbook-failed"),
                )
            }
        }
    }
}
