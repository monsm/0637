import SwiftUI

/// The stage props' real cues, published by the controller's /state poll:
/// the idle glance's yaw (the core's own idle head command) and the vet
/// book's counters. A kick fired by a rule, a neighbour or a skill counts
/// exactly like one fired by a hand — the prop moves only when the duck
/// really moved.
struct PropCues: Equatable {
    var idleYaw: Double = 0
    var kickLeft = 0
    var kickRight = 0
    var groundPicks = 0
    var sitDowns = 0
}

/// Props on the duck's floor: a ball it can kick, a block it can pick, a
/// cushion it can sit on.
///
/// Every prop answers with the duck's own body — the kick is the official
/// ball_kick policy, the pick the ground_pick scheduler, the sit the loop's
/// own sit. Nothing here invents a verb; the prop is a target, not an
/// animation. The duck also reaches for props on its own: the idle
/// look-around's real head command lights the prop it is looking at, and a
/// kick, pick or sit it actually started moves the matching prop. A sleeping
/// duck answers nothing — the core owns sleep.
///
/// The layer takes no touches: the stage gesture reads taps against the
/// same zones (`StagePropsLayout.hit`), so the first screen's seven-control
/// budget is untouched and TalkBack still hears one stage element.
final class StagePropsModel: ObservableObject {
    @Published private(set) var ballFlight: CGFloat = 0
    @Published private(set) var blockCarried = false
    @Published private(set) var warmGlow = false
    @Published private(set) var lookedAt: PropSide?

    private var last = PropCues()

    /// The poll delivered new cues. Increments are the duck's real moves;
    /// equal values change nothing (the 1 Hz poll must not restart a beat).
    func apply(_ cues: PropCues, asleep: Bool) {
        defer { last = cues }
        guard !asleep else {
            lookedAt = nil
            return
        }
        if cues.idleYaw != last.idleYaw {
            lookedAt = StagePropsLayout.side(of: cues.idleYaw)
        }
        if cues.kickLeft > last.kickLeft { fly(-1) }
        if cues.kickRight > last.kickRight { fly(1) }
        if cues.groundPicks > last.groundPicks { carry() }
        if cues.sitDowns > last.sitDowns { warm() }
    }

    private func fly(_ direction: CGFloat) {
        ballFlight = direction
        let beat = UIAccessibility.isReduceMotionEnabled ? 0.0 : 0.8
        DispatchQueue.main.asyncAfter(deadline: .now() + beat) { [weak self] in
            guard let self, self.ballFlight == direction else { return }
            self.ballFlight = 0
        }
    }

    private func carry() {
        blockCarried = true
        let beat = UIAccessibility.isReduceMotionEnabled ? 0.0 : 1.0
        DispatchQueue.main.asyncAfter(deadline: .now() + beat) { [weak self] in
            self?.blockCarried = false
        }
    }

    private func warm() {
        warmGlow = true
        let beat = UIAccessibility.isReduceMotionEnabled ? 0.0 : 2.0
        DispatchQueue.main.asyncAfter(deadline: .now() + beat) { [weak self] in
            self?.warmGlow = false
        }
    }
}

struct StageProps: View {
    @ObservedObject var model: StagePropsModel
    /// The duck is asleep — props go still, like the duck.
    let asleep: Bool

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            ZStack {
                propVisual(.ball, zone: StagePropsLayout.ball, w: w, h: h) {
                    ball
                        .offset(
                            x: model.ballFlight * w * 0.16,
                            y: -abs(model.ballFlight) * h * 0.09
                        )
                        .opacity(1 - abs(model.ballFlight) * 0.40)
                }
                propVisual(.block, zone: StagePropsLayout.block, w: w, h: h) {
                    block
                        .offset(y: model.blockCarried ? -h * 0.06 : 0)
                        .opacity(model.blockCarried ? 0.35 : 1)
                }
                propVisual(.cushion, zone: StagePropsLayout.cushion, w: w, h: h) {
                    cushion
                        .overlay(
                            Capsule()
                                .fill(Color.duckAction.opacity(model.warmGlow ? 0.35 : 0))
                        )
                }
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.ballFlight)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.blockCarried)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.warmGlow)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.base, value: model.lookedAt)
    }

    /// One prop's visual, positioned at its zone's centre and glowing when
    /// the duck is looking at it or just touched it.
    private func propVisual(
        _ kind: PropKind, zone: StagePropsLayout.Zone, w: CGFloat, h: CGFloat,
        @ViewBuilder visual: () -> some View
    ) -> some View {
        let glow = !asleep && (model.lookedAt == zone.side
            || StagePropsLayout.hitCue(zone: zone, model: model, asleep: asleep))
        return visual()
            .position(x: zone.x * w, y: zone.y * h)
            .scaleEffect(glow ? 1.12 : 1)
            .brightness(glow ? 0.25 : 0)
    }

    // MARK: - the props themselves

    /// The ball: a small sphere at the floor line, left of the duck.
    private var ball: some View {
        ZStack {
            Circle()
                .fill(
                    RadialGradient(
                        colors: [Color.duckAction, Color.duckAction.opacity(0.55)],
                        center: .topLeading,
                        startRadius: 1, endRadius: 22
                    )
                )
                .frame(width: 30, height: 30)
            Circle()
                .strokeBorder(Color.white.opacity(0.65), lineWidth: 2)
                .frame(width: 30, height: 30)
            Rectangle()
                .fill(Color.white.opacity(0.5))
                .frame(width: 30, height: 2.5)
                .rotationEffect(.degrees(-18))
        }
    }

    /// The block: a small wooden cube at the floor line, right of the duck.
    private var block: some View {
        ZStack {
            RoundedRectangle(cornerRadius: DuckRadius.tiny)
                .fill(
                    LinearGradient(
                        colors: [Color.duckBrass, Color.duckBrassDeep],
                        startPoint: .top, endPoint: .bottom
                    )
                )
                .frame(width: 24, height: 24)
            Circle()
                .fill(Color.black.opacity(0.35))
                .frame(width: 6, height: 6)
        }
    }

    /// The cushion: a low pad under the duck's feet, centre.
    private var cushion: some View {
        Capsule()
            .fill(
                LinearGradient(
                    colors: [Color.duckCeramic.opacity(0.9), Color.duckCeramic.opacity(0.55)],
                    startPoint: .top, endPoint: .bottom
                )
            )
            .frame(width: 74, height: 16)
    }
}

/// Which prop a tap lands on, if any.
enum StagePropHit {
    case ball
    case block
    case cushion
}

/// Which side of the stage a prop sits on.
enum PropSide: Equatable {
    case left
    case right
    case centre
}

enum StagePropsLayout {
    /// A prop zone: its centre as stage fractions, and which side it is on.
    struct Zone {
        let x: CGFloat
        let y: CGFloat
        let side: PropSide
        let cueHit: StagePropHit
    }

    static let ball = Zone(x: 0.16, y: 0.68, side: .left, cueHit: .ball)
    static let block = Zone(x: 0.86, y: 0.68, side: .right, cueHit: .block)
    static let cushion = Zone(x: 0.50, y: 0.75, side: .centre, cueHit: .cushion)

    /// Whether a prop is the one a real move just touched.
    static func hitCue(zone: Zone, model: StagePropsModel, asleep: Bool) -> Bool {
        guard !asleep else { return false }
        switch zone.cueHit {
        case .ball: return model.ballFlight != 0
        case .block: return model.blockCarried
        case .cushion: return model.warmGlow
        }
    }

    /// Which side an idle glance at `yaw` (radians, +right) lights. The
    /// threshold is generous: a glance, not a stare.
    static func side(of yaw: Double) -> PropSide? {
        if yaw > 0.25 { return .right }
        if yaw < -0.25 { return .left }
        return nil
    }

    /// Which prop a tap at these stage fractions lands on, if any. The
    /// zones sit beside and under the duck body (x 0.25–0.75, y 0.33–0.68),
    /// so the body keeps its own taps and the props keep the floor.
    static func hit(x: CGFloat, y: CGFloat) -> StagePropHit? {
        if x < 0.25 && y > 0.58 { return .ball }
        if x > 0.75 && y > 0.58 { return .block }
        if x > 0.36 && x < 0.64 && y > 0.70 { return .cushion }
        return nil
    }
}

/// A prop was tapped: the duck does its real verb toward it. The prop's
/// response comes from the vet book's counter a beat later, so the screen
/// cannot claim a move the core refused.
func propagate(_ prop: StagePropHit, controller: DuckController) {
    switch prop {
    case .ball: controller.trigger("kick_left")
    case .block: controller.groundPick()
    case .cushion: controller.trigger("sit")
    }
    DuckHaptics.selection()
}

enum PropKind {
    case ball, block, cushion
}
