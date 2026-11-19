package ai.duck.microduck

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/// The one colour table both phones share (UI_TOKENS §7). Every value here
/// is a decision from docs/UI_TOKENS.md, not a taste: the warm black of the
/// room, the duck's beak orange as the one action accent, a cool blue for
/// data, and three state colours. A composable that needs colour names one
/// of these; one that invents a value breaks the cross-platform promise
/// the ratchet (scripts/check-ui-baseline.py) now counts.
object DuckTheme {
    /// bg.root #161513 — panels, warm black, not cold blue-black.
    val BgRoot = Color(0xFF161513)
    /// bg.surface #221F1D — cards, overlays, sheets.
    val BgSurface = Color(0xFF221F1D)
    /// text.primary #F6F1E8 — warm white, never pure white.
    val TextPrimary = Color(0xFFF6F1E8)
    /// text.onAccent #1A1208 — the word on a beak-orange chip.
    val TextOnAccent = Color(0xFF1A1208)
    /// accent.action #FF8A3D — the ONE action accent (beak-orange, one up
    /// from the 3D material's #FF671F). Selection, checks, primary buttons.
    val AccentAction = Color(0xFFFF8A3D)
    /// accent.data #7FB2D9 — numbers, gauges, the instrument's cool voice.
    val AccentData = Color(0xFF7FB2D9)
    /// accent.info #B08FD9 — the time machine and playback.
    val AccentInfo = Color(0xFFB08FD9)
    /// state.good #86D6A4.
    val StateGood = Color(0xFF86D6A4)
    /// state.warn #E8B45E.
    val StateWarn = Color(0xFFE8B45E)
    /// state.bad #E8836B.
    val StateBad = Color(0xFFE8836B)

    /// type.* — the seven text roles (UI_TOKENS §3). sp already scales with
    /// the system font scale; fixed dp containers must use heightIn(min).
    object Type {
        val Micro = 10.sp
        val Caption = 11.sp
        val Label = 13.sp
        val Body = 15.sp
        val Title = 20.sp
        val Display = 28.sp
    }

    /// space.* — the 4-point grid (UI_TOKENS §4). Hand-tuned 1/3/5/9/18
    /// values snap to the nearest step; 0 stays 0.
    object Space {
        val xxs = 2.dp
        val xs = 4.dp
        val sm = 8.dp
        val md = 12.dp
        val lg = 16.dp
        val xl = 20.dp
        val xxl = 24.dp
        val xxxl = 32.dp
    }

    /// radius.* — one gear per role (UI_TOKENS §4); tiny is the 4 dp optical
    /// exception for tags and hairlines, so a 3 dp literal never comes back.
    object Radius {
        val tiny = 4.dp
        val thumb = 12.dp
        val control = 14.dp
        val card = 18.dp
        val panel = 24.dp
    }

    /// Icon glyphs are geometry, not text: fixed dp sizes that do not follow
    /// the font scale, so a 52 dp circle never bursts under large type.
    object Icon {
        val xs = 12.dp
        val sm = 15.dp
        val md = 18.dp
        val lg = 22.dp
    }
}
