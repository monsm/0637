import SwiftUI

/// The stage props' real cues, published by the controller's /state poll:
/// the idle glance's yaw (the core's own idle head command), the vet book's
/// counters, and the emotion timeline the duck is in right now. A kick fired
/// by a rule, a neighbour or a skill counts exactly like one fired by a
/// hand — the prop moves only when the duck really moved.
struct PropCues: Equatable {
    var idleYaw: Double = 0
    var kickLeft = 0
    var kickRight = 0
    var groundPicks = 0
    var sitDowns = 0
    var rolls = 0
    /// The duck's own emotion timeline (`/state.emotion.name`), "" when none
    /// is playing. The props answer it: a curious duck makes the mirror
    /// glint, a laughing or sad one warms the cushion.
    var emotion = ""
    // The guards the self-play scheduler reads, from the same /state.
    var idleTakeover: Double = 0
    var paused = false
    var carrying = false
    var idleS: Double = 0
    var tick: Int = 0
    // The autonomy cues: the duck's own mood and hunger.
    var mood: Double = 0
    var hunger: Double = 0
}

/// A self-play beat no sooner than this many simulated ticks (25 s at 50 Hz).
let selfPlayCooldownTicks = 1250

/// The duck must have been left alone this long before it goes to play.
let selfPlayIdleSeconds = 10.0

/// Props on the duck's floor: a ball it can kick, blocks it can pick, a
/// cushion it can sit on, a hoop it can roll through, and a mirror it can
/// look into.
///
/// Every prop answers with the duck's own body — the kick is the official
/// ball_kick policy, the pick the ground_pick scheduler, the sit the loop's
/// own sit, the roll the official roulade. Nothing here invents a verb; the
/// prop is a target, not an animation. The duck also reaches for props on
/// its own: the idle look-around's real head command lights the prop it is
/// looking at, a kick/pick/sit/roll it actually started moves the matching
/// prop, and its own emotions reach them — a curious duck's mirror glints,
/// an excited one twirls the hoop. A sleeping duck answers nothing; the core
/// owns sleep.
///
/// The layer takes no touches: the stage gesture reads taps against the
/// same zones (`StagePropsLayout.hit`), so the first screen's seven-control
/// budget is untouched and TalkBack still hears one stage element.
final class StagePropsModel: ObservableObject {
    @Published private(set) var ballFlight: CGFloat = 0
    @Published private(set) var blockCarried = false
    @Published private(set) var warmGlow = false
    @Published private(set) var hoopSpin: CGFloat = 0
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
        if cues.rolls > last.rolls { spin() }
        // The duck's own excitement reaches the hoop even when the emotion
        // came from a rule, an achievement or a neighbour.
        if cues.emotion == "excited" && last.emotion != "excited" { spin() }
    }

    private func fly(_ direction: CGFloat) {
        ballFlight = direction
        beat(UIAccessibility.isReduceMotionEnabled ? 0.0 : 0.8) { [weak self] in
            guard let self, self.ballFlight == direction else { return }
            self.ballFlight = 0
        }
    }

    private func carry() {
        blockCarried = true
        beat(UIAccessibility.isReduceMotionEnabled ? 0.0 : 1.0) { [weak self] in
            self?.blockCarried = false
        }
    }

    private func warm() {
        warmGlow = true
        beat(UIAccessibility.isReduceMotionEnabled ? 0.0 : 2.0) { [weak self] in
            self?.warmGlow = false
        }
    }

    private func spin() {
        hoopSpin = 1
        beat(UIAccessibility.isReduceMotionEnabled ? 0.0 : 1.2) { [weak self] in
            self?.hoopSpin = 0
        }
    }

    private func beat(_ delay: TimeInterval, _ work: @escaping () -> Void) {
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }
}

struct StageProps: View {
    @ObservedObject var model: StagePropsModel
    /// The duck's own emotion timeline, "" when none is playing.
    let emotion: String
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
                propVisual(.hoop, zone: StagePropsLayout.hoop, w: w, h: h) {
                    hoop
                        .rotationEffect(.degrees(Double(model.hoopSpin) * 140))
                        .scaleEffect(1 + model.hoopSpin * 0.12)
                }
                propVisual(.block, zone: StagePropsLayout.block, w: w, h: h) {
                    block
                        .offset(y: model.blockCarried ? -h * 0.06 : 0)
                        .opacity(model.blockCarried ? 0.35 : 1)
                }
                propVisual(.mirror, zone: StagePropsLayout.mirror, w: w, h: h) {
                    mirror
                }
                propVisual(.cushion, zone: StagePropsLayout.cushion, w: w, h: h) {
                    cushion
                        .overlay(
                            Capsule()
                                .fill(Color.duckAction.opacity(cushionWarm ? 0.35 : 0))
                        )
                }
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.ballFlight)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.blockCarried)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.hoopSpin)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.slow, value: model.warmGlow)
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.base, value: model.lookedAt)
    }

    /// The cushion warms for a real sit, and for the duck's own laugh or
    /// sadness (its emotion timeline, not a guess).
    private var cushionWarm: Bool {
        model.warmGlow || emotion == "laugh" || emotion == "sad"
    }

    /// One prop's visual, positioned at its zone's centre and glowing when
    /// the duck is looking at it, just touched it, or is feeling it.
    private func propVisual(
        _ kind: PropKind, zone: StagePropsLayout.Zone, w: CGFloat, h: CGFloat,
        @ViewBuilder visual: () -> some View
    ) -> some View {
        let glow = !asleep && (model.lookedAt == zone.side
            || StagePropsLayout.hitCue(zone: zone, model: model, emotion: emotion, asleep: asleep))
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

    /// The hoop: a brass ring the duck rolls through, under the ball.
    private var hoop: some View {
        Circle()
            .strokeBorder(
                LinearGradient(
                    colors: [Color.duckBrass, Color.duckBrassDeep],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                ),
                lineWidth: 3
            )
            .frame(width: 30, height: 30)
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

    /// The mirror: a small silver disc under the block, for the duck's own
    /// curious glances.
    private var mirror: some View {
        ZStack {
            Circle()
                .fill(
                    RadialGradient(
                        colors: [Color.white.opacity(0.92), Color.duckCeramic.opacity(0.55)],
                        center: .topLeading, startRadius: 1, endRadius: 18
                    )
                )
                .frame(width: 26, height: 26)
            Circle()
                .strokeBorder(Color.duckBrass, lineWidth: 2)
                .frame(width: 26, height: 26)
            Rectangle()
                .fill(Color.white.opacity(0.75))
                .frame(width: 8, height: 2)
                .rotationEffect(.degrees(-38))
                .offset(x: -3, y: -4)
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
    case hoop
    case block
    case mirror
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

    static let ball = Zone(x: 0.14, y: 0.62, side: .left, cueHit: .ball)
    static let hoop = Zone(x: 0.14, y: 0.76, side: .left, cueHit: .hoop)
    static let block = Zone(x: 0.86, y: 0.62, side: .right, cueHit: .block)
    static let mirror = Zone(x: 0.86, y: 0.76, side: .right, cueHit: .mirror)
    static let cushion = Zone(x: 0.50, y: 0.76, side: .centre, cueHit: .cushion)

    /// Whether a prop is the one a real move (or the duck's own mood) just
    /// touched. The mirror and cushion also answer the emotion timeline.
    static func hitCue(
        zone: Zone, model: StagePropsModel, emotion: String, asleep: Bool
    ) -> Bool {
        guard !asleep else { return false }
        switch zone.cueHit {
        case .ball: return model.ballFlight != 0 || emotion == "excited"
        case .hoop: return model.hoopSpin != 0 || emotion == "excited"
        case .block: return model.blockCarried
        case .mirror: return emotion == "curious"
        case .cushion:
            return model.warmGlow || emotion == "laugh" || emotion == "sad"
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
        if x < 0.25 && y >= 0.50 && y < 0.70 { return .ball }
        if x < 0.25 && y >= 0.70 { return .hoop }
        if x > 0.75 && y >= 0.50 && y < 0.70 { return .block }
        if x > 0.75 && y >= 0.70 { return .mirror }
        if x > 0.34 && x < 0.66 && y >= 0.70 { return .cushion }
        return nil
    }
}

/// The faces the duck makes at the mirror, in the order a child meets them.
/// Official emotions only; the timeline plays its own voice. Session-only.
private let mirrorFaces = ["curious", "laugh", "excited", "sad", "yes"]
private var mirrorTaps = 0

/// The face the next accepted mirror tap will make.
func mirrorFace() -> String { mirrorFaces[mirrorTaps % mirrorFaces.count] }

/// The duck finished a face; the next tap meets the next one.
func mirrorFaceDone() { mirrorTaps += 1 }

/// A prop was tapped: the duck does its real verb toward it. The prop's
/// response comes from the vet book's counter (or the emotion timeline) a
/// beat later, so the screen cannot claim a move the core refused. Returns
/// whether the core accepted the verb — a refusal changes nothing.
///
/// Each prop also gives the duck its real synth voice (the core's own tags);
/// the mirror needs none — an emotion timeline carries its own voice.
@discardableResult
func propagate(_ prop: StagePropHit, controller: DuckController) -> Bool {
    let face = prop == .mirror ? mirrorFace() : ""
    let accepted: Bool
    switch prop {
    case .ball: accepted = controller.trigger("kick_left")
    case .hoop: accepted = controller.trigger("roulade")
    case .block: accepted = controller.groundPick()
    case .mirror:
        accepted = controller.trigger(face)
        if accepted { mirrorFaceDone() }
    case .cushion: accepted = controller.trigger("sit")
    }
    if accepted {
        switch prop {
        case .ball: controller.queueSound("peck")
        case .hoop: controller.queueSound("chirp")
        case .block: controller.queueSound("greet")
        case .cushion: controller.queueSound("coo")
        case .mirror: break
        }
    }
    DuckHaptics.selection()
    return accepted
}

/// Whether the duck may go off to play now. Every input is a real /state
/// field: awake, unfrozen, not carried, not in an emotion, genuinely idle
/// (the core's own look-around owns the head), left alone long enough, and
/// past the cooldown. The caller adds its own window guard (no sheet).
func selfPlayDue(_ cues: PropCues, asleep: Bool, enabled: Bool, lastTick: Int) -> Bool {
    guard enabled, !asleep, !cues.paused, !cues.carrying, cues.emotion.isEmpty
    else { return false }
    guard cues.idleTakeover > 0.1, cues.idleS >= selfPlayIdleSeconds
    else { return false }
    return lastTick <= 0 || cues.tick - lastTick >= selfPlayCooldownTicks
}

/// The prop the duck chooses this beat: a pure function of its own
/// fingerprint and the simulated tick, so two ducks differ and the same duck
/// is stable within a beat — a personality, not a coin.
func selfPlayProp(fingerprint: String, tick: Int) -> StagePropHit {
    var hash: UInt64 = 0xcbf29ce484222325
    for byte in fingerprint.utf8 {
        hash ^= UInt64(byte)
        hash = hash &* 0x100000001b3
    }
    hash ^= UInt64(bitPattern: Int64(tick / selfPlayCooldownTicks))
    let list: [StagePropHit] = [.ball, .hoop, .block, .mirror, .cushion]
    return list[Int(hash % UInt64(list.count))]
}

/// The chip that asks for self-play. Off by default; the choice persists so
/// a child's duck keeps its habit across launches.
struct SelfPlayChip: View {
    @AppStorage("duck_self_play") private var on = false

    var body: some View {
        VStack(spacing: DuckSpace.xxs) {
            Circle()
                .fill(on ? Color.duckAction : Color.duckBrass)
                .frame(width: 20, height: 20)
                .overlay(
                    Circle()
                        .fill(Color.black.opacity(0.35))
                        .frame(width: 7, height: 7)
                )
            Text(on ? Copy.selfPlayOn : Copy.selfPlay)
                .font(.duckCaption)
                .foregroundStyle(.white.opacity(0.7))
                .lineLimit(1)
        }
        .padding(DuckSpace.xs)
        .contentShape(Rectangle())
        .onTapGesture {
            on.toggle()
            DuckHaptics.selection()
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("stage-self-play")
        .accessibilityLabel(Copy.selfPlay)
        .accessibilityValue(on ? "on" : "off")
        // Deliberately NOT a button trait: the first screen's seven-control
        // budget is the shelf/wall set, and the switch is scene like the
        // props. A screen reader toggles it through the stage's named action
        // (the same split Android uses).
    }
}

enum PropKind {
    case ball, hoop, block, mirror, cushion
}
