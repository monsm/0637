import Foundation
import Combine
import SwiftUI
import UIKit

/// The fused shell: a live duck first, an instrument underneath, a viewfinder
/// when you want one.
///
/// * **Live** (default): the duck fills the screen and nothing else does. Tap
///   the floor in front of it to walk there, tap either side to turn, long-press
///   it for the ring of skills. There is no visible joystick.
/// * **Instrument**: pull down and the time machine rises — a scrubber over the
///   ticks that really ran, freeze, single-step, fork. The same sheet shows the
///   numbers (label, bring-up, gain, tick) and the sensors.
/// * **Director**: switch the room on and the viewfinder gets a frame, a
///   horizon and the record state; the duck is composed into the room.
struct ContentView: View {
    @StateObject private var controller = DuckController()
    @State private var surface = Surface.live
    /// The pad's stick modes, reduced to the four a phone can express:
    /// walk/turn, look, crouch/lean the standing body, and drawing a route
    /// for the duck to walk.
    @State private var stickMode = StickMode.drive
    @Environment(\.scenePhase) private var scenePhase
    /// The one-time gesture hint. The flag is persisted before the caption
    /// appears, so a crash or a force-quit can never show it a second time.
    /// `showHint` is separate so the fade-out cannot undo that write.
    @AppStorage("first_launch_hint_shown") private var hintShown = false
    @State private var showHint = false

    /// The time button's long press: the quick freeze. The button's own
    /// action fires on the same lift, so the hold has to be able to say
    /// "that was me".
    @State private var timeHolding = false
    @State private var timeHoldEndedAt = Date.distantPast
    /// One route table (B7): every sheet is a DuckRoute, the skill ring is
    /// the overlay field. Rows inside a presented sheet nest their own
    /// destination (DuckRouteSheets), so this field only ever owns the
    /// top-level sheet and a reduce can never stack two of them.
    @State private var routeState = ShellRouteState()
    /// The Director photo behind the `photo` route. Kept out of the route so
    /// a restored destination can never show a stale image.
    @State private var photoPreview: DuckController.CapturedPhoto?
    @State private var tapTarget: CGPoint?
    @State private var lastTap = Date.distantPast
    /// Where the hand was when the grab engaged: a carry measures from the
    /// grab, not from a drag that may have been walking the duck for 150 pt.
    @State private var carryAnchor: CGPoint?
    /// Which tap-pulse a pending zero belongs to: a newer touch invalidates
    /// the old timer, exactly like Android's pulse token.
    @State private var pulseToken = 0
    /// The Path Painter's screen-space draft: the line the finger draws. Its
    /// first point is the origin the body frame is measured from, so the
    /// core drops it when Go posts the route.
    @State private var pathDraft: [CGPoint] = []
    /// Flipping this re-renders every `Copy` string.
    @State private var languageTick = 0
    /// The held joy-ride: a long press on the quack button starts it, lifting
    /// the finger ends it. The flag marks the gesture so the Button's release
    /// action does not also queue a chirp on the way up.
    @State private var wheeeHolding = false
    /// When the gesture itself ended a ride, so the Button action — which
    /// fires on the same finger lift, after the gesture's `onEnded` — can
    /// tell that release apart from a fresh tap. The window matters only for
    /// a lift outside the button, where the action never runs.
    @State private var wheeeEndedAt = Date.distantPast
    /// One transient line above the time rail: what happened while the app
    /// was away. It hides itself and takes no space when empty.
    @State private var moodText: String?
    /// The progress store's unlock signal (a real fact, never invented):
    /// a fresh achievement makes the duck celebrate once.
    @ObservedObject private var progress = ProgressStore.shared
    /// A route just ran its whole course: offer to keep the moment as a
    /// postcard for a few seconds. Real event, real frame — the shutter is
    /// the same one Director uses.
    @State private var pathPostcardReady = false
    /// D4 sheet focus: the stage control a sheet must return focus to when it
    /// closes. The route reducer does not know who opened the sheet, so every
    /// entry point names its own anchor here.
    @AccessibilityFocusState private var stageFocus: DuckStageFocus?
    @State private var lastSheetOpener: DuckStageFocus = .more
    enum Surface: String, CaseIterable {
        case live = "Live"
        case director = "Director"
    }

    var body: some View {
        choreographed()
    }
    /// The stage and its lifecycle modifiers, in three separately checked
    /// pieces: one chain of them exceeded Xcode's type-checker budget once
    /// the route table joined the shell (the error moved from the ZStack to
    /// .task when only the stage was split out). A plain function, not a
    /// ViewBuilder, so the pieces can be bound and returned.
    private func choreographed() -> some View {
        let _ = languageTick
        let base = GeometryReader { geometry in
            // Portrait is the product; landscape is the two-hand instrument
            // and demo layout. A width under 600 keeps phones (and a narrow
            // split view) in the one-thumb shell; wider surfaces keep the
            // already-tested landscape chrome.
            let portrait = geometry.size.height > geometry.size.width
                && geometry.size.width < 600
            let stageView = stage(geometry, portrait: portrait)
            // Halfway through a rotation the thumb's old translation means
            // something else; hand the duck back neutral, once.
            let rotated = stageView
                .onChange(of: portrait) { _, _ in
                controller.joystick = .zero
                controller.setHead(yaw: 0, pitch: 0)
                controller.releaseBody()
                wheeeHolding = false
                controller.setWheee(false)
                // A draft is measured in the old screen space, which the new
                // orientation no longer means.
                pathDraft.removeAll()
            }
            // The 3D scene fills the screen (it owns its own ignoresSafeArea),
            // but the chrome stays inside the safe area: on a notched phone
            // the old full-screen ignore put the buttons under the home bar.
            // One modal sheet for every surface: instrument, Director photo,
            // blocks, neighbours and the user's model.
            rotated
            .sheet(item: $routeState.sheet) { sheet in
                modalSheet(sheet, portrait: portrait)
            }
            // The renderer cannot see a SwiftUI sheet: tell it when the
            // instrument is up so the stage becomes the cold lab (look 2).
            .onChange(of: routeState.sheet) { old, sheet in
                controller.instrumentOpen = (sheet == .instrument)
                // D4: closing a sheet returns VoiceOver focus to the control
                // that opened it; wait out the dismissal before moving it.
                if old != nil, sheet == nil {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        stageFocus = lastSheetOpener
                    }
                }
            }
            .onReceive(controller.$capturedPhoto) { photo in
                if let photo {
                    controller.capturedPhoto = nil
                    photoPreview = photo
                    open(.photo)
                }
            }
            .duckFocusProbe(stageFocus)
        }
        // The hint teaches gestures, so it must wait until gestures work:
        // the core takes seconds to load, and a hint spent over a dead screen
        // is spent forever. It is shown once the duck is ready, on both
        // orientations, and its flag is only written when it is really shown.
        .onChange(of: controller.ready) { _, ready in
            if ready {
                presentHint()
                controller.restoreCompanionState()
                // A launch argument opens the More drawer straight away: CI
                // takes the acceptance screenshot of it without a hand on the
                // phone (`-duck.openMore YES`). The argument domain stores
                // strings, so the value is read as text.
                if let launchArg = UserDefaults.standard.string(forKey: "duck.openMore"),
                   launchArg.lowercased() == "yes" || launchArg == "1" {
                    open(.more)
                }
            }
        }
        // D4: every event pill is announced too — the same words the eye
        // gets, through one hook instead of eighteen call sites.
        .onChange(of: moodText) { _, text in
            if let text, !text.isEmpty {
                UIAccessibility.post(notification: .announcement, argument: text)
            }
        }
        .onChange(of: controller.awaySummary) { _, summary in
            guard let summary, !summary.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = summary }
            DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                guard moodText == summary else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // A clap or a whistle is answered visibly as well as audibly: the
        // same pill the away recap uses, for a shorter spell.
        .onChange(of: controller.roomSound) { _, sound in
            guard let sound, !sound.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = sound }
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
                guard moodText == sound else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // What the duck heard you say, for a moment: without this, a spoken
        // word that no verb matched simply vanished — the ear worked and the
        // screen said nothing, which reads as a deaf duck.
        .onChange(of: controller.heardText) { _, text in
            guard !text.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = text }
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
                guard moodText == text else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // A programmed block shows its work: "拍手 → 翻滚".
        .onChange(of: controller.ruleNotice) { _, notice in
            guard let notice, !notice.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = notice }
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                guard moodText == notice else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // A badge just unlocked (ProgressStore only sets latestUnlock on a
        // fresh unlock during a real record — never at launch): let the duck
        // celebrate. excited is its own celebration; the ripple follows the
        // voice, and the badge is named in the growth book.
        .onChange(of: progress.latestUnlock?.id ?? "") { _, id in
            guard !id.isEmpty else { return }
            controller.trigger("excited")
            DuckHaptics.success()
        }
        // A greeting from the room next door, or the answer to ours.
        .onChange(of: controller.peerNotice) { _, notice in
            guard let notice, !notice.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = notice }
            DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
                guard moodText == notice else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // A neighbour's gift — a treat or a cheer — in the same one-line
        // pill. The tick fires even when two gifts read the same.
        .onChange(of: controller.giftTick) { _, _ in
            guard let line = controller.giftLine, !line.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = line }
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                guard moodText == line else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        let withRoom = base
        // A chorus starting on the Wi-Fi: the same pill, naming the ducks in
        // the room. The tick only fires on the inactive -> active edge, so a
        // long song does not re-announce itself on every publish.
        .onChange(of: controller.chorusTick) { _, tick in
            guard tick > 0 else { return }
            let line = Copy.chorusLine(controller.chorusPeers)
            withDuckMotion(DuckMotion.base) { moodText = line }
            DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
                guard moodText == line else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // The model's own line, when it decides something.
        .onChange(of: controller.aiDecision) { _, decision in
            guard !decision.isEmpty else { return }
            withDuckMotion(DuckMotion.base) { moodText = decision }
            DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
                guard moodText == decision else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // A painted route that ran its whole course gets the same pill the
        // other events use; a Stop press stays quiet.
        .onChange(of: controller.pathDoneTick) { _, tick in
            guard tick > 0 else { return }
            let notice = Copy.pathDone
            withDuckMotion(DuckMotion.base) { moodText = notice }
            withDuckMotion(DuckMotion.base) { pathPostcardReady = true }
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                guard moodText == notice else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 15) {
                if pathPostcardReady { withDuckMotion(DuckMotion.slow) { pathPostcardReady = false } }
            }
        }
        // The morning after a chosen night: the same one-line pill, saying
        // how long the duck stayed quiet. The tick only fires when a real
        // stretch lay behind the wake, so a duck that was put to bed and
        // picked up again at once has no morning to announce.
        .onChange(of: controller.nightWakeTick) { _, tick in
            guard tick > 0 else { return }
            let notice = Copy.goodMorningLine(controller.nightWakeMinutes)
            withDuckMotion(DuckMotion.base) { moodText = notice }
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                guard moodText == notice else { return }
                withDuckMotion(DuckMotion.slow) { moodText = nil }
            }
        }
        // The accepted walk renders from /state.path; the drawn draft has
        // done its job the moment the route starts.
        .onChange(of: controller.pathActive) { _, active in
            if active {
                pathDraft.removeAll()
            }
        }
        // The haptic map (D3): real edges only, nothing on polling.
        .onChange(of: controller.pathDoneTick) { _, tick in if tick > 0 { DuckHaptics.success() } }
        .onChange(of: controller.fallen) { _, fell in if fell { DuckHaptics.error() } }
        .onChange(of: stickMode) { _, _ in DuckHaptics.selection() }
        return withRoom
        .task {
            if controller.ready {
                presentHint()
                controller.restoreCompanionState()
            }
        }
        // SwiftUI does not report a cancelled drag. A phone call, a
        // notification pull or leaving the app would otherwise leave the
        // joystick where the finger was and the duck walking by itself.
        .onChange(of: scenePhase) { _, phase in
            if phase == .background {
                controller.appWentBackground()
            }
            if phase == .active {
                controller.appBecameActive()
            }
            if phase != .active {
                controller.joystick = .zero
                controller.setHead(yaw: 0, pitch: 0)
                controller.releaseBody()
                // SwiftUI does not report a gesture cancelled by backgrounding.
                // End the ride here so its loops cannot outlive the finger.
                wheeeHolding = false
                controller.setWheee(false)
            }
        }
    }

    // MARK: - Scene

    @ViewBuilder
    private var scene: some View {
        if controller.ready && controller.renderError == nil {
            ZStack {
                // Behind the Metal view, never as its sublayer: sublayers draw
                // above the drawable and would cover the duck.
                if controller.cameraActive {
                    CameraBackdrop(session: controller.camera.session)
                        .ignoresSafeArea()
                        .allowsHitTesting(false)
                }
                MetalView(controller: controller)
                    .ignoresSafeArea()
                // The observation HUD: a few REAL numbers over the stage -
                // the same tick and mode the status line publishes, drawn
                // where the person is looking instead of behind a sheet.
                if controller.hudOn {
                    VStack(alignment: .leading) {
                        Text("tick \(controller.simTick) · \(controller.label)")
                            .font(.duckData)
                            .padding(.horizontal, DuckSpace.sm)
                            .padding(.vertical, DuckSpace.xs)
                            .background(Color.black.opacity(0.45), in: RoundedRectangle(cornerRadius: DuckRadius.tiny))
                            .accessibilityIdentifier("hud-overlay")
                        Spacer()
                    }
                    .padding(.top, DuckSpace.sm)
                    .padding(.horizontal, DuckSpace.lg)
                    .allowsHitTesting(false)
                }
                // The second player: the right edge belongs to them - three
                // verbs at arm's length while player one keeps the joystick.
                if controller.p2On {
                    VStack(spacing: DuckSpace.sm) {
                        bigP2(0, icon: "hand.raised.fill") { controller.pet() }
                        bigP2(2, icon: Icons.quack) {
                            if let core = controller.core {
                                controller.voice.play(core: core, tag: "chirp")
                            }
                        }
                        bigP2(3, icon: "figure.wave") { controller.sit() }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .trailing)
                    .padding(.trailing, DuckSpace.sm)
                }
            }
            .onAppear { controller.setDirector(surface == .director) }
            .onChange(of: surface) { _, newValue in
                controller.setDirector(newValue == .director)
            }
        } else {
            Color.duckStageInstrument
                .ignoresSafeArea()
        }
        if surface == .director {
            // The bars and the shutter are chrome; the stage behind them keeps
            // the drive gesture. The old blanket `allowsHitTesting(false)` is
            // gone because the shutter is a real control.
            DirectorOverlay(controller: controller)
        }
    }



    // MARK: - Driving
    /// Tap in front of the duck to walk there, either side to turn. The tap is
    /// turned into the twist the real pad would send, and released when the
    /// finger lifts — the duck stops when you stop asking.
    ///
    /// While the duck is held, the same drag moves the hand instead: sideways
    /// across the room, up and down off the floor. The gait never sees it —
    /// a hand is an external force, and the pickup classifier decides what the
    /// policy does about it.
    private func driveGesture(in size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                // Portrait's own guard, for its landscape twin: a ring open
                // on top means no driving underneath it.
                guard controller.ready, routeState.overlay != .ring else { return }
                if controller.carrying {
                    // Measure from the grab, not from a drag that started
                    // while the duck was still on the floor.
                    let start = carryAnchor ?? gesture.startLocation
                    carryAnchor = start
                    let dx = (gesture.location.x - start.x) / (size.width * 0.5)
                    let dy = (gesture.location.y - start.y) / (size.height * 0.5)
                    controller.carry(dx: Double(dx) * 0.35, dy: 0, dz: Double(-dy) * 0.30)
                    return
                }
                let center = CGPoint(x: size.width * 0.5, y: size.height * 0.52)
                let dx = (gesture.location.x - center.x) / (size.width * 0.5)
                let dy = (gesture.location.y - center.y) / (size.height * 0.5)
                switch stickMode {
                case .head:
                    // The pad's head mode: up looks up, sideways turns, and
                    // releasing puts the head back.
                    controller.setHead(yaw: Double(dx) * 1.2, pitch: Double(dy) * 0.6)
                case .body:
                    // Body-pose mode: up stands tall, down crouches, sideways
                    // leans. Releasing releases the pose rather than freezing
                    // a lean, which is the pad's own mode exit.
                    controller.setBody(z: Double(-dy) * 0.05, roll: Double(dx) * 0.12)
                case .path:
                    // The stage is a canvas: a drag paints the draft route.
                    // A route already walking is stopped from its own chip,
                    // not by drawing under the duck.
                    if !controller.pathActive {
                        recordPathPoint(gesture.location)
                    }
                case .drive:
                    // Forward is up the screen: the duck walks away from you.
                    // Drag below the centre to walk backwards, the pad's stick
                    // does.
                    let forward = -dy
                    controller.joystick = CGPoint(
                        x: abs(dx) > 0.08 ? dx : 0,
                        y: abs(forward) > 0.08 ? -forward : 0
                    )
                }
            }
            .onEnded { _ in
                // A held duck stays in the hand where the drag left it; the
                // walk command is what must stop with the finger.
                carryAnchor = nil
                guard !controller.carrying else { return }
                controller.joystick = .zero
                switch stickMode {
                case .head:
                    controller.setHead(yaw: 0, pitch: 0)
                case .body:
                    controller.releaseBody()
                case .drive, .path:
                    break
                }
            }
    }

    // MARK: - Chrome
    /// The one-time gesture hint. It lives in the chrome column, not as an
    /// overlay with a magic bottom offset: any change in bar height or safe
    /// area would otherwise move it onto a button.
    private var hintCapsule: some View {
        Text(Copy.firstLaunchHint)
            .font(.duckCaption.weight(.medium))
            .foregroundStyle(.white.opacity(0.85))
            .padding(.horizontal, DuckSpace.md)
            .padding(.vertical, DuckSpace.sm)
            .background(.ultraThinMaterial, in: Capsule())
            .allowsHitTesting(false)
            .padding(.bottom, DuckSpace.xs)
            .transition(.opacity)
            .accessibilityIdentifier("first-launch-hint")
    }
    /// The stage literal, split out of the body expression: once the route
    /// table's sheet/onChange modifiers joined the ZStack, Xcode stopped
    /// type-checking the body in reasonable time. The function's opaque
    /// return type keeps the literal out of that inference.
    private func stage(_ geometry: GeometryProxy, portrait: Bool) -> some View {
            ZStack {
                // The drive gesture lives ON the scene rather than on a
                // full-screen clear layer: a separate hit-testing layer above
                // a UIView-backed Metal view is what swallowed the bars'
                // presses on a device.
                scene
                    .contentShape(Rectangle())
                    // D4: the stage is one accessible element with the four
                    // actions a VoiceOver user needs to drive the duck.
                    // SwiftUI on iOS has no `.accessibilityCustomActions` —
                    // that is UIKit's API. Four named actions are the SwiftUI
                    // spelling and compile from iOS 14 on.
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(Copy.duckStage)
                    .accessibilityAction(named: Text(Copy.walkStep)) {
                        pulseDrive(0, -0.6)
                    }
                    .accessibilityAction(named: Text(Copy.turnLeft)) {
                        pulseDrive(-0.6, 0)
                    }
                    .accessibilityAction(named: Text(Copy.turnRight)) {
                        pulseDrive(0.6, 0)
                    }
                    .accessibilityAction(named: Text(Copy.petIt)) {
                        controller.pet()
                        DuckHaptics.pet()
                    }
                    // Both gestures stay installed; the mask picks the one
                    // for this composition. A ternary of AnyGesture erased
                    // two different `some Gesture` types and failed to infer.
                    .gesture(
                        driveGesture(in: geometry.size),
                        including: portrait ? .none : .all
                    )
                    .gesture(
                        portraitDriveGesture(in: geometry.size),
                        including: portrait ? .all : .none
                    )
                    .onLongPressGesture(minimumDuration: 0.35) {
                        if controller.ready && controller.renderError == nil {
                            setRing(true)
                        }
                    }
                // The route being drawn, in the same screen space as the
                // gesture that records it. It takes no touches, and it sits
                // under the chrome and the skill ring.
                if stickMode == .path && !pathDraft.isEmpty {
                    PathDraftOverlay(points: pathDraft)
                }
                // Direction-A play: the ripple of every sound the duck
                // makes. Above the scene, below the chrome, so the wave
                // reaches the duck without covering the controls.
                VoiceRipple(isSpeaking: controller.isSpeaking).zIndex(1)
                if portrait {
                    portraitChrome(in: geometry.size)
                        .zIndex(2)
                } else {
                    chrome(in: geometry.size)
                        .zIndex(2)
                }
                if routeState.overlay == .ring {
                    SkillRing(
                        names: controller.skillNames(),
                        bottomAnchored: portrait
                    ) { name in
                        controller.trigger(name)
                        DuckHaptics.selection()
                        setRing(false)
                    } onClose: {
                        setRing(false)
                    }
                    // Above the chrome: the portrait deck and the landscape
                    // bar carry zIndex(2), and the bottom-anchored picker sat
                    // beneath them — its lower half was covered and taps
                    // landed on the deck instead of the skill.
                    .zIndex(5)
                }
                // 核要几秒钟才起得来；在那之前控件看着是活的，其实每一下
                // 都是空操作。用一只鸭子说这件事，不用转圈：里面有个东西
                // 正在醒。失败时例外——失败不能被这句承诺盖住。
                if !controller.ready && failureMessage == nil {
                    WakingView()
                        .allowsHitTesting(false)
                        .zIndex(3)
                }
                if Voice.diagnosticsEnabled {
                    // A launch-argument-only probe for the voice regression
                    // test: the session, the engine and the count of sounds
                    // that really reached AVAudioEngine. It is absent from
                    // the product UI and takes no touches.
                    Text(controller.voiceLine)
                        .font(.duckMicro.weight(.regular))
                        .foregroundStyle(.white.opacity(0.55))
                        .lineLimit(1)
                        .minimumScaleFactor(0.2)
                        .allowsHitTesting(false)
                        .accessibilityIdentifier("duck-voice-state")
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                        .padding(DuckSpace.xxs)
                        .zIndex(4)
                }
            }
    }

    /// A 250 ms joystick pulse — the same nudge a tap sends, exposed as an
    /// accessibility action. A newer touch owns the stick; this never fights
    /// it (the token matches what was set).
    private func pulseDrive(_ x: CGFloat, _ y: CGFloat) {
        guard controller.ready else { return }
        controller.joystick = CGPoint(x: x, y: y)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            if controller.joystick == CGPoint(x: x, y: y) {
                controller.joystick = .zero
            }
        }
    }

    private func presentHint() {
        guard !hintShown else { return }
        hintShown = true
        withDuckMotion(DuckMotion.base) { showHint = true }
        DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
            withDuckMotion(DuckMotion.slow) { showHint = false }
        }
    }

    // MARK: - Portrait shell (Stage Deck)

    /// 单手构图：状态在上（只读），舞台在中，拇指下面是 48 pt 时间条和
    /// 82 pt 托盘（4 个动词 + 更多）。
    /// 横屏走 `chrome(in:)`，两边共用同一张失败卡片、同一行说人话的状态、
    /// 同一套语义字号。
    private func portraitChrome(in size: CGSize) -> some View {
        // 核失败的时候，以前只剩一块深灰空屏和一排看着还活着的死按钮。
        // 现在把发生了什么说出来，并把还不能干活的控件压灰。
        let failure = failureMessage
        let usable = controller.ready && failure == nil
        return VStack(spacing: 0) {
            titleBar()
            if let failure {
                Spacer(minLength: DuckSpace.md)
                failureCard(failure)
                Spacer(minLength: DuckSpace.md)
            } else {
                Spacer(minLength: 0)
                    // The stage keeps the drive gesture; the chrome above it
                    // must not swallow it.
                    .allowsHitTesting(false)
                if stickMode == .path {
                    pathChips(in: size)
                        .disabled(!usable)
                        .opacity(usable ? 1 : 0.4)
                }
                if showHint {
                    hintCapsule
                } else if let mood = moodText {
                    moodPill(mood)
                } else if usable {
                    // 只说一种心情，不带百分比：数字住在仪表盘里，
                    // 想读的人自己去看。
                    moodPill(Copy.moodLine(
                        hunger: controller.careHunger,
                        mood: controller.careMood,
                        asleep: controller.careAsleep
                    ))
                }
            }
            DuckShelf(
                hunger: controller.careHunger,
                carrying: controller.carrying,
                sheetClosed: routeState.sheet == nil,
                lastOpener: lastSheetOpener,
                stageFocus: $stageFocus,
                onFeed: {
                    controller.feed()
                    DuckHaptics.medium()
                },
                onSkills: { setRing(true) },
                onQuack: quackTapped,
                onGrab: {
                    if controller.carrying { controller.release() } else { controller.grab() }
                },
                onMore: { open(.more, from: .more) },
                wheee: AnyGesture(wheeeGesture.map { _ in true })
            )
            .disabled(!usable)
            .opacity(usable ? 1 : 0.4)
        }
        .padding(.horizontal, DuckSpace.lg)
        .padding(.top, DuckSpace.sm)
        .padding(.bottom, DuckSpace.sm)
        // Direction A: the wall clock is the time machine and the brass
        // barometer is the readings; both float over the room, not a rail.
        .overlay(alignment: .topTrailing) {
            VStack(spacing: DuckSpace.sm) {
                DuckWallClock(
                    focusActive: routeState.sheet == nil && lastSheetOpener == .time,
                    stageFocus: $stageFocus
                ) { openInstrument() }
                    .simultaneousGesture(timeHoldGesture)
                DuckWallGauge { open(.instrument, from: .time) }
            }
            .offset(y: 56)
            .padding(.trailing, DuckSpace.sm)
            .disabled(!usable)
            .opacity(usable ? 1 : 0.4)
        }
    }

    /// The whole story layer from the companion direction, reduced to one
    /// line: what happened while you were away, or what the duck feels now.
    private func moodPill(_ text: String) -> some View {
        Text(text)
            .font(.duckCaption.weight(.medium))
            .foregroundStyle(.white.opacity(0.85))
            .multilineTextAlignment(.center)
            .lineLimit(2)
            .padding(.horizontal, DuckSpace.md)
            .padding(.vertical, DuckSpace.xs)
            .background(.ultraThinMaterial, in: Capsule())
            .allowsHitTesting(false)
            .padding(.bottom, DuckSpace.xs)
            .transition(.opacity)
            .accessibilityIdentifier("mood-pill")
    }

    /// The time button opens the time machine — unless the press was a hold,
    /// which is the quick freeze. The button's own action fires on the same
    /// lift, so the two have to tell each other apart.
    private func openInstrument() {
        if timeHolding || Date().timeIntervalSince(timeHoldEndedAt) < 0.6 { return }
        open(.instrument, from: .time)
    }

    /// Open a top-level destination. Rows inside a presented sheet do not
    /// come through here - the sheet host nests them - so this is a plain
    /// "replace the sheet" reduce, same as Android. `from` is the stage
    /// control that gets focus back when the sheet closes (D4).
    private func open(_ route: DuckRoute, from opener: DuckStageFocus? = nil) {
        if let opener { lastSheetOpener = opener }
        routeState = ShellOverlayReducer.reduce(routeState, .open(route))
    }

    private func setRing(_ on: Bool) {
        routeState = ShellOverlayReducer.reduce(routeState, on ? .openRing : .closeRing)
    }

    /// Long press the time button = freeze at once. `LongPressGesture.sequenced`
    /// is the one primitive whose `onEnded` sees the real lift.
    private var timeHoldGesture: some Gesture {
        LongPressGesture(minimumDuration: 0.45)
            .sequenced(before: DragGesture(minimumDistance: 0))
            .onChanged { value in
                if case .second(true, _) = value, !timeHolding {
                    timeHolding = true
                    controller.setPaused(true)
                }
            }
            .onEnded { _ in
                // Only a real hold arms the "that was me" window. A plain
                // tap ends the gesture too, and stamping it there would
                // swallow the tap that opens the time machine.
                if timeHolding { timeHoldEndedAt = Date() }
                timeHolding = false
            }
    }

    /// One sheet at a time. The switch lives in its own function because the
    /// body expression — every modifier on the scene — was more than the
    /// type checker could solve once the drawer joined it.
    @ViewBuilder
    private func modalSheet(_ sheet: DuckRoute, portrait: Bool) -> some View {
        DuckRouteSheets(
            route: sheet,
            portrait: portrait,
            controller: controller,
            stickMode: $stickMode,
            languageTick: $languageTick,
            photo: photoPreview,
            onDanceStep: playDanceStep,
            onOpenDirector: {
                // A sheet can hand over to Director (the postcards empty
                // state); the switch belongs to ContentView, not the sheet.
                withDuckMotion(DuckMotion.base) {
                    routeState = ShellOverlayReducer.reduce(routeState, .closeSheet)
                    surface = .director
                }
            },
            onToggleDirector: {
                toggleSurface()
                routeState = ShellOverlayReducer.reduce(routeState, .closeSheet)
            },
            surfaceIsDirector: surface == .director
        )
    }

    private func toggleSurface() {
        withDuckMotion(DuckMotion.quick) {
            surface = surface == .live ? .director : .live
        }
        DuckHaptics.selection()
    }

    /// Tap = chirp, hold 0.3 s = the official wheee ride. Shared by the
    /// landscape bar and the portrait deck so the two cannot drift.
    private func quackTapped() {
        if wheeeHolding || Date().timeIntervalSince(wheeeEndedAt) < 0.5 {
            wheeeHolding = false
            wheeeEndedAt = .distantPast
        } else if let core = controller.core {
            controller.voice.play(core: core, tag: "chirp")
            controller.dance.record("sound", value: "chirp", label: Copy.danceStepSound)
        }
    }

    /// Replay one recorded step through the same button path it was made by.
    private func playDanceStep(_ step: DanceStep) {
        switch step.kind {
        case "skill": controller.trigger(step.value)
        case "sound":
            if let core = controller.core {
                controller.voice.play(core: core, tag: step.value)
            }
        case "pet": controller.pet()
        case "feed": controller.feed()
        case "sit": controller.sit()
        case "light": controller.setFlashlight(color: step.value)
        default: break
        }
    }

    /// Portrait steering is translation-based from wherever the thumb lands:
    /// a hidden centre-origin joystick made a resting thumb command reverse,
    /// and a quick tap only stepped for as long as the finger was down.
    private func portraitDriveGesture(in size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                guard controller.ready, routeState.overlay != .ring else { return }
                pulseToken += 1
                if controller.carrying {
                    // First change after the grab is the anchor: the drag
                    // that was walking the duck must not teleport the hand.
                    let start = carryAnchor ?? gesture.startLocation
                    carryAnchor = start
                    let dx = (gesture.location.x - start.x) / (size.width * 0.5)
                    let dy = (gesture.location.y - start.y) / (size.height * 0.5)
                    controller.carry(dx: Double(dx) * 0.35, dy: 0, dz: Double(-dy) * 0.30)
                    return
                }
                if stickMode == .path {
                    // Drawing replaces driving: record the stroke and leave
                    // the joystick alone. While the core walks a route the
                    // stage is inert, and its Stop chip is the way out.
                    if !controller.pathActive {
                        recordPathPoint(gesture.location)
                    }
                    return
                }
                let distance = hypot(gesture.translation.width, gesture.translation.height)
                guard distance >= 8 else { return }
                let scale = min(distance / 90, 1)
                let nx = gesture.translation.width / distance * scale
                let ny = gesture.translation.height / distance * scale
                switch stickMode {
                case .drive:
                    // Up (negative height) is forward; the controller turns
                    // joystick.y into vx and joystick.x into -wz.
                    controller.joystick = CGPoint(x: nx, y: -ny)
                case .head:
                    // Same sign as landscape and Android: up looks up.
                    controller.setHead(yaw: Double(nx) * 1.2, pitch: Double(ny) * 0.6)
                case .body:
                    controller.setBody(z: Double(-ny) * 0.05, roll: Double(nx) * 0.12)
                case .path:
                    // Handled above: a path stroke is a draft, not a drive.
                    break
                }
            }
            .onEnded { gesture in
                guard controller.ready else { return }
                if controller.carrying {
                    carryAnchor = nil
                    return
                }
                if stickMode == .path {
                    // A stroke is a draft, not a drive: there is nothing to
                    // release, and a tap must not become a step or a pet.
                    // A drive drag that was live when the mode changed still
                    // owns the stick; hand it back neutral.
                    controller.joystick = .zero
                    return
                }
                let distance = hypot(gesture.translation.width, gesture.translation.height)
                let wasTap = distance < 8
                if wasTap && routeState.overlay != .ring {
                    // A touch on the duck strokes it; a touch on the floor
                    // ahead sends a short step or turn, not a dead no-op.
                    let insideDuck = gesture.location.x > size.width * 0.25
                        && gesture.location.x < size.width * 0.75
                        && gesture.location.y > size.height * 0.33
                        && gesture.location.y < size.height * 0.68
                    if insideDuck {
                        controller.pet()
                        DuckHaptics.pet()
                    } else if stickMode == .drive {
                        let dx = gesture.location.x - size.width * 0.5
                        let dy = gesture.location.y - size.height * 0.5
                        let norm = max(hypot(dx, dy), 1)
                        controller.joystick = CGPoint(
                            x: dx / norm * 0.6,
                            y: -dy / norm * 0.6
                        )
                        let token = pulseToken
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                            // A newer touch (a drive drag holding a direction)
                            // owns the joystick now: leave it alone.
                            guard pulseToken == token else { return }
                            controller.joystick = .zero
                        }
                    }
                } else {
                    controller.joystick = .zero
                    controller.setHead(yaw: 0, pitch: 0)
                    controller.releaseBody()
                }
            }
    }

    // MARK: - Path Painter

    /// One recorded screen point of the draft. 8 pt keeps a thumb's tremor
    /// out of the line without making it look chopped, and 64 points is the
    /// wire's own budget.
    private func recordPathPoint(_ point: CGPoint) {
        if let last = pathDraft.last {
            guard hypot(point.x - last.x, point.y - last.y) >= 8 else { return }
        }
        guard pathDraft.count < 64 else { return }
        pathDraft.append(point)
    }

    /// The draft as the wire's metre frame: +x forward, +y left, measured
    /// from the first recorded point. 2.2 m spans the stage's narrow side.
    /// The first point becomes [0,0] (the core drops it), and points under
    /// 0.05 m apart in screen terms are dropped so a shiver does not spend
    /// the route's point budget.
    private func pathBodyPoints(in size: CGSize) -> [[Double]] {
        guard let origin = pathDraft.first, size.width > 0, size.height > 0 else {
            return []
        }
        let metresPerPoint = 2.2 / Double(min(size.width, size.height))
        let minimumSpacing = 0.05 / metresPerPoint
        var points: [[Double]] = [[0, 0]]
        var previous = origin
        for point in pathDraft.dropFirst() {
            let spacing = hypot(point.x - previous.x, point.y - previous.y)
            guard Double(spacing) >= minimumSpacing else { continue }
            guard points.count < 64 else { break }
            points.append([
                -Double(point.y - origin.y) * metresPerPoint,
                -Double(point.x - origin.x) * metresPerPoint,
            ])
            previous = point
        }
        return points
    }

    /// What the core will keep, by its own rules: the first element is the
    /// dropped origin and anything under 0.1 m from the previous kept point
    /// goes. The count and the length are what Go and the 20 m cap need.
    private func keptPath(_ points: [[Double]]) -> (count: Int, metres: Double) {
        guard let first = points.first else { return (0, 0) }
        var count = 0
        var metres = 0.0
        var previous = first
        for point in points.dropFirst() {
            let distance = hypot(point[0] - previous[0], point[1] - previous[1])
            guard distance >= 0.1 else { continue }
            count += 1
            metres += distance
            previous = point
        }
        return (count, metres)
    }

    /// The Path Painter's own deck: Go walks the drawing, Clear drops it,
    /// and while the core walks a route it shows its step and a Stop. Same
    /// materials and radii as the thumb deck.
    private func pathChips(in size: CGSize) -> some View {
        let kept = keptPath(pathBodyPoints(in: size))
        // The core refuses a route with fewer than two real steps, and one
        // longer than 20 m; keep the chip from posting a route it will drop.
        let canWalk = kept.count >= 2 && kept.metres <= 20
        // One-based: the child is walking the (reached+1)-st of total steps.
        let progress = Copy.pathStep(controller.pathReached + 1, controller.pathTotal)
        return HStack(spacing: DuckSpace.sm) {
            pathChip("path-go", Copy.pathGo, "figure.walk", enabled: canWalk) {
                let points = pathBodyPoints(in: size)
                guard !points.isEmpty else { return }
                controller.postPath(points: points)
                // The drawing stays until the core says the walk started: a
                // refused POST must not silently eat the child's route.
            }
            pathChip("path-clear", Copy.pathClear, "trash", enabled: !pathDraft.isEmpty) {
                pathDraft.removeAll()
            }
            if pathPostcardReady {
                pathChip("path-postcard", Copy.pathPostcard, "camera", enabled: true) {
                    pathPostcardReady = false
                    controller.captureDirectorPhoto()
                }
            }
            Spacer(minLength: 0)
            if controller.pathActive {
                Text(progress)
                    .font(.duckCaption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.85))
                    .lineLimit(2)
                    .accessibilityLabel(progress)
                    .accessibilityIdentifier("path-progress")
                pathChip("path-cancel", Copy.pathCancel, "xmark", enabled: true) {
                    controller.cancelPath()
                }
            }
        }
        .padding(.horizontal, DuckSpace.sm)
        .padding(.vertical, DuckSpace.xs)
        .background(
            Color.duckBgSurface.opacity(0.94),
            in: RoundedRectangle(cornerRadius: DuckRadius.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: DuckRadius.panel)
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
        .padding(.bottom, DuckSpace.xs)
    }

    private func pathChip(
        _ id: String,
        _ label: String,
        _ icon: String,
        enabled: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: DuckSpace.xs) {
                Image(systemName: icon)
                    .font(DuckIcon.xs)
                Text(label)
                    .font(.duckCaption.weight(.semibold))
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
            }
            .padding(.horizontal, DuckSpace.md)
            .frame(minHeight: 44)
            .background(.ultraThinMaterial, in: Capsule())
            .contentShape(Capsule())
            .opacity(enabled ? 1 : 0.45)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
    }

    @ViewBuilder
    private func chrome(in size: CGSize) -> some View {
        // 和竖屏同一条规矩：失败要有话说，身后的控件不许假装
        // 还能驱动一只没起来的鸭子。
        let failure = failureMessage
        let usable = controller.ready && failure == nil
        VStack(spacing: 0) {
            topBar()
            Spacer()
            if let failure {
                failureCard(failure)
            } else {
                // No card: the space between the bars stays transparent to taps.
                Color.clear.frame(height: 0)
            }
            if showHint { hintCapsule }
            if stickMode == .path {
                pathChips(in: size)
                    .disabled(!usable)
                    .opacity(usable ? 1 : 0.4)
            }
            bottomBar
                // Until the core is ready every control is a no-op; dim and
                // disable them rather than letting them look alive.
                .disabled(!usable)
                .opacity(usable ? 1 : 0.4)
        }
        .padding(.horizontal, DuckSpace.lg)
        .padding(.top, DuckSpace.sm)
        .padding(.bottom, DuckSpace.md)
        .allowsHitTesting(true)
    }

    private var failureMessage: String? {
        if let error = controller.errorMessage { return error }
        if let error = controller.renderError { return error }
        return nil
    }

    /// 名字下面那一行：活着时是鸭子此刻的状态，核加载时是正在叫醒它的
    /// 承诺，起不来时是一句诚实的结论。原因由舞台下方的卡片来说。
    private var statusLine: String {
        if failureMessage != nil { return Copy.wakeFailed }
        // A frozen duck says so. The top bar must not keep reporting the
        // core's own label while the clock is stopped.
        if controller.paused { return Copy.freeze }
        if controller.ready { return Copy.behaviourOf(raw: controller.label) }
        return Copy.waking
    }

    /// The portrait top area is read-only (B4 §1): the duck's name and what
    /// it is doing, in words rather than numbers. Every menu that used to
    /// hide in the right-hand corner moved into the More drawer.
    private func titleBar() -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.xxs) {
            Text(Copy.name)
                .font(.duckBody.weight(.semibold))
            Text(statusLine)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("duck-label")
            if !controller.lastSkill.isEmpty {
                Text(SkillLabel.of(controller.lastSkill))
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("duck-last-skill")
            }
            if controller.fallen {
                Text(Copy.fell)
                    .font(.duckCaption.weight(.semibold))
                    .foregroundStyle(.orange)
            }
            if controller.pickedUp && !controller.carrying {
                Text(Copy.held)
                    .font(.duckCaption.weight(.semibold))
                    .foregroundStyle(.cyan)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func topBar(showSurface: Bool = true, showProgress: Bool = false) -> some View {
        HStack(alignment: .top) {
            // The duck's name and what it is doing, in words rather than numbers.
            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                Text(Copy.name)
                    .font(.duckBody.weight(.semibold))
                // 核的 label 是协议原文（`held`、`limp_fall`、`mode: walk`）；
                // 人读翻译，不读 wire。仪表盘里还留着原话。
                Text(statusLine)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("duck-label")
                // The ring's verdict outlives the skill's brief label. A slow
                // CI runner must be able to read it after the move finishes.
                if !controller.lastSkill.isEmpty {
                    Text(SkillLabel.of(controller.lastSkill))
                        .font(.duckCaption)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("duck-last-skill")
                }
                if controller.fallen {
                    Text(Copy.fell)
                        .font(.duckCaption.weight(.semibold))
                        .foregroundStyle(.orange)
                }
                if controller.pickedUp && !controller.carrying {
                    Text(Copy.held)
                        .font(.duckCaption.weight(.semibold))
                        .foregroundStyle(.cyan)
                }
            }
            Spacer()
            // The camera is one tap, not one menu deep: framing the duck with
            // the phone is the whole point of Director, and it was the one
            // chip worth keeping on the bar. In portrait it moves down to the
            // time rail, inside the thumb's arc.
            if showSurface {
                Button {
                    withDuckMotion(DuckMotion.quick) {
                        surface = surface == .live ? .director : .live
                    }
                } label: {
                    Image(systemName: surface == .live ? "viewfinder" : "circle.dashed")
                        .font(DuckIcon.md)
                        .frame(width: 48, height: 48)
                        .background(.ultraThinMaterial, in: Circle())
                        .contentShape(Circle())
                }
                .accessibilityLabel(surface == .live ? Copy.director : Copy.live)
                .accessibilityIdentifier("surface-toggle")
                .accessibilityFocused($stageFocus, equals: .surface)
            }
            // One menu, not six chips. On a landscape phone the chips ran off
            // the edge and nothing was legible; the stick modes, the surface,
            // the language and the flashlight now live behind one button a
            // thumb can open.
            if showProgress {
                // 每日任务与成就墙：竖屏主力形态的入口。放在顶栏而不是时间
                // 栏：时间栏的进度条不能被第五个钮挤成一条缝。
                Button {
                    open(.progress)
                } label: {
                    Image(systemName: "flame")
                        .font(DuckIcon.md)
                        .frame(width: 48, height: 48)
                        .background(.ultraThinMaterial, in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.progress)
                .accessibilityIdentifier("duck-progress")
            }
            LandscapeMenu(
                controller: controller,
                stickMode: $stickMode,
                surface: $surface,
                onOpen: { open($0) }
            )
        }
        .padding(DuckSpace.sm)
        // No full-width material panel: it painted a light band across the
        // top ~16% of the stage and broke the room's vertical gradient
        // (review 2026-11-13, 打回 1). The title is white on the dark room;
        // the flame/menu buttons carry their own material circles.
    }

    private var bottomBar: some View {
        HStack(spacing: DuckSpace.sm) {
            // The time machine lives one pull away, not on the screen.
            Button {
                open(.instrument, from: .time)
            } label: {
                Label(Copy.time, systemImage: "clock.arrow.circlepath")
                    .labelStyle(.iconOnly)
                    .font(.duckTitle)
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.time)
            .accessibilityIdentifier("duck-time")
            .accessibilityFocused($stageFocus, equals: .time)

            Button {
                setRing(true)
            } label: {
                Label(Copy.skills, systemImage: "sparkles")
                    .labelStyle(.iconOnly)
                    .font(.duckTitle)
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.skills)
            .accessibilityIdentifier("duck-skills")

            // The duck's own voice: `robotctl quack` is a chirp.
            Button {
                // Held for 0.3 s it is the joy-ride instead; `quackTapped`
                // keeps the tap and the ride from drifting apart between
                // the two compositions.
                quackTapped()
            } label: {
                Label(Copy.quack, systemImage: "waveform")
                    .labelStyle(.iconOnly)
                    .font(.duckTitle)
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .simultaneousGesture(wheeeGesture)
            .accessibilityLabel(Copy.quack)
            .accessibilityIdentifier("duck-quack")

            // 今日任务与成就墙：每天回来的理由。
            Button {
                open(.progress)
            } label: {
                Label(Copy.progress, systemImage: "flame")
                    .labelStyle(.iconOnly)
                    .font(.duckTitle)
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.progress)
            .accessibilityIdentifier("duck-progress")

            // A treat on the floor: the official ground-pick peck.
            Button {
                controller.feed()
            } label: {
                Label(Copy.feed, systemImage: "fork.knife")
                    .labelStyle(.iconOnly)
                    .font(.duckTitle)
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.feed)
            .accessibilityIdentifier("duck-feed")

            // The face's flashlight, where a hand would reach: a press
            // toggles, white, as `robot.flashlight` does.
            Button {
                controller.toggleFlashlight()
            } label: {
                Label(Copy.flashlight, systemImage: "flashlight.on.fill")
                    .labelStyle(.iconOnly)
                    .font(.duckTitle)
                    .frame(width: 48, height: 48)
                    .background(
                        controller.flashlightColor == nil
                            ? AnyShapeStyle(.ultraThinMaterial)
                            : AnyShapeStyle(Color.white.opacity(0.32)),
                        in: Circle()
                    )
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.flashlight)
            .accessibilityIdentifier("duck-flashlight")

            Button {
                if controller.carrying {
                    controller.release()
                } else {
                    controller.grab()
                }
            } label: {
                Label(
                    controller.carrying ? Copy.putDown : Copy.grab,
                    systemImage: controller.carrying ? "hand.raised.slash.fill" : "hand.draw.fill"
                )
                .labelStyle(.iconOnly)
                .font(.duckTitle)
                .frame(width: 48, height: 48)
                .background(
                    controller.carrying
                        ? AnyShapeStyle(Color.white.opacity(0.32))
                        : AnyShapeStyle(.ultraThinMaterial),
                    in: Circle()
                )
                .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(controller.carrying ? Copy.putDown : Copy.grab)
            .accessibilityIdentifier("duck-grab")

            // The block program, one tap from the stage: a child should not
            // have to find it in a menu.
            Button {
                open(.rules)
            } label: {
                Label(Copy.blocks, systemImage: "square.grid.2x2.fill")
                    .labelStyle(.iconOnly)
                    .font(DuckIcon.md)
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.blocks)
            .accessibilityIdentifier("duck-rules")

            Spacer()

            Text(controller.carrying ? Copy.held : Copy.tapHint)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
                .opacity(controller.ready ? 1 : 0)
                .allowsHitTesting(false)
        }
    }

    /// The held joy-ride: a long press feeds into a drag that lasts as long
    /// as the finger stays down. `LongPressGesture.sequenced(before:)` is the
    /// one SwiftUI primitive whose `onEnded` sees the real lift, so the ride
    /// cannot outlive the finger.
    private var wheeeGesture: some Gesture {
        LongPressGesture(minimumDuration: 0.3)
            .sequenced(before: DragGesture(minimumDistance: 0, coordinateSpace: .local))
            .onChanged { value in
                // Only the sequence's second stage carries the drag; the
                // first is just the press itself.
                guard case .second(true, _) = value else { return }
                if !wheeeHolding {
                    wheeeHolding = true
                    controller.setWheee(true)
                }
            }
            .onEnded { value in
                guard case .second(_, let drag) = value else { return }
                // The drag's end is the finger lift this gesture exists to
                // see. `wheeeEndedAt` lets the Button action — which fires on
                // this same lift — recognise the release and keep quiet.
                if drag != nil || wheeeHolding {
                    wheeeHolding = false
                    wheeeEndedAt = Date()
                    controller.setWheee(false)
                }
            }
    }

    /// 唯一的失败出口，横竖屏共用。诊断信息要翻成人话：一个人对
    /// "Bundle resources not found" 做不了任何事，读懂了也唤不醒鸭子。
    private func failureCard(_ message: String) -> some View {
        VStack(spacing: DuckSpace.sm) {
            Image(systemName: "exclamationmark.bubble.fill")
                .font(DuckIcon.lg)
                .foregroundStyle(.orange.opacity(0.9))
            Text(Copy.couldNotStart)
                .font(.duckBody.weight(.semibold))
            Text(Copy.humanError(raw: message))
                .font(.duckLabel)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button {
                controller.retryStartup()
            } label: {
                Label(Copy.retry, systemImage: "arrow.clockwise")
                    .font(.duckLabel.weight(.semibold))
                    .frame(minWidth: 96, minHeight: 44)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("failure-retry")
        }
        .padding(DuckSpace.xl)
        .frame(maxWidth: 360)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: DuckRadius.card))
        .padding(.vertical, DuckSpace.xxl)
        // Contain, not combine: the retry button must stay its own element.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("failure-card")
    }
}

/// 一个人第一眼看到的东西：不是盖在死屏上的转圈，而是一只鸭子形状的
/// 东西在暗处呼吸，等核醒过来。故意半透明——它是承诺，不是肖像。
struct WakingView: View {
    /// 尊重手机的设置：呼吸剪影是装饰，不是信息，而装饰是第一个该停的。
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// 一次慢呼吸：2.6 秒吸，2.6 秒呼。一只睡在地板上的鸭子差不多就动这么多。
    @State private var breathing = false

    var body: some View {
        VStack(spacing: DuckSpace.lg) {
            ZStack {
                // 它将站上的影子，跟着一起呼吸。
                Ellipse()
                    .fill(Color.black.opacity(0.35))
                    .frame(width: 96, height: 14)
                    .offset(y: 38)
                // 身体：一只低低的椭圆，像鸭子坐在自己的脚上。
                Ellipse()
                    .fill(Color.orange.opacity(0.42))
                    .frame(width: 88, height: 58)
                    .offset(y: 8)
                // 头，往前探一点、抬一点。
                Circle()
                    .fill(Color.orange.opacity(0.52))
                    .frame(width: 46, height: 46)
                    .offset(x: 27, y: -22)
                // 嘴：整个剪影里唯一一条硬边。
                Beak()
                    .fill(Color.orange.opacity(0.75))
                    .frame(width: 16, height: 12)
                    .offset(x: 54, y: -18)
            }
            // 给影子和头留出地方，呼吸时不会压到下面那行字。
            .frame(width: 120, height: 96)
            .scaleEffect(breathing ? 1.05 : 0.96, anchor: .bottom)
            .opacity(breathing ? 0.95 : 0.6)
            .animation(
                reduceMotion
                    ? nil
                    : .easeInOut(duration: 2.6).repeatForever(autoreverses: true),
                value: breathing
            )

            Text(Copy.waking)
                .font(.duckBody)
                .foregroundStyle(.white.opacity(0.75))
        }
        .onAppear { breathing = true }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Copy.waking)
    }
}

/// 鸭子拿来当鼻子用的小楔子。
private struct Beak: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.minX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.closeSubpath()
        return path
    }
}

/// The route being drawn on the stage, in the gesture's own screen space:
/// the shell's only drawing, since the duck and the room are Metal's.
struct PathDraftOverlay: View {
    let points: [CGPoint]

    var body: some View {
        Canvas { context, _ in
            guard let first = points.first else { return }
            var line = Path()
            line.move(to: first)
            for point in points.dropFirst() {
                line.addLine(to: point)
            }
            context.stroke(
                line,
                with: .color(Color.duckData.opacity(0.85)),
                style: StrokeStyle(lineWidth: 3, lineCap: .round, lineJoin: .round)
            )
            // The origin: the duck stands here when the route starts.
            let dot = CGRect(x: first.x - 4, y: first.y - 4, width: 8, height: 8)
            context.fill(
                Path(ellipseIn: dot),
                with: .color(Color.duckData.opacity(0.85))
            )
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// The skills picker: every verb `/skills` publishes, grouped into the moves
/// the duck has always had and `robotd`'s emotions. Twenty entries would pile
/// up on a circle, so they live in a scrolling panel instead — same material
/// capsules, same tap-outside-to-close overlay.
struct SkillRing: View {
    let names: [String]
    /// Portrait presents the picker as a bottom deck so the duck above it
    /// stays visible; landscape keeps the centred card.
    var bottomAnchored = false
    let onPick: (String) -> Void
    let onClose: () -> Void

    /// `robotd`'s emotion verbs. Everything else — the locomotion verbs, the
    /// one-shots and any skill a user manifest adds — is an action.
    private static let emotionNames: Set<String> = [
        "angry", "curious", "defiant", "devastated", "excited", "impatient",
        "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
    ]

    private var actions: [String] {
        names.filter { !Self.emotionNames.contains($0) }
    }

    private var emotions: [String] {
        names.filter { Self.emotionNames.contains($0) }
    }

    var body: some View {
        ZStack(alignment: bottomAnchored ? .bottom : .center) {
            Color.black.opacity(0.45)
                .ignoresSafeArea()
                .onTapGesture(perform: onClose)
            picker
                .padding(.bottom, bottomAnchored ? 12 : 0)
                .transition(
                    bottomAnchored
                        ? .move(edge: .bottom).combined(with: .opacity)
                        : .scale.combined(with: .opacity)
                )
        }
    }

    private var picker: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack(spacing: DuckSpace.sm) {
                Image(systemName: "sparkles")
                    .font(DuckIcon.sm)
                    .foregroundStyle(.secondary)
                Text(Copy.skills)
                    .font(.duckBody.weight(.semibold))
                Spacer()
                Button(action: onClose) {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("ring-close")
            }
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: DuckSpace.lg) {
                    if !actions.isEmpty {
                        section(Copy.actions, names: actions)
                    }
                    if !emotions.isEmpty {
                        section(Copy.emotions, names: emotions)
                    }
                }
                .padding(.bottom, DuckSpace.xs)
            }
            // Landscape's safe height is about 372 pt; the old fixed 360 pt
            // scroll plus header and padding clipped both ends. Portrait has
            // room for the whole palette.
            .frame(maxHeight: bottomAnchored ? 430 : 240)
        }
        .padding(DuckSpace.lg)
        .frame(maxWidth: 360)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: DuckRadius.panel))
        .padding(.horizontal, DuckSpace.xxl)
    }

    /// One group of capsules: a small heading, then an adaptive grid so the
    /// English wire names still fit three to a row.
    private func section(_ title: String, names: [String]) -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            Text(title)
                .font(.duckCaption.weight(.semibold))
                .tracking(0.8)
                .foregroundStyle(.secondary)
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 88), spacing: DuckSpace.sm)],
                alignment: .leading,
                spacing: DuckSpace.sm
            ) {
                ForEach(names, id: \.self) { name in
                    Button {
                        onPick(name)
                    } label: {
                        Text(SkillLabel.of(name))
                            .font(.duckLabel.weight(.medium))
                            .lineLimit(2)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, DuckSpace.sm)
                            .padding(.vertical, DuckSpace.sm)
                            .frame(maxWidth: .infinity, minHeight: 48)
                            .background(.ultraThinMaterial, in: Capsule())
                    }
                    .buttonStyle(.plain)
                    // The picker's entries share their names with nothing
                    // else, but an accessibility id keeps the UI test's query
                    // unambiguous — and every entry is reachable by scrolling.
                    .accessibilityIdentifier("ring-\(name)")
                }
            }
        }
    }
}

/// The director's frame: a letterbox, a horizon and the record state. It never
/// covers the duck, and it takes no touches.
struct DirectorOverlay: View {
    let controller: DuckController

    var body: some View {
        GeometryReader { geometry in
            let bar = geometry.size.height * 0.055
            VStack {
                Rectangle()
                    .fill(Color.black.opacity(0.55))
                    .frame(height: bar)
                Spacer()
                Rectangle()
                    .fill(Color.black.opacity(0.55))
                    .frame(height: bar)
            }
            .overlay(alignment: .topLeading) {
                HStack(spacing: DuckSpace.xs) {
                    Circle()
                        .fill(controller.cameraActive ? Color.red : Color.orange)
                        .frame(width: 7, height: 7)
                    // With no camera (simulator, denied permission, a device
                    // without one) Director is a dark studio, not a room.
                    // Say that instead of drawing a viewfinder over it.
                    Text(controller.cameraActive ? Copy.viewfinder : Copy.cameraUnavailable)
                        .font(.duckCaption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.85))
                }
                .padding(.top, bar / 2 - 5)
                .padding(.leading, DuckSpace.md)
            }
            .overlay(alignment: .bottomTrailing) {
                // 取景器的时间码，不是仿真的 tick：构图的人要的是几点，
                // 不是引擎数到哪了。
                Text(Date.now, style: .time)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.7))
                    .padding(.bottom, bar / 2 - 5)
                    .padding(.trailing, DuckSpace.md)
            }
            .overlay(alignment: .trailing) {
                // The one shutter: the frame already on the screen is the photo.
                // Vertically centred, not top-trailing: in landscape the top
                // bar's menu sits exactly where a top-trailing shutter would
                // be, and the menu would eat the tap.
                Button {
                    controller.captureDirectorPhoto()
                } label: {
                    ZStack {
                        Circle()
                            .fill(Color.white.opacity(0.92))
                            .frame(width: 64, height: 64)
                            .shadow(color: .black.opacity(0.35), radius: 8, y: 2)
                        Circle()
                            .stroke(Color.black.opacity(0.18), lineWidth: 2)
                            .frame(width: 52, height: 52)
                        Image(systemName: "camera.fill")
                            .font(DuckIcon.md)
                            .foregroundStyle(.black)
                    }
                    .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.photoTake)
                .accessibilityIdentifier("duck-photo")
                .padding(.trailing, DuckSpace.lg)
            }
        }
    }
}

/// One second-player verb: the same calls the big-button page makes,
/// sized for the right edge of a portrait stage.
fileprivate extension ContentView {
    func bigP2(_ index: Int, icon: String, action: @escaping () -> Void) -> some View {
        Button {
            action()
        } label: {
            Image(systemName: icon)
                .font(DuckIcon.lg)
                .frame(width: 56, height: 56)
                .background(Color.black.opacity(0.45), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Copy.bigButtonAction(index))
        .accessibilityIdentifier("p2-action-\(index)")
    }
}
