import SwiftUI

/// The duck's real pulse, made visible.
///
/// `care.heart_bpm` is the core's own reading of the same mood/hunger/bond/
/// asleep numbers the capsules show (Heart.swift). This draws one soft amber
/// ring around the duck that thumps at that real rate — faster when it is
/// excited, slower when it is dozing. It is a bonus sense, like the haptic
/// heart: nothing else depends on it, and a zero reading (before the first
/// poll) shows nothing.
///
/// The thump is computed from the wall clock, not a timer: the phase is
/// `time × bpm / 60`, so a bpm change lands on the very next frame and no
/// publisher is ever re-created. TimelineView redraws only this ring.
struct HeartRipple: View {
    var bpm: Double

    var body: some View {
        if bpm > 0 {
            if UIAccessibility.isReduceMotionEnabled {
                ring(pulse: 0)
            } else {
                TimelineView(.animation(minimumInterval: 1.0 / 30.0)) { timeline in
                    let beats = timeline.date.timeIntervalSinceReferenceDate * bpm / 60
                    let phase = beats - floor(beats)
                    // A soft lub-dub: the flash decays from the beat and the
                    // ring settles before the next one.
                    ring(pulse: exp(-phase * 5))
                }
            }
        }
    }

    private func ring(pulse: Double) -> some View {
        Circle()
            .stroke(
                Color.duckAction.opacity(0.08 + 0.42 * pulse),
                lineWidth: 1.5 + 2.5 * pulse
            )
            .scaleEffect(0.92 + 0.16 * pulse)
            .frame(width: 150, height: 150)
            .accessibilityHidden(true)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .allowsHitTesting(false)
    }
}