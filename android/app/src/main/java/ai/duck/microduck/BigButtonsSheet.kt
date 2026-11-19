package ai.duck.microduck

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Four verbs, 72 dp tall, one page. The big-button page is for a
 * grandparent (or a three-year-old): no drawer, no ring, no tiny chips —
 * the whole duck's day is a stroke, a treat, a voice and a sit. The verbs
 * go through the same intents the small buttons use, so the duck cannot
 * tell the difference. TalkBack reads the whole action label; the words
 * are already large, so nothing needs scaling up.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BigButtonsSheet(
    onPet: () -> Unit,
    onFeed: () -> Unit,
    onQuack: () -> Unit,
    onSit: () -> Unit,
    onClose: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheetState) {
        Column(
            Modifier.padding(DuckTheme.Space.xl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    Copy.bigButtons,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = DuckTheme.Type.Title,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("big-title"),
                )
                CloseChip("big-close", onClose)
            }
            Text(
                Copy.bigButtonsNote,
                color = Color.White.copy(alpha = 0.6f),
                fontSize = DuckTheme.Type.Label,
            )
            bigButton(0, Icons.pet) { onPet() }
            bigButton(1, Icons.feed) { onFeed() }
            bigButton(2, Icons.quack) { onQuack() }
            bigButton(3, Icons.sit) { onSit() }
            Spacer(Modifier.size(8.dp))
        }
    }
}

@Composable
private fun bigButton(
    index: Int,
    icon: ImageVector,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(DuckTheme.Radius.control))
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClickLabel = Copy.bigButtonAction(index),
                role = Role.Button,
            ) { onClick() }
            .padding(horizontal = DuckTheme.Space.lg)
            .testTag("big-action-$index"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint = Color.White.copy(alpha = 0.85f),
            modifier = Modifier.size(26.dp),
        )
        Spacer(Modifier.width(14.dp))
        Text(
            Copy.bigButtonAction(index),
            color = Color.White,
            fontWeight = FontWeight.SemiBold,
            fontSize = DuckTheme.Type.Title,
        )
    }
}
