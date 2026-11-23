package ai.duck.microduck

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.File

/**
 * The kept postcards: real Director frames with their honest captions.
 * Share hands the file to the chooser; nothing here leaves the phone until
 * the user picks a destination.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PostcardsSheet(onClose: () -> Unit, onOpenDirector: () -> Unit) {
    val context = LocalContext.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val cards = remember { PostcardShelf.load(context) }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = sheetState,
        containerColor = CabinetTray,
    ) {
        Column(
            Modifier.padding(DuckTheme.Space.xl),
            verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.md),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Copy.postcardShelf,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = DuckTheme.Type.Title,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("postcards-title").duckSheetFirstFocus(),
                )
                CloseChip("postcards-close", onClose)
            }
            if (cards.isEmpty()) {
                DuckEmptyState(
                    title = Copy.postcardNone,
                    actionTitle = Copy.emptyPostcardAction,
                    onAction = {
                        onClose()
                        onOpenDirector()
                    },
                    testTag = "postcards-empty",
                )
            } else {
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(minSize = 150.dp),
                    verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
                    horizontalArrangement = Arrangement.spacedBy(DuckTheme.Space.sm),
                    modifier = Modifier.heightIn(max = 420.dp),
                ) {
                    items(cards, key = { it.file }) { card ->
                        val bitmap = remember(card.file) {
                            BitmapFactory.decodeFile(
                                File(PostcardShelf.dir(context), card.file).path
                            )
                        }
                        Column(verticalArrangement = Arrangement.spacedBy(DuckTheme.Space.xs)) {
                            bitmap?.let {
                                Image(
                                    bitmap = it.asImageBitmap(),
                                    contentDescription = card.caption,
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .height(150.dp)
                                        .background(
                                            Color.White.copy(alpha = 0.06f),
                                            RoundedCornerShape(DuckTheme.Radius.thumb),
                                        )
                                        .testTag("postcard-${card.file}"),
                                    contentScale = ContentScale.Crop,
                                )
                            }
                            Text(
                                card.caption,
                                color = Color.White.copy(alpha = 0.8f),
                                fontSize = DuckTheme.Type.Caption,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Row(
                                Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    card.day,
                                    color = Color.White.copy(alpha = 0.5f),
                                    fontSize = DuckTheme.Type.Micro,
                                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                                )
                                androidx.compose.foundation.layout.Spacer(
                                    Modifier.weight(1f)
                                )
                                ButtonChip(Copy.share, "postcard-share-${card.file}") {
                                    sharePostcard(
                                        context,
                                        File(PostcardShelf.dir(context), card.file),
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
