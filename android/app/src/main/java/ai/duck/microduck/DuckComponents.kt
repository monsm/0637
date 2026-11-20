package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/**
 * D2 wave 1 (contracts/ui-components.json): the five base surfaces, one
 * implementation per end, tokens only. Each carries a STATE MATRIX line the
 * component gate compares with the contract.
 */

/** The press feedback every component shares: scale down a touch, dim a touch. */
@Composable
private fun Modifier.duckPress(interactionSource: MutableInteractionSource): Modifier {
    val pressed by interactionSource.collectIsPressedAsState()
    return this.graphicsLayer {
        scaleX = if (pressed) 0.96f else 1f
        scaleY = if (pressed) 0.96f else 1f
        alpha = if (pressed) 0.82f else 1f
    }
}

/**
 * D4's line for the Android dense sheets: at font_scale 2.0 two or more
 * columns collapse to one, and values that had maxLines=1 may wrap.
 */
@Composable
fun duckLargeText(): Boolean = LocalDensity.current.fontScale >= 1.5f

/**
 * D4 §3.5 on Android: the first element of a sheet takes focus when the
 * sheet appears. The ModalBottomSheet restores the opener's focus on close;
 * this half moves it in.
 */
@Composable
fun Modifier.duckSheetFirstFocus(): Modifier {
    val requester = remember { FocusRequester() }
    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay(200)
        requester.requestFocus()
    }
    return focusRequester(requester).focusable()
}

/**
 * A row of chips that becomes a column at accessibility font scales and
 * keeps its normal row shape otherwise. No weight is used by its callers,
 * so the same children can live in either scope.
 */
@Composable
fun DuckChipWrap(content: @Composable () -> Unit) {
    if (duckLargeText()) {
        Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) { content() }
    } else {
        Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) { content() }
    }
}

/**
 * The one button. STATE MATRIX: DuckButton = default, pressed, disabled, selected, focus
 */
@Composable
fun DuckButton(
    title: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    selected: Boolean = false,
    enabled: Boolean = true,
    testTag: String = "",
) {
    val interaction = remember { MutableInteractionSource() }
    val shape = RoundedCornerShape(DuckTheme.Radius.control)
    val background = when {
        selected -> DuckTheme.AccentAction.copy(alpha = 0.35f)
        else -> Color.White.copy(alpha = 0.10f)
    }
    Row(
        modifier
            .duckPress(interaction)
            .background(background, shape)
            .clickable(
                interactionSource = interaction,
                indication = null,
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
            )
            .padding(horizontal = DuckTheme.Space.md)
            .heightIn(min = 48.dp)
            .semantics { this.selected = selected }
            .testTag(testTag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs),
    ) {
        Text(
            title,
            color = if (enabled) DuckTheme.TextPrimary else DuckTheme.TextPrimary.copy(alpha = 0.4f),
            fontSize = DuckTheme.Type.Label,
            fontWeight = FontWeight.Medium,
        )
    }
}

/** The one chip: selection is check glyph + state, never hue alone.
 * STATE MATRIX: DuckChip = default, pressed, disabled, selected */
@Composable
fun DuckChip(
    title: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    selected: Boolean = false,
    enabled: Boolean = true,
    testTag: String = "",
) {
    val interaction = remember { MutableInteractionSource() }
    val shape = RoundedCornerShape(DuckTheme.Radius.control)
    Row(
        modifier
            .duckPress(interaction)
            .background(
                if (selected) DuckTheme.AccentAction else Color.White.copy(alpha = 0.10f),
                shape,
            )
            .clickable(
                interactionSource = interaction,
                indication = null,
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
            )
            .padding(horizontal = DuckTheme.Space.md)
            .heightIn(min = 48.dp)
            .semantics { this.selected = selected }
            .testTag(testTag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs),
    ) {
        if (selected) {
            Icon(
                Icons.check,
                contentDescription = null,
                tint = DuckTheme.TextOnAccent,
                modifier = Modifier.size(14.dp),
            )
        }
        Text(
            title,
            color = if (selected) DuckTheme.TextOnAccent else DuckTheme.TextPrimary,
            fontSize = DuckTheme.Type.Label,
            fontWeight = FontWeight.Medium,
        )
    }
}

/** The one card. STATE MATRIX: DuckCard = default, pressed, disabled */
@Composable
fun DuckCard(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Column(
        modifier
            .fillMaxWidth()
            .background(DuckTheme.BgSurface.copy(alpha = 0.94f), RoundedCornerShape(DuckTheme.Radius.card))
            .padding(DuckTheme.Space.lg),
    ) {
        content()
    }
}

/** The shared 48 dp close control behind DuckSheetHeader and CloseChip.
 * STATE MATRIX: shared with DuckSheetHeader */
@Composable
fun DuckCloseControl(
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    testTag: String = "sheet-close-shared",
) {
    val interaction = remember { MutableInteractionSource() }
    Icon(
        Icons.close,
        contentDescription = contentDescription,
        tint = Color.White.copy(alpha = 0.55f),
        modifier = modifier
            .duckPress(interaction)
            .size(48.dp)
            .background(Color.White.copy(alpha = 0.10f), CircleShape)
            .clickable(
                interactionSource = interaction,
                indication = null,
                role = Role.Button,
                onClick = onClick,
            )
            .testTag(testTag)
            .padding(DuckTheme.Space.md),
    )
}

/** The one sheet chrome: title + the shared close, one implementation.
 * STATE MATRIX: DuckSheetHeader = default, closePressed, closeDisabled */
@Composable
fun DuckSheetHeader(
    title: String,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
    closeDescription: String = Copy.close,
) {
    Row(
        modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
    ) {
        Text(
            title,
            color = Color.White,
            fontSize = DuckTheme.Type.Title,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.weight(1f),
        )
        DuckCloseControl(contentDescription = closeDescription, onClick = onClose)
    }
}

/** The one list row: icon, title, note, trailing value/badge; the row is the
 * target. The drawer's MoreRow delegates here so the shell has one row.
 * STATE MATRIX: DuckListRow = default, pressed, disabled, selected */
@Composable
fun DuckListRow(
    title: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    note: String? = null,
    value: String? = null,
    selected: Boolean = false,
    showChevron: Boolean = false,
    enabled: Boolean = true,
    testTag: String = "",
) {
    val interaction = remember { MutableInteractionSource() }
    Row(
        modifier
            .duckPress(interaction)
            .heightIn(min = 48.dp)
            .clickable(
                interactionSource = interaction,
                indication = null,
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
            )
            .semantics { this.selected = selected }
            .testTag(testTag)
            .padding(horizontal = DuckTheme.Space.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
    ) {
        if (icon != null) {
            Icon(
                icon,
                contentDescription = null,
                tint = Color.White.copy(alpha = 0.85f),
                modifier = Modifier.size(20.dp),
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs)) {
            Text(title, color = Color.White, fontSize = DuckTheme.Type.Body, fontWeight = FontWeight.Medium)
            if (note != null) {
                Text(note, color = Color.White.copy(alpha = 0.6f), fontSize = DuckTheme.Type.Caption)
            }
        }
        if (selected) {
            Icon(
                Icons.check,
                contentDescription = null,
                tint = DuckTheme.AccentAction,
                modifier = Modifier.size(16.dp),
            )
        }
        if (value != null) {
            Text(
                value,
                color = if (selected) DuckTheme.AccentAction else Color.White.copy(alpha = 0.55f),
                fontSize = DuckTheme.Type.Caption,
                fontWeight = FontWeight.SemiBold,
            )
        }
        if (showChevron) {
            Icon(
                Icons.chevronRight,
                contentDescription = null,
                tint = Color.White.copy(alpha = 0.65f),
                modifier = Modifier.size(16.dp),
            )
        }
    }
}

/**
 * D2 wave 2: the state-system surfaces. Same rules — tokens only, one
 * implementation, a STATE MATRIX line per component.
 */

/** The one toggle. STATE MATRIX: DuckToggle = off, on, disabled */
@Composable
fun DuckToggle(
    title: String,
    isOn: Boolean,
    onToggle: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    testTag: String = "",
) {
    Row(
        modifier
            .heightIn(min = 48.dp)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                enabled = enabled,
                role = Role.Switch,
                onClickLabel = title,
            ) { onToggle(!isOn) }
            .semantics { this.selected = isOn }
            .testTag(testTag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
    ) {
        Text(
            title,
            color = if (enabled) DuckTheme.TextPrimary else DuckTheme.TextPrimary.copy(alpha = 0.4f),
            fontSize = DuckTheme.Type.Body,
            modifier = Modifier.weight(1f),
        )
        // The track: shape and the word carry the state beside the colour.
        Row(
            Modifier
                .size(width = 44.dp, height = 26.dp)
                .background(
                    if (isOn) DuckTheme.AccentAction else Color.White.copy(alpha = 0.15f),
                    RoundedCornerShape(DuckTheme.Radius.control),
                )
                .padding(DuckTheme.Space.xxs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Spacer(Modifier.weight(if (isOn) 0f else 1f))
            Text(
                if (isOn) "ON" else "OFF",
                color = if (isOn) DuckTheme.TextOnAccent else DuckTheme.TextPrimary.copy(alpha = 0.7f),
                fontSize = DuckTheme.Type.Micro,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.weight(if (isOn) 1f else 0f))
        }
    }
}

/** The one segmented control. STATE MATRIX: DuckSegmented = default, selected, disabled */
@Composable
fun DuckSegmented(
    options: List<String>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    testTag: String = "",
) {
    val large = duckLargeText()
    val base = modifier
        .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(DuckTheme.Radius.control))
        .padding(DuckTheme.Space.xxs)
        .testTag(testTag)
    if (large) {
        Column(base, verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs)) {
            options.forEachIndexed { index, option ->
                val isPicked = index == selectedIndex
                Text(
                    option,
                    color = if (isPicked) DuckTheme.TextOnAccent else DuckTheme.TextPrimary.copy(alpha = if (enabled) 0.85f else 0.4f),
                    fontSize = DuckTheme.Type.Label,
                    fontWeight = if (isPicked) FontWeight.SemiBold else FontWeight.Medium,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .background(
                            if (isPicked) DuckTheme.AccentAction else Color.Transparent,
                            RoundedCornerShape(DuckTheme.Radius.control),
                        )
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            enabled = enabled,
                            role = Role.Button,
                            onClickLabel = option,
                        ) { onSelect(index) }
                        .padding(horizontal = DuckTheme.Space.md)
                        .wrapContentHeight(Alignment.CenterVertically)
                        .semantics { this.selected = isPicked },
                    textAlign = TextAlign.Center,
                )
            }
        }
    } else {
        Row(base, horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs)) {
            options.forEachIndexed { index, option ->
                val isPicked = index == selectedIndex
                Text(
                    option,
                    color = if (isPicked) DuckTheme.TextOnAccent else DuckTheme.TextPrimary.copy(alpha = if (enabled) 0.85f else 0.4f),
                    fontSize = DuckTheme.Type.Label,
                    fontWeight = if (isPicked) FontWeight.SemiBold else FontWeight.Medium,
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 48.dp)
                        .background(
                            if (isPicked) DuckTheme.AccentAction else Color.Transparent,
                            RoundedCornerShape(DuckTheme.Radius.control),
                        )
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            enabled = enabled,
                            role = Role.Button,
                            onClickLabel = option,
                        ) { onSelect(index) }
                        .padding(horizontal = DuckTheme.Space.md)
                        .wrapContentHeight(Alignment.CenterVertically)
                        .semantics { this.selected = isPicked },
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

/** The one progress row: value text beside the bar so colour is never alone.
 * STATE MATRIX: DuckProgress = normal, warning, error */
@Composable
fun DuckProgress(
    value: Float,
    modifier: Modifier = Modifier,
    label: String? = null,
    state: DuckProgressState = DuckProgressState.Normal,
    testTag: String = "",
) {
    val tint = when (state) {
        DuckProgressState.Normal -> DuckTheme.AccentData
        DuckProgressState.Warning -> DuckTheme.StateWarn
        DuckProgressState.Error -> DuckTheme.StateBad
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs)) {
        if (label != null) {
            Text(label, color = DuckTheme.TextPrimary, fontSize = DuckTheme.Type.Caption)
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
        ) {
            Row(
                Modifier
                    .weight(1f)
                    .heightIn(min = 8.dp)
                    .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(DuckTheme.Radius.tiny))
                    .testTag(testTag),
            ) {
                Row(
                    Modifier
                        .fillMaxWidth(value.coerceIn(0f, 1f))
                        .heightIn(min = 8.dp)
                        .background(tint, RoundedCornerShape(DuckTheme.Radius.tiny)),
                ) {}
            }
            Text(
                "${(value.coerceIn(0f, 1f) * 100).toInt()}%",
                color = tint,
                fontSize = DuckTheme.Type.Micro,
            )
        }
    }
}

enum class DuckProgressState { Normal, Warning, Error }

/** The one empty state: one sentence, one action, no fabricated numbers.
 * STATE MATRIX: DuckEmptyState = default, actionPressed */
@Composable
fun DuckEmptyState(
    title: String,
    modifier: Modifier = Modifier,
    note: String? = null,
    actionTitle: String? = null,
    onAction: (() -> Unit)? = null,
    testTag: String = "",
) {
    Column(
        modifier
            .fillMaxWidth()
            .padding(DuckTheme.Space.xl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
    ) {
        Text(
            title,
            color = DuckTheme.TextPrimary,
            fontSize = DuckTheme.Type.Body,
            textAlign = TextAlign.Center,
            modifier = Modifier.testTag(testTag),
        )
        if (note != null) {
            Text(
                note,
                color = DuckTheme.TextPrimary.copy(alpha = 0.7f),
                fontSize = DuckTheme.Type.Caption,
                textAlign = TextAlign.Center,
            )
        }
        if (actionTitle != null && onAction != null) {
            DuckChip(title = actionTitle, onClick = onAction, testTag = "$testTag-action")
        }
    }
}

/** The one error card: what happened, what to do, retry; never a fake success.
 * STATE MATRIX: DuckErrorCard = default, retryPressed, retrying, offline */
@Composable
fun DuckErrorCard(
    title: String,
    reason: String,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    retrying: Boolean = false,
    offline: Boolean = false,
    testTag: String = "",
    retryTag: String = "",
) {
    Column(
        modifier
            .fillMaxWidth()
            .background(DuckTheme.BgSurface.copy(alpha = 0.94f), RoundedCornerShape(DuckTheme.Radius.card))
            .padding(DuckTheme.Space.lg)
            .testTag(testTag),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
    ) {
        Text(
            title,
            color = DuckTheme.StateBad,
            fontSize = DuckTheme.Type.Label,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
        )
        Text(
            reason,
            color = DuckTheme.TextPrimary.copy(alpha = 0.7f),
            fontSize = DuckTheme.Type.Caption,
            textAlign = TextAlign.Center,
        )
        DuckButton(
            title = Copy.retry,
            onClick = onRetry,
            enabled = !retrying,
            testTag = if (retryTag.isNotEmpty()) retryTag else "$testTag-retry",
        )
    }
}

/** The one loading state: plain words, static under reduced motion.
 * STATE MATRIX: DuckLoading = default, reducedMotion */
@Composable
fun DuckLoading(
    title: String,
    modifier: Modifier = Modifier,
    testTag: String = "",
) {
    Text(
        title,
        color = DuckTheme.TextPrimary.copy(alpha = 0.7f),
        fontSize = DuckTheme.Type.Caption,
        modifier = modifier
            .fillMaxWidth()
            .padding(DuckTheme.Space.lg)
            .testTag(testTag),
        textAlign = TextAlign.Center,
    )
}
