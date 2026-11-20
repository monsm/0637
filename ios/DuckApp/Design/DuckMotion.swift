import UIKit
import SwiftUI

/// The four motion gears (UI_TOKENS §5, D3). Every transition in the app
/// picks one of these; a raw duration outside this file is a defect the
/// ratchet counts.
enum DuckMotion {
    /// Button feedback, chip states.
    static let quick: Animation = .easeOut(duration: 0.15)
    /// State switches, capsule fades.
    static let base: Animation = .easeInOut(duration: 0.25)
    /// Sheets, camera switches.
    static let slow: Animation = .easeOut(duration: 0.40)
    /// Skill ring, the time rail's rise.
    static let deliberate: Animation = .spring(response: 0.32, dampingFraction: 0.9)
}

/// Reduce Motion is honoured in one place: state changes still run (the
/// colour and the words stay), the transition just stops animating.
func withDuckMotion(_ animation: Animation, _ body: () -> Void) {
    if UIAccessibility.isReduceMotionEnabled {
        body()
    } else {
        withAnimation(animation, body)
    }
}

/// The haptic map (总纲 §3.4): one event, one strength, one generator. Every
/// entry is a real user-visible moment — nothing fires on state polling.
enum DuckHaptics {
    /// A tap on the duck.
    static func pet() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
    /// A skill leaves the ring, a mode flips.
    static func selection() { UISelectionFeedbackGenerator().selectionChanged() }
    /// The duck is fed.
    static func success() {
        UINotificationFeedbackGenerator().notificationOccurred(.success)
    }
    /// The duck falls.
    static func error() {
        UINotificationFeedbackGenerator().notificationOccurred(.error)
    }
    /// A drawn route finishes on its own.
    static func medium() { UIImpactFeedbackGenerator(style: .medium).impactOccurred() }
}
