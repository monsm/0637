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
    @State private var showRing = false
    /// The pad's stick modes, reduced to the three a phone can express:
    /// walk/turn, look, and crouch/lean the standing body.
    @State private var stickMode = StickMode.drive
    @Environment(\.scenePhase) private var scenePhase
    /// The one-time gesture hint. The flag is persisted before the caption
    /// appears, so a crash or a force-quit can never show it a second time.
    /// `showHint` is separate so the fade-out cannot undo that write.
    @AppStorage("first_launch_hint_shown") private var hintShown = false
    @State private var showHint = false

    enum StickMode: String, CaseIterable {
        case drive, head, body
    }
    @State private var showInstrument = false
    /// The child's block program, edited over the live duck.
    @State private var showRules = false
    /// Other ducks on the Wi-Fi.
    @State private var showPeers = false
    /// The user's own model.
    @State private var showAI = false
    /// The child's choreography recorder.
    @State private var showDance = false
    /// One sheet at a time. SwiftUI's multiple `.sheet` modifiers on one view
    /// are not reliable; a single item sheet cannot disagree with itself.
    enum Modal: Identifiable {
        case instrument
        case photo(DuckController.CapturedPhoto)
        case rules
        case peers
        case ai
        case dance

        var id: String {
            switch self {
            case .instrument: return "instrument"
            case .photo(let photo): return "photo-\(photo.id)"
            case .rules: return "rules"
            case .peers: return "peers"
            case .ai: return "ai"
            case .dance: return "dance"
            }
        }
    }
    @State private var modal: Modal?
    @State private var tapTarget: CGPoint?
    @State private var lastTap = Date.distantPast
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

    enum Surface: String, CaseIterable {
        case live = "Live"
        case director = "Director"
    }

    var body: some View {
        let _ = languageTick
        GeometryReader { geometry in
            // Portrait is the product; landscape is the two-hand instrument
            // and demo layout. A width under 600 keeps phones (and a narrow
            // split view) in the one-thumb shell; wider surfaces keep the
            // already-tested landscape chrome.
            let portrait = geometry.size.height > geometry.size.width
                && geometry.size.width < 600
            ZStack {
                // The drive gesture lives ON the scene rather than on a
                // full-screen clear layer: a separate hit-testing layer above
                // a UIView-backed Metal view is what swallowed the bars'
                // presses on a device.
                scene
                    .contentShape(Rectangle())
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
                            showRing = true
                        }
                    }
                if portrait {
                    portraitChrome(in: geometry.size)
                        .zIndex(2)
                } else {
                    chrome(in: geometry.size)
                        .zIndex(2)
                }
                if showRing {
                    SkillRing(
                        names: controller.skillNames(),
                        bottomAnchored: portrait
                    ) { name in
                        controller.trigger(name)
                        showRing = false
                    } onClose: {
                        showRing = false
                    }
                }
                // The core takes seconds to load; until it is ready the
                // controls look alive but every call would be a no-op. Say so.
                if !controller.ready && controller.renderError == nil {
                    VStack(spacing: 10) {
                        ProgressView()
                            .tint(.white)
                        Text(Copy.waking)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.white.opacity(0.75))
                    }
                    .allowsHitTesting(false)
                    .zIndex(3)
                }
            }
            // Halfway through a rotation the thumb's old translation means
            // something else; hand the duck back neutral, once.
            .onChange(of: portrait) { _, _ in
                controller.joystick = .zero
                controller.setHead(yaw: 0, pitch: 0)
                controller.releaseBody()
                wheeeHolding = false
                controller.setWheee(false)
            }
            // The 3D scene fills the screen (it owns its own ignoresSafeArea),
            // but the chrome stays inside the safe area: on a notched phone
            // the old full-screen ignore put the buttons under the home bar.
            // One modal sheet for every surface: instrument, Director photo,
            // blocks, neighbours and the user's model.
            .sheet(item: $modal) { sheet in
                switch sheet {
                case .instrument:
                    InstrumentSheet(controller: controller)
                        .presentationDetents(portrait ? [.medium, .large] : [.large])
                        .presentationDragIndicator(.visible)
                case .photo(let photo):
                    PhotoPreviewSheet(
                        photo: photo,
                        shelf: controller.postcards
                    )
                case .rules:
                    RulesSheet(controller: controller)
                        .presentationDetents([.large])
                        .presentationDragIndicator(.visible)
                case .peers:
                    PeersSheet(controller: controller)
                        .presentationDetents(portrait ? [.medium, .large] : [.large])
                        .presentationDragIndicator(.visible)
                case .ai:
                    AISheet(controller: controller)
                        .presentationDetents(portrait ? [.medium, .large] : [.large])
                        .presentationDragIndicator(.visible)
                case .dance:
                    DanceSheet(
                        studio: controller.dance,
                        execute: playDanceStep,
                        tryStep: { kind, value in
                            let label = kind == "sound"
                                ? Copy.danceStepSound
                                : SkillLabel.of(value)
                            controller.dance.record(
                                kind,
                                value: value,
                                label: label
                            )
                            playDanceStep(DanceStep(
                                kind: kind,
                                value: value,
                                label: label
                            ))
                        }
                    )
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
                }
            }
            .onChange(of: showInstrument) { _, show in
                if show { showInstrument = false; modal = .instrument }
            }
            .onReceive(controller.$capturedPhoto) { photo in
                if let photo {
                    controller.capturedPhoto = nil
                    modal = .photo(photo)
                }
            }
            .onChange(of: showRules) { _, show in
                if show { showRules = false; modal = .rules }
            }
            .onChange(of: showPeers) { _, show in
                if show { showPeers = false; modal = .peers }
            }
            .onChange(of: showAI) { _, show in
                if show { showAI = false; modal = .ai }
            }
            .onChange(of: showDance) { _, show in
                if show { showDance = false; modal = .dance }
            }
        }
        .preferredColorScheme(.dark)
        // The hint teaches gestures, so it must wait until gestures work:
        // the core takes seconds to load, and a hint spent over a dead screen
        // is spent forever. It is shown once the duck is ready, on both
        // orientations, and its flag is only written when it is really shown.
        .onChange(of: controller.ready) { _, ready in
            if ready {
                presentHint()
                controller.restoreCompanionState()
            }
        }
        .onChange(of: controller.awaySummary) { _, summary in
            guard let summary, !summary.isEmpty else { return }
            withAnimation(.easeIn(duration: 0.2)) { moodText = summary }
            DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                guard moodText == summary else { return }
                withAnimation(.easeOut(duration: 0.5)) { moodText = nil }
            }
        }
        // A clap or a whistle is answered visibly as well as audibly: the
        // same pill the away recap uses, for a shorter spell.
        .onChange(of: controller.roomSound) { _, sound in
            guard let sound, !sound.isEmpty else { return }
            withAnimation(.easeIn(duration: 0.2)) { moodText = sound }
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
                guard moodText == sound else { return }
                withAnimation(.easeOut(duration: 0.5)) { moodText = nil }
            }
        }
        // A programmed block shows its work: "拍手 → 翻滚".
        .onChange(of: controller.ruleNotice) { _, notice in
            guard let notice, !notice.isEmpty else { return }
            withAnimation(.easeIn(duration: 0.2)) { moodText = notice }
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                guard moodText == notice else { return }
                withAnimation(.easeOut(duration: 0.5)) { moodText = nil }
            }
        }
        // A greeting from the room next door, or the answer to ours.
        .onChange(of: controller.peerNotice) { _, notice in
            guard let notice, !notice.isEmpty else { return }
            withAnimation(.easeIn(duration: 0.2)) { moodText = notice }
            DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
                guard moodText == notice else { return }
                withAnimation(.easeOut(duration: 0.5)) { moodText = nil }
            }
        }
        // The model's own line, when it decides something.
        .onChange(of: controller.aiDecision) { _, decision in
            guard !decision.isEmpty else { return }
            withAnimation(.easeIn(duration: 0.2)) { moodText = decision }
            DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
                guard moodText == decision else { return }
                withAnimation(.easeOut(duration: 0.5)) { moodText = nil }
            }
        }
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
            }
            .onAppear { controller.setDirector(surface == .director) }
            .onChange(of: surface) { _, newValue in
                controller.setDirector(newValue == .director)
            }
        } else {
            Color(red: 0.055, green: 0.075, blue: 0.10)
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
                guard controller.ready else { return }
                if controller.carrying {
                    let dx = (gesture.location.x - gesture.startLocation.x) / (size.width * 0.5)
                    let dy = (gesture.location.y - gesture.startLocation.y) / (size.height * 0.5)
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
                guard !controller.carrying else { return }
                controller.joystick = .zero
                switch stickMode {
                case .head:
                    controller.setHead(yaw: 0, pitch: 0)
                case .body:
                    controller.releaseBody()
                case .drive:
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
            .font(.system(size: 11, weight: .medium, design: .rounded))
            .foregroundStyle(.white.opacity(0.85))
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(.ultraThinMaterial, in: Capsule())
            .allowsHitTesting(false)
            .padding(.bottom, 6)
            .transition(.opacity)
            .accessibilityIdentifier("first-launch-hint")
    }

    private func presentHint() {
        guard !hintShown else { return }
        hintShown = true
        withAnimation(.easeIn(duration: 0.25)) { showHint = true }
        DispatchQueue.main.asyncAfter(deadline: .now() + 6) {
            withAnimation(.easeOut(duration: 0.6)) { showHint = false }
        }
    }

    // MARK: - Portrait shell (Stage Deck)

    /// The one-thumb composition: status on top, the duck's stage in the
    /// middle, and a 48 pt time rail plus a 76 pt deck under the thumb. The
    /// landscape chrome is untouched and still used whenever the window is
    /// wider than it is tall.
    private func portraitChrome(in size: CGSize) -> some View {
        VStack(spacing: 0) {
            topBar(showSurface: false)
            Spacer(minLength: 0)
                // The stage keeps the drive gesture; the chrome above it must
                // not swallow it.
                .allowsHitTesting(false)
            if showHint {
                hintCapsule
            } else if let mood = moodText {
                moodPill(mood)
            } else if let care = controller.careLine {
                moodPill(care)
            }
            timeRail
                .disabled(!controller.ready)
                .opacity(controller.ready ? 1 : 0.4)
            thumbDeck
                .disabled(!controller.ready)
                .opacity(controller.ready ? 1 : 0.4)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 8)
    }

    /// The whole story layer from the companion direction, reduced to one
    /// line: what happened while you were away, or what the duck feels now.
    private func moodPill(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 11, weight: .medium, design: .rounded))
            .foregroundStyle(.white.opacity(0.85))
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(.ultraThinMaterial, in: Capsule())
            .allowsHitTesting(false)
            .padding(.bottom, 6)
            .transition(.opacity)
            .accessibilityIdentifier("mood-pill")
    }

    private var timeRail: some View {
        HStack(spacing: 8) {
            Button {
                showInstrument = true
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "clock.arrow.circlepath")
                        .font(.system(size: 15))
                    Text("t \(controller.simTick)")
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundStyle(.secondary)
                    GeometryReader { proxy in
                        ZStack(alignment: .leading) {
                            Capsule()
                                .fill(Color.white.opacity(0.08))
                                .frame(height: 3)
                            Capsule()
                                .fill(Color.accentColor.opacity(0.6))
                                .frame(width: railProgress(width: proxy.size.width), height: 3)
                        }
                        .frame(maxHeight: .infinity, alignment: .center)
                    }
                    .frame(height: 18)
                    Image(systemName: "chevron.up")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.tertiary)
                }
                .padding(.horizontal, 12)
                .frame(maxWidth: .infinity, minHeight: 48)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.timeMachine)
            .accessibilityIdentifier("duck-time")

            Button {
                controller.setPaused(!controller.paused)
            } label: {
                Image(systemName: controller.paused ? "play.fill" : "pause.fill")
                    .font(.system(size: 16))
                    .frame(width: 48, height: 48)
                    .background(
                        controller.paused
                            ? AnyShapeStyle(Color.orange.opacity(0.28))
                            : AnyShapeStyle(.ultraThinMaterial),
                        in: Circle()
                    )
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(controller.paused ? Copy.resume : Copy.freeze)
            .accessibilityIdentifier("rail-pause")

            // Other ducks on this Wi-Fi: discovery is the core's, the list is
            // one tap from the stage.
            Button {
                showPeers = true
            } label: {
                Image(systemName: "dot.radiowaves.left.and.right")
                    .font(.system(size: 16))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.peers)
            .accessibilityIdentifier("duck-peers")

            Button {
                toggleSurface()
            } label: {
                Image(systemName: surface == .live ? "viewfinder" : "circle.dashed")
                    .font(.system(size: 16, weight: .semibold))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(surface == .live ? Copy.director : Copy.live)
            .accessibilityIdentifier("surface-toggle")
        }
        .frame(height: 48)
        .padding(.top, 6)
        .padding(.bottom, 8)
    }

    /// The share of retained history, not of the whole run: the bar fills as
    /// checkpoints age out of the ring buffer.
    private func railProgress(width: CGFloat) -> CGFloat {
        let newest = Double(controller.simTick)
        guard newest > 0 else { return 0 }
        let oldest = Double(min(controller.oldestTick, controller.simTick))
        let fraction = max(0, min(1, (newest - oldest) / newest))
        return width * CGFloat(fraction)
    }

    private var thumbDeck: some View {
        HStack(spacing: 6) {
            Spacer(minLength: 0)
            deckButton("duck-feed", Copy.feed, "fork.knife") { controller.feed() }
            deckButton("duck-skills", Copy.skills, "sparkles") { showRing = true }
            Button {
                quackTapped()
            } label: {
                Image(systemName: "waveform")
                    .font(.system(size: 19))
                    .frame(width: 50, height: 50)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .simultaneousGesture(wheeeGesture)
            .accessibilityLabel(Copy.quack)
            .accessibilityIdentifier("duck-quack")

            Button {
                if controller.carrying {
                    controller.release()
                } else {
                    controller.grab()
                }
            } label: {
                Image(systemName: controller.carrying ? "hand.raised.fill" : "hand.point.up.left.fill")
                    .font(.system(size: 20))
                    .frame(width: 56, height: 56)
                    .background(
                        controller.carrying
                            ? AnyShapeStyle(Color.cyan.opacity(0.35))
                            : AnyShapeStyle(.ultraThinMaterial),
                        in: Circle()
                    )
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(controller.carrying ? Copy.putDown : Copy.grab)
            .accessibilityIdentifier("duck-grab")

            Button {
                showRules = true
            } label: {
                Image(systemName: "square.grid.2x2.fill")
                    .font(.system(size: 16))
                    .frame(width: 44, height: 44)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.blocks)
            .accessibilityIdentifier("duck-rules")
        }
        .frame(height: 76)
        .padding(.horizontal, 10)
        .background(
            Color(red: 0.078, green: 0.102, blue: 0.133).opacity(0.94),
            in: RoundedRectangle(cornerRadius: 28)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 28)
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
    }

    private func deckButton(
        _ id: String,
        _ label: String,
        _ icon: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 18))
                .frame(width: 46, height: 46)
                .background(.ultraThinMaterial, in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
    }

    private func toggleSurface() {
        withAnimation(.snappy(duration: 0.2)) {
            surface = surface == .live ? .director : .live
        }
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
        default: break
        }
    }

    /// Portrait steering is translation-based from wherever the thumb lands:
    /// a hidden centre-origin joystick made a resting thumb command reverse,
    /// and a quick tap only stepped for as long as the finger was down.
    private func portraitDriveGesture(in size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                guard controller.ready, !showRing else { return }
                if controller.carrying {
                    let dx = gesture.translation.width / (size.width * 0.5)
                    let dy = gesture.translation.height / (size.height * 0.5)
                    controller.carry(dx: Double(dx) * 0.35, dy: 0, dz: Double(-dy) * 0.30)
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
                    controller.setHead(yaw: Double(nx) * 1.2, pitch: Double(-ny) * 0.6)
                case .body:
                    controller.setBody(z: Double(-ny) * 0.05, roll: Double(nx) * 0.12)
                }
            }
            .onEnded { gesture in
                guard controller.ready else { return }
                if controller.carrying { return }
                let distance = hypot(gesture.translation.width, gesture.translation.height)
                let wasTap = distance < 8
                if wasTap && !showRing {
                    // A touch on the duck strokes it; a touch on the floor
                    // ahead sends a short step or turn, not a dead no-op.
                    let insideDuck = gesture.location.x > size.width * 0.25
                        && gesture.location.x < size.width * 0.75
                        && gesture.location.y > size.height * 0.33
                        && gesture.location.y < size.height * 0.68
                    if insideDuck {
                        controller.pet()
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                    } else if stickMode == .drive {
                        let dx = gesture.location.x - size.width * 0.5
                        let dy = gesture.location.y - size.height * 0.5
                        let norm = max(hypot(dx, dy), 1)
                        controller.joystick = CGPoint(
                            x: dx / norm * 0.6,
                            y: -dy / norm * 0.6
                        )
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
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

    @ViewBuilder
    private func chrome(in size: CGSize) -> some View {
        VStack(spacing: 0) {
            topBar()
            Spacer()
            if let failure = failureMessage {
                failureCard(failure)
            } else {
                // No card: the space between the bars stays transparent to taps.
                Color.clear.frame(height: 0)
            }
            if showHint { hintCapsule }
            bottomBar
                // Until the core is ready every control is a no-op; dim and
                // disable them rather than letting them look alive.
                .disabled(!controller.ready)
                .opacity(controller.ready ? 1 : 0.4)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .allowsHitTesting(true)
    }

    private var failureMessage: String? {
        if let error = controller.errorMessage { return error }
        if let error = controller.renderError { return error }
        return nil
    }

    private func topBar(showSurface: Bool = true) -> some View {
        HStack(alignment: .top) {
            // The duck's name and what it is doing, in words rather than numbers.
            VStack(alignment: .leading, spacing: 2) {
                Text(Copy.name)
                    .font(.system(size: 17, weight: .semibold, design: .rounded))
                Text(controller.ready ? controller.label : Copy.waking)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("duck-label")
                // The ring's verdict outlives the skill's brief label. A slow
                // CI runner must be able to read it after the move finishes.
                if !controller.lastSkill.isEmpty {
                    Text(controller.lastSkill)
                        .font(.system(size: 9, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("duck-last-skill")
                }
                if controller.fallen {
                    Text(Copy.fell)
                        .font(.system(size: 11, weight: .semibold, design: .rounded))
                        .foregroundStyle(.orange)
                }
                if controller.pickedUp && !controller.carrying {
                    Text(Copy.held)
                        .font(.system(size: 11, weight: .semibold, design: .rounded))
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
                    withAnimation(.snappy(duration: 0.2)) {
                        surface = surface == .live ? .director : .live
                    }
                } label: {
                    Image(systemName: surface == .live ? "viewfinder" : "circle.dashed")
                        .font(.system(size: 17, weight: .semibold))
                        .frame(width: 48, height: 48)
                        .background(.ultraThinMaterial, in: Circle())
                        .contentShape(Circle())
                }
                .accessibilityLabel(surface == .live ? Copy.director : Copy.live)
                .accessibilityIdentifier("surface-toggle")
            }
            // One menu, not six chips. On a landscape phone the chips ran off
            // the edge and nothing was legible; the stick modes, the surface,
            // the language and the flashlight now live behind one button a
            // thumb can open.
            Menu {
                ForEach(StickMode.allCases, id: \.self) { option in
                    Button {
                        stickMode = option
                        controller.setHead(yaw: 0, pitch: 0)
                        controller.releaseBody()
                    } label: {
                        Label(Copy.stick(option.rawValue), systemImage: stickIcon(option))
                    }
                }
                Divider()
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = .live }
                } label: {
                    Label(Copy.live, systemImage: "circle.dashed")
                }
                .accessibilityIdentifier("surface-live")
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = .director }
                } label: {
                    Label(Copy.director, systemImage: "viewfinder")
                }
                .accessibilityIdentifier("surface-director")
                Divider()
                Button {
                    controller.toggleFlashlight()
                } label: {
                    Label(
                        controller.flashlightColor == nil ? Copy.flashlight : Copy.flashlightOn,
                        systemImage: "flashlight.on.fill"
                    )
                }
                // Tilt-to-look: the phone is the head, as the pad's IMU is.
                Button {
                    controller.tiltLook.toggle()
                } label: {
                    Label(
                        controller.tiltLook ? Copy.tiltLookOn : Copy.tiltLook,
                        systemImage: controller.tiltLook ? "iphone.and.arrow.left.and.right" : "iphone.gen3"
                    )
                }
                Button {
                    Copy.language = Copy.language == .english ? .chinese : .english
                    languageTick &+= 1
                } label: {
                    Label(Copy.language.rawValue, systemImage: "globe")
                }
                Button {
                    showRules = true
                } label: {
                    Label(Copy.blocks, systemImage: "square.grid.2x2.fill")
                }
                .accessibilityIdentifier("open-rules")
                Button {
                    showPeers = true
                } label: {
                    Label(Copy.peers, systemImage: "dot.radiowaves.left.and.right")
                }
                .accessibilityIdentifier("open-peers")
                Button {
                    showAI = true
                } label: {
                    Label(Copy.ai, systemImage: "brain.head.profile")
                }
                .accessibilityIdentifier("open-ai")
                Button {
                    showDance = true
                } label: {
                    Label(Copy.dance, systemImage: "music.note.list")
                }
                .accessibilityIdentifier("open-dance")
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 17, weight: .semibold))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .accessibilityLabel(Copy.menu)
            .accessibilityIdentifier("duck-menu")
        }
        .padding(10)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
    }

    private var bottomBar: some View {
        HStack(spacing: 10) {
            // The time machine lives one pull away, not on the screen.
            Button {
                showInstrument = true
            } label: {
                Label(Copy.time, systemImage: "clock.arrow.circlepath")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 18))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.time)
            .accessibilityIdentifier("duck-time")

            Button {
                showRing = true
            } label: {
                Label(Copy.skills, systemImage: "sparkles")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 18))
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
                    .font(.system(size: 18))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .simultaneousGesture(wheeeGesture)
            .accessibilityLabel(Copy.quack)
            .accessibilityIdentifier("duck-quack")

            // A treat on the floor: the official ground-pick peck.
            Button {
                controller.feed()
            } label: {
                Label(Copy.feed, systemImage: "fork.knife")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 18))
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
                    .font(.system(size: 18))
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
                .font(.system(size: 18))
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
                showRules = true
            } label: {
                Label(Copy.blocks, systemImage: "square.grid.2x2.fill")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 17))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.blocks)
            .accessibilityIdentifier("duck-rules")

            Spacer()

            Text(controller.carrying ? Copy.held : Copy.tapHint)
                .font(.system(size: 10, design: .rounded))
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

    /// One icon per stick mode, so the menu reads at a glance instead of
    /// being three identical words.
    private func stickIcon(_ mode: StickMode) -> String {
        switch mode {
        case .drive: return "figure.walk"
        case .head: return "eye"
        case .body: return "arrow.up.and.down"
        }
    }

    private func failureCard(_ message: String) -> some View {
        VStack(spacing: 10) {
            Text(Copy.couldNotStart)
                .font(.system(size: 15, weight: .semibold, design: .rounded))
            Text(message)
                .font(.system(size: 12, design: .rounded))
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
        }
        .padding(20)
        .frame(maxWidth: 360)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
        .padding(.vertical, 24)
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
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: "sparkles")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.secondary)
                Text(Copy.skills)
                    .font(.system(size: 15, weight: .semibold, design: .rounded))
                Spacer()
                Button(action: onClose) {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.secondary)
                        // 48 pt, the same floor as every other control; the
                        // 28 pt icon-only target was the smallest thing in
                        // the app.
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("ring-close")
            }
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 16) {
                    if !actions.isEmpty {
                        section(Copy.actions, names: actions)
                    }
                    if !emotions.isEmpty {
                        section(Copy.emotions, names: emotions)
                    }
                }
                .padding(.bottom, 4)
            }
            // Landscape's safe height is about 372 pt; the old fixed 360 pt
            // scroll plus header and padding clipped both ends. Portrait has
            // room for the whole palette.
            .frame(maxHeight: bottomAnchored ? 430 : 240)
        }
        .padding(16)
        .frame(maxWidth: 360)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 22))
        .padding(.horizontal, 24)
    }

    /// One group of capsules: a small heading, then an adaptive grid so the
    /// English wire names still fit three to a row.
    private func section(_ title: String, names: [String]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.system(size: 10, weight: .semibold, design: .rounded))
                .tracking(0.8)
                .foregroundStyle(.secondary)
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 88), spacing: 8)],
                alignment: .leading,
                spacing: 8
            ) {
                ForEach(names, id: \.self) { name in
                    Button {
                        onPick(name)
                    } label: {
                        Text(SkillLabel.of(name))
                            .font(.system(size: 12, weight: .medium, design: .rounded))
                            .lineLimit(1)
                            .minimumScaleFactor(0.8)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 9)
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
                HStack(spacing: 6) {
                    Circle()
                        .fill(controller.cameraActive ? Color.red : Color.orange)
                        .frame(width: 7, height: 7)
                    // With no camera (simulator, denied permission, a device
                    // without one) Director is a dark studio, not a room.
                    // Say that instead of drawing a viewfinder over it.
                    Text(controller.cameraActive ? Copy.viewfinder : Copy.cameraUnavailable)
                        .font(.system(size: 9, weight: .semibold, design: .monospaced))
                        .foregroundStyle(.white.opacity(0.85))
                }
                .padding(.top, bar / 2 - 5)
                .padding(.leading, 14)
            }
            .overlay(alignment: .bottomTrailing) {
                Text(String(format: "t %llu · %.2fx", controller.simTick, controller.speed))
                    .font(.system(size: 9, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.7))
                    .padding(.bottom, bar / 2 - 5)
                    .padding(.trailing, 14)
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
                            .font(.system(size: 20, weight: .semibold))
                            .foregroundStyle(.black)
                    }
                    .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.photoTake)
                .accessibilityIdentifier("duck-photo")
                .padding(.trailing, 16)
            }
        }
    }
}
