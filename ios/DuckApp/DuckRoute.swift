import SwiftUI

/// One route table for the shell (B7, docs/UIUX_REFACTOR_2026-11-15.md §5).
///
/// The iOS shell used to keep three trees: `ContentView.Modal` (8 cases),
/// `MoreSheet.Destination` (12 cases) and four loose booleans inside
/// `InstrumentSheet` — the same destination could be registered in two of
/// them and disagree. This enum is the single vocabulary. The Android shell
/// carries the same case ids in `DuckRoute.kt`; the Java/Kotlin reducer is
/// unit-tested on the JVM, and this file mirrors its rules so the phones
/// cannot drift.
///
/// Ring and sheet are deliberately two fields: the skill ring floats above
/// the chrome and may be up while a sheet is open; one enum would let them
/// cancel each other.
enum DuckRoute: String, CaseIterable, Identifiable {
    case instrument
    case photo
    case rules
    case peers
    case ai
    case dance
    case progress
    case more
    case diary
    case postcards
    case dex
    case modes
    case big
    case playMore
    case duckPanel
    case tools
    case backup
    case ab

    var id: String { rawValue }

    /// The tag a per-route test looks for on the destination surface.
    var tag: String { "route-\(rawValue)" }
}

enum ShellOverlay: Equatable {
    case none
    case ring
}

/// The whole navigation state, small enough to compare and to test. The
/// photo payload stays outside: a route restores a destination, never a
/// stale image.
struct ShellRouteState: Equatable {
    var sheet: DuckRoute?
    var overlay: ShellOverlay = .none

    var layerCount: Int {
        (sheet == nil ? 0 : 1) + (overlay == .none ? 0 : 1)
    }
}

enum ShellRouteAction {
    case open(DuckRoute)
    case closeSheet
    case openRing
    case closeRing
    case back
}

/// The pure reducer — the same five rules as `ShellOverlayReducer` on
/// Android: open replaces the sheet, close clears it, ring and sheet are
/// independent, back closes the topmost layer (ring first).
enum ShellOverlayReducer {
    static func reduce(_ state: ShellRouteState, _ action: ShellRouteAction) -> ShellRouteState {
        switch action {
        case .open(let route):
            return ShellRouteState(sheet: route, overlay: state.overlay)
        case .closeSheet:
            return ShellRouteState(sheet: nil, overlay: state.overlay)
        case .openRing:
            return ShellRouteState(sheet: state.sheet, overlay: .ring)
        case .closeRing:
            return ShellRouteState(sheet: state.sheet, overlay: .none)
        case .back:
            if state.overlay != .none {
                return ShellRouteState(sheet: state.sheet, overlay: .none)
            }
            return ShellRouteState(sheet: nil, overlay: state.overlay)
        }
    }
}
