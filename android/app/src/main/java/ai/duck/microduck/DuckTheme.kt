package ai.duck.microduck

import androidx.compose.ui.graphics.Color

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
}
