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

/// type.* — the seven text roles (UI_TOKENS §3) expressed as semantic
/// fonts, so Dynamic Type scales every one of them. The semantic ramp lines
/// up with the token sizes: .title 28, .title3 20, .subheadline 15,
/// .footnote 13, .caption2 11. Chinese keeps the system glyphs (the boss
/// ruled no bundled rounded-CJK face); `rounded` still applies to Latin.
extension Font {
    static let duckDisplay = Font.system(.title, design: .rounded, weight: .semibold)
    static let duckTitle = Font.system(.title3, design: .rounded, weight: .semibold)
    static let duckBody = Font.system(.subheadline, design: .rounded, weight: .regular)
    static let duckLabel = Font.system(.footnote, design: .rounded, weight: .medium)
    static let duckCaption = Font.system(.caption2, design: .rounded, weight: .medium)
    static let duckData = Font.system(.caption2, design: .monospaced, weight: .regular)
    /// Ten-point instrument ticks follow .caption2 so they scale too; the
    /// one-point difference is the price of a working Dynamic Type.
    static let duckMicro = Font.system(.caption2, design: .monospaced, weight: .regular)
}

/// Icon glyphs are geometry, not text: they keep their point size while the
/// words beside them grow. Four sizes, no literals outside this file.
enum DuckIcon {
    static let xs = Font.system(size: 12, weight: .medium)
    static let sm = Font.system(size: 15, weight: .medium)
    static let md = Font.system(size: 18, weight: .medium)
    static let lg = Font.system(size: 22, weight: .medium)
}

/// space.* — the 4-point grid (UI_TOKENS §4). Hand-tuned 1/3/5/9/18 values
/// snap to the nearest step; 0 stays 0.
enum DuckSpace {
    static let xxs: CGFloat = 2
    static let xs: CGFloat = 4
    static let sm: CGFloat = 8
    static let md: CGFloat = 12
    static let lg: CGFloat = 16
    static let xl: CGFloat = 20
    static let xxl: CGFloat = 24
    static let xxxl: CGFloat = 32
}

/// radius.* — one gear per role (UI_TOKENS §4); `tiny` is the 4 pt optical
/// exception for tags and hairlines, so a 3 pt literal never comes back.
enum DuckRadius {
    static let tiny: CGFloat = 4
    static let thumb: CGFloat = 12
    static let control: CGFloat = 14
    static let card: CGFloat = 18
    static let panel: CGFloat = 24
}
