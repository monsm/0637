@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * Direction A (design-demos/A-complete.html): the Live screen's verbs are
 * objects on the duck's bedside shelf. The board itself is the cabinet pull
 * (More); the wall carries a long wooden clock (time machine) and a brass
 * gauge (readings). Seven controls, the same budget the shipped shell had.
 */

@Composable
private fun ShelfObject(
    label: String,
    tag: String,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
    visual: @Composable () -> Unit,
) {
    Column(
        modifier
            .width(64.dp)
            .combinedClickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                role = Role.Button,
                onClickLabel = label,
                onClick = onClick,
            )
            .testTag(tag)
            .padding(vertical = DuckTheme.Space.xs),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs),
    ) {
        Box(Modifier.size(width = 54.dp, height = 54.dp), contentAlignment = Alignment.BottomCenter) {
            visual()
        }
        Text(
            label,
            color = DuckTheme.TextPrimary.copy(alpha = 0.85f),
            fontSize = DuckTheme.Type.Caption,
            maxLines = 2,
        )
    }
}

@Composable
private fun Bowl(hunger: Double) {
    Box(
        Modifier
            .width(52.dp)
            .height(30.dp)
            .background(
                Brush.verticalGradient(listOf(DuckTheme.Ceramic, DuckTheme.Ceramic.copy(alpha = 0.72f))),
                RoundedCornerShape(DuckTheme.Radius.control),
            ),
    ) {
        Box(
            Modifier
                .padding(horizontal = DuckTheme.Space.xs, vertical = DuckTheme.Space.xxs)
                .fillMaxWidth()
                .height((3 + 9 * hunger.coerceIn(0.0, 1.0)).dp)
                .background(DuckTheme.Food, CircleShape),
        )
    }
}

@Composable
private fun CardBox() {
    Box(Modifier.size(44.dp)) {
        listOf(-7, 4, 0).forEach { degrees ->
            Box(
                Modifier
                    .align(Alignment.BottomCenter)
                    .size(width = 30.dp, height = 44.dp)
                    .rotate(degrees.toFloat())
                    .background(
                        Brush.verticalGradient(listOf(DuckTheme.WoodHi, DuckTheme.WoodDeep)),
                        RoundedCornerShape(DuckTheme.Radius.tiny),
                    ),
            )
        }
        Box(
            Modifier
                .align(Alignment.Center)
                .size(7.dp)
                .rotate(45f)
                .background(DuckTheme.AccentAction),
        )
    }
}

@Composable
private fun Bell() {
    Box(Modifier.size(width = 40.dp, height = 38.dp), contentAlignment = Alignment.BottomCenter) {
        Box(
            Modifier
                .size(width = 36.dp, height = 32.dp)
                .background(
                    Brush.linearGradient(listOf(DuckTheme.Brass, DuckTheme.BrassDeep)),
                    RoundedCornerShape(topStart = DuckTheme.Radius.card, topEnd = DuckTheme.Radius.card, bottomEnd = DuckTheme.Radius.tiny, bottomStart = DuckTheme.Radius.tiny),
                ),
        )
        Box(
            Modifier
                .size(9.dp)
                .background(DuckTheme.BrassDeep, CircleShape),
        )
    }
}

@Composable
private fun Hand(held: Boolean) {
    Row(
        Modifier
            .size(width = 42.dp, height = 48.dp)
            .rotate(if (held) -8f else 0f),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs),
    ) {
        listOf(16, 20, 18, 13).forEach { h ->
            Box(
                Modifier
                    .width(6.dp)
                    .height(h.dp)
                    .background(DuckTheme.Ceramic, CircleShape),
            )
        }
    }
}

@Composable
fun DuckShelf(
    hunger: Double,
    carrying: Boolean,
    onFeed: () -> Unit,
    onSkills: () -> Unit,
    onQuack: () -> Unit,
    onGrab: () -> Unit,
    onMore: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(modifier) {
        // The board is the cabinet pull: its label is the More verb.
        Box(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 118.dp)
                .combinedClickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    role = Role.Button,
                    onClickLabel = Copy.more,
                    onClick = onMore,
                )
                .testTag("duck-more")
                .background(
                    Brush.verticalGradient(listOf(DuckTheme.WoodHi, DuckTheme.Wood, DuckTheme.WoodDeep)),
                    RoundedCornerShape(DuckTheme.Radius.panel),
                ),
            contentAlignment = Alignment.BottomCenter,
        ) {
            // The cabinet pull carries its own word: discoverable, not a bare
            // board of dots.
            Column(
                Modifier.padding(bottom = DuckTheme.Space.sm),
                verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xxs),
            ) {
                Text(Copy.more, color = DuckTheme.TextPrimary.copy(alpha = 0.55f), fontSize = DuckTheme.Type.Micro)
                Row(horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm)) {
                    repeat(3) {
                        Box(Modifier.size(3.dp).background(DuckTheme.TextPrimary.copy(alpha = 0.4f), CircleShape))
                    }
                }
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 118.dp)
                .padding(horizontal = DuckTheme.Space.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceEvenly,
        ) {
            ShelfObject(Copy.feed, "duck-feed", onClick = onFeed) { Bowl(hunger) }
            ShelfObject(Copy.skills, "duck-skills", onClick = onSkills) { CardBox() }
            ShelfObject(Copy.quack, "duck-quack", onClick = onQuack) { Bell() }
            ShelfObject(
                if (carrying) Copy.putDown else Copy.grab,
                "duck-grab",
                onClick = onGrab,
            ) { Hand(carrying) }
        }
    }
}

@Composable
private fun WallObject(
    label: String,
    tag: String,
    modifier: Modifier = Modifier,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    Box(
        modifier
            .combinedClickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                role = Role.Button,
                onClickLabel = label,
                onLongClickLabel = if (onLongClick != null) Copy.freeze else null,
                onClick = onClick,
                onLongClick = onLongClick,
            )
            .testTag(tag)
            ,
    ) {
        content()
    }
}

/// The long wooden clock: tap = time machine, hold = freeze.
@Composable
fun DuckWallClock(
    onOpen: () -> Unit,
    onHold: () -> Unit,
    modifier: Modifier = Modifier,
) {
    WallObject(Copy.time, "duck-time", modifier, onLongClick = onHold, onClick = onOpen) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Box(
                Modifier
                    .width(30.dp)
                    .height(64.dp)
                    .background(
                        Brush.verticalGradient(listOf(DuckTheme.WoodHi, DuckTheme.Wood, DuckTheme.WoodDeep)),
                        RoundedCornerShape(DuckTheme.Radius.tiny),
                    ),
            ) {
                Column(
                    Modifier
                        .align(Alignment.Center)
                        .width(18.dp)
                        .height(46.dp)
                        .background(Color.Black.copy(alpha = 0.45f), RoundedCornerShape(DuckTheme.Radius.tiny)),
                    verticalArrangement = Arrangement.SpaceEvenly,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    repeat(6) {
                        Box(Modifier.width(14.dp).height(1.dp).background(DuckTheme.TextPrimary.copy(alpha = 0.28f)))
                    }
                }
                Box(
                    Modifier
                        .align(Alignment.TopCenter)
                        .offset(y = 30.dp)
                        .width(24.dp)
                        .height(3.dp)
                        .background(DuckTheme.AccentAction, RoundedCornerShape(DuckTheme.Radius.tiny)),
                )
                Box(
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(bottom = DuckTheme.Space.xs)
                        .size(7.dp)
                        .background(DuckTheme.Brass, CircleShape),
                )
            }
            Text(Copy.wallClock, color = DuckTheme.TextPrimary.copy(alpha = 0.5f), fontSize = DuckTheme.Type.Micro)
        }
    }
}

/// The brass gauge: the readings surface.
@Composable
fun DuckWallGauge(onOpen: () -> Unit, modifier: Modifier = Modifier) {
    WallObject(Copy.readings, "duck-readings", modifier, onClick = onOpen) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Box(
                Modifier
                    .size(44.dp)
                    .background(
                        Brush.radialGradient(listOf(DuckTheme.WoodDeep, Color.Black.copy(alpha = 0.9f))),
                        CircleShape,
                    ),
                contentAlignment = Alignment.Center,
            ) {
                // Two needles: brass for pressure, amber for the room.
                Box(Modifier.width(1.5.dp).height(13.dp).rotate(24f).background(DuckTheme.Brass))
                Box(Modifier.width(1.dp).height(10.dp).rotate(-38f).background(DuckTheme.AccentAction.copy(alpha = 0.85f)))
                Box(Modifier.size(4.dp).background(DuckTheme.Brass, CircleShape))
            }
            Text(Copy.readings, color = DuckTheme.TextPrimary.copy(alpha = 0.5f), fontSize = DuckTheme.Type.Micro)
        }
    }
}
