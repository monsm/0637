import SwiftUI

/// Direction-A play: a ripple on the stage whenever the duck is sounding.
///
/// A quack, an emotion's voice, a greet — the duck is speaking, and the
/// room should see it as much as hear it. One expanding, fading ring of
/// the room's amber over the whole stage: it appears and holds while the
/// duck keeps sounding, and fades away when it goes quiet.
///
/// The effect is honest to the signal: `isSpeaking` is published by the
/// 50 Hz tick, so the ripple follows the same mouth timeline the beak uses
/// rather than a timer guessing at the sound. Motion goes through the
/// DuckMotion token (no inline duration), and reduce-motion drops the ring.
struct VoiceRipple: View {
    var isSpeaking: Bool

    var body: some View {
        Circle()
            .fill(
                RadialGradient(
                    colors: [
                        Color.duckAction.opacity(0.55),
                        Color.duckAction.opacity(0),
                    ],
                    center: .center,
                    startRadius: 0,
                    endRadius: 1
                )
            )
            .scaleEffect(isSpeaking ? 1.15 : 0.55)
            .opacity(isSpeaking ? 0.55 : 0)
            .animation(
                UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow,
                value: isSpeaking
            )
            .accessibilityHidden(true)
            // The wave spans the whole stage, and is a picture over the
            // scene — never a hit target, never a control.
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .allowsHitTesting(false)
    }
}
