import SwiftUI

/// The one colour table both phones share (UI_TOKENS §7). Every value is a
/// decision from docs/UI_TOKENS.md, not a taste: the warm black of the room,
/// the duck's beak orange as the ONE action accent, a cool blue for data,
/// and three state colours. ProgressKit's old private extension carried
/// these same values since 11-13; this file is where they live now, under
/// the names the whole app already calls.
extension Color {
    /// bg.root #161513 — panels, warm black, not cold blue-black.
    static let duckBgRoot = Color(red: 0.086, green: 0.082, blue: 0.075)
    /// bg.surface #221F1D — cards, overlays, sheets.
    static let duckBgSurface = Color(red: 0.133, green: 0.122, blue: 0.114)
    /// text.primary #F6F1E8 — warm white, never pure white.
    static let duckTextPrimary = Color(red: 0.965, green: 0.945, blue: 0.910)
    /// text.secondary — text.primary at 70% (UI_TOKENS §2.2).
    static var duckTextSecondary: Color { duckTextPrimary.opacity(0.70) }
    /// text.tertiary — text.primary at 50%, the documented floor.
    static var duckTextTertiary: Color { duckTextPrimary.opacity(0.50) }
    /// text.onAccent #1A1208 — the word on a beak-orange chip.
    static let duckOnAccent = Color(red: 0.102, green: 0.071, blue: 0.031)
    /// accent.action #FF8A3D — the ONE action accent (beak-orange, one up
    /// from the 3D material's #FF671F). Selection, checks, primary buttons.
    static let duckAction = Color(red: 1.0, green: 0.541, blue: 0.239)
    /// accent.data #7FB2D9 — numbers, gauges, the instrument's cool voice.
    static let duckData = Color(red: 0.498, green: 0.698, blue: 0.851)
    /// accent.info #B08FD9 — the time machine and playback.
    static let duckInfo = Color(red: 0.690, green: 0.561, blue: 0.851)
    /// state.good #86D6A4.
    static let duckGood = Color(red: 0.525, green: 0.839, blue: 0.643)
    /// state.warn #E8B45E.
    static let duckWarn = Color(red: 0.910, green: 0.706, blue: 0.369)
    /// state.bad #E8836B.
    static let duckBad = Color(red: 0.910, green: 0.514, blue: 0.420)
    /// The Instrument stage's cold clear colour (STAGE_LOOK §3 look 2):
    /// a stage value, mirrored here so the UI layers behind the 3D view
    /// never invent a second cold black. Renderer keeps its own copy.
    static let duckStageInstrument = Color(red: 0.055, green: 0.075, blue: 0.10)
}
