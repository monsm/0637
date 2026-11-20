import SwiftUI

/// D4 §3.5: a sheet opens with VoiceOver focus on its first element, and
/// closing it puts focus back on the control that opened it (the return half
/// lives in ContentView, which owns the route). One modifier so no sheet can
/// drift from the rule; the short delay lets the presentation finish before
/// the focus move, otherwise UIKit drops it during the transition.
struct DuckSheetFirstFocus: ViewModifier {
    var delay: Double = 0.35
    @AccessibilityFocusState private var focused: Bool
    /// The probe marker. XCUITest cannot query the OS accessibility focus,
    /// so the test-only value reports that this anchor was armed (and the
    /// real @AccessibilityFocusState was set) rather than guessing.
    @State private var armed = false

    /// XCUITest cannot query the OS accessibility focus; with the probe
    /// launch argument the focused element carries it as its value so the
    /// test asserts the focus anchor the modifier drives.
    private var probe: Bool {
        UserDefaults.standard.string(forKey: "duck.focusProbe")?.lowercased() == "yes"
    }

    func body(content: Content) -> some View {
        Group {
            if probe {
                content
                    .accessibilityFocused($focused)
                    .accessibilityValue(armed ? "focused" : "")
                    .overlay(alignment: .topLeading) {
                        if armed {
                            Text("focus")
                                .font(.duckMicro)
                                .opacity(0.01)
                                .allowsHitTesting(false)
                                .accessibilityIdentifier("sheet-focus-probe")
                        }
                    }
            } else {
                content.accessibilityFocused($focused)
            }
        }
        .onAppear {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                focused = true
                armed = true
            }
        }
    }
}

extension View {
    /// Mark this view as the sheet's first focus stop.
    func duckSheetFirstFocus() -> some View {
        modifier(DuckSheetFirstFocus())
    }
}

/// The stage controls a sheet can return focus to. Shared here because the
/// focus modifier and ContentView's state must name the same type.
enum DuckStageFocus: Hashable {
    case feed, skills, quack, grab, more, time, surface, progress
}

/// Applies an accessibility-focus anchor only to the control that owns one;
/// the other buttons on the same row stay ordinary elements.
struct StageFocusModifier: ViewModifier {
    let focus: DuckStageFocus?
    let active: Bool
    let binding: AccessibilityFocusState<DuckStageFocus?>.Binding

    private var probe: Bool {
        UserDefaults.standard.string(forKey: "duck.focusProbe")?.lowercased() == "yes"
    }

    func body(content: Content) -> some View {
        if let focus {
            if probe && active {
                content
                    .accessibilityFocused(binding, equals: focus)
                    .overlay(alignment: .topLeading) {
                        Text(" ")
                            .font(.duckMicro)
                            .opacity(0.01)
                            .allowsHitTesting(false)
                            .accessibilityIdentifier("stage-focus-\(focus)")
                    }
            } else {
                content.accessibilityFocused(binding, equals: focus)
            }
        } else {
            content
        }
    }
}

/// The test-only focus read-out: the return-focus half of D4 §3.5, visible
/// to XCUITest as an identifier/value pair (absent from the product UI).
struct DuckFocusProbe: View {
    let line: String
    private var on: Bool {
        UserDefaults.standard.string(forKey: "duck.focusProbe")?.lowercased() == "yes"
    }

    var body: some View {
        if on {
            Text(line)
                .font(.duckMicro)
                .foregroundStyle(.white.opacity(0.3))
                .allowsHitTesting(false)
                .accessibilityIdentifier("focus-probe")
        }
    }
}

extension View {
    /// Attach the test-only focus read-out without spending shell lines.
    func duckFocusProbe(_ focus: DuckStageFocus?) -> some View {
        overlay(alignment: .bottomTrailing) {
            DuckFocusProbe(line: focus.map { "\($0)" } ?? "none")
        }
    }

    /// The return-focus probe for a stage control that binds focus directly.
    func duckStageFocusBadge(_ focus: DuckStageFocus, active: Bool) -> some View {
        overlay(alignment: .topLeading) {
            if active,
               UserDefaults.standard.string(forKey: "duck.focusProbe")?.lowercased() == "yes" {
                Text("focus")
                    .font(.duckMicro)
                    .opacity(0.01)
                    .allowsHitTesting(false)
                    .accessibilityIdentifier("stage-focus-\(focus)")
            }
        }
    }
}
