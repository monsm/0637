package ai.duck.microduck

import androidx.compose.material.icons.Icons as MaterialIcons
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.BackHand
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.Checklist
import androidx.compose.material.icons.outlined.Collections
import androidx.compose.material.icons.outlined.DirectionsWalk
import androidx.compose.material.icons.outlined.FlashlightOn
import androidx.compose.material.icons.outlined.Gesture
import androidx.compose.material.icons.outlined.GraphicEq
import androidx.compose.material.icons.outlined.GridView
import androidx.compose.material.icons.outlined.Height
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Language
import androidx.compose.material.icons.outlined.MenuBook
import androidx.compose.material.icons.outlined.MoreHoriz
import androidx.compose.material.icons.outlined.MusicNote
import androidx.compose.material.icons.outlined.PanTool
import androidx.compose.material.icons.outlined.PhotoLibrary
import androidx.compose.material.icons.outlined.Psychology
import androidx.compose.material.icons.outlined.RadioButtonUnchecked
import androidx.compose.material.icons.outlined.Restaurant
import androidx.compose.material.icons.outlined.SportsEsports
import androidx.compose.material.icons.outlined.Videocam
import androidx.compose.material.icons.outlined.Visibility
import androidx.compose.material.icons.outlined.WbSunny
import androidx.compose.material.icons.outlined.WifiTethering
import androidx.compose.ui.graphics.vector.ImageVector

/**
 * One semantic icon per action, mirroring iOS's Icons.swift. The names are
 * the contract between the platforms; only the glyphs differ.
 */
internal object Icons {
    val feed: ImageVector = MaterialIcons.Outlined.Restaurant
    val skills: ImageVector = MaterialIcons.Outlined.AutoAwesome
    val quack: ImageVector = MaterialIcons.Outlined.GraphicEq
    val grab: ImageVector = MaterialIcons.Outlined.PanTool
    val drop: ImageVector = MaterialIcons.Outlined.BackHand
    val more: ImageVector = MaterialIcons.Outlined.MoreHoriz
    val path: ImageVector = MaterialIcons.Outlined.Gesture
    val blocks: ImageVector = MaterialIcons.Outlined.GridView
    val dance: ImageVector = MaterialIcons.Outlined.MusicNote
    val controls: ImageVector = MaterialIcons.Outlined.SportsEsports
    val peers: ImageVector = MaterialIcons.Outlined.WifiTethering
    val quests: ImageVector = MaterialIcons.Outlined.Checklist
    val diary: ImageVector = MaterialIcons.Outlined.MenuBook
    val postcards: ImageVector = MaterialIcons.Outlined.PhotoLibrary
    val dex: ImageVector = MaterialIcons.Outlined.Collections
    val ai: ImageVector = MaterialIcons.Outlined.Psychology
    val night: ImageVector = MaterialIcons.Outlined.Bedtime
    val morning: ImageVector = MaterialIcons.Outlined.WbSunny
    val flashlight: ImageVector = MaterialIcons.Outlined.FlashlightOn
    val language: ImageVector = MaterialIcons.Outlined.Language
    val time: ImageVector = MaterialIcons.Outlined.History
    val director: ImageVector = MaterialIcons.Outlined.Videocam
    val live: ImageVector = MaterialIcons.Outlined.RadioButtonUnchecked

    /** The three stick modes the pad understands (path has its own row). */
    fun stick(mode: String): ImageVector = when (mode) {
        "head" -> MaterialIcons.Outlined.Visibility
        "body" -> MaterialIcons.Outlined.Height
        "path" -> MaterialIcons.Outlined.Gesture
        else -> MaterialIcons.Outlined.DirectionsWalk
    }
}
