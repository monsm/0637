import Foundation
import SwiftUI

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

    enum Surface: String, CaseIterable {
        case live = "Live"
        case director = "Director"
    }

    var body: some View {
        let _ = languageTick
        GeometryReader { geometry in
            ZStack {
                // The drive gesture lives ON the scene rather than on a
                // full-screen clear layer: a separate hit-testing layer above
                // a UIView-backed Metal view is what swallowed the bars'
                // presses on a device.
                scene
                    .contentShape(Rectangle())
                    .gesture(driveGesture(in: geometry.size))
                    .onLongPressGesture(minimumDuration: 0.35) {
                        if controller.ready && controller.renderError == nil {
                            showRing = true
                        }
                    }
                chrome(in: geometry.size)
                    .zIndex(2)
                if showRing {
                    SkillRing(names: controller.skillNames()) { name in
                        controller.trigger(name)
                        showRing = false
                    } onClose: {
                        showRing = false
                    }
                }
                // One small caption, above the bottom bar, in the same capsule
                // language as the rest of the chrome. It takes no touches so
                // it cannot swallow the first walk tap.
                if showHint {
                    Text(Copy.firstLaunchHint)
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .foregroundStyle(.white.opacity(0.85))
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                        .background(.ultraThinMaterial, in: Capsule())
                        .allowsHitTesting(false)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                        .padding(.bottom, 74)
                        .transition(.opacity)
                        .zIndex(1)
                        .accessibilityIdentifier("first-launch-hint")
                }
            }
            // The 3D scene fills the screen (it owns its own ignoresSafeArea),
            // but the chrome stays inside the safe area: on a notched phone
            // the old full-screen ignore put the buttons under the home bar.
            .sheet(isPresented: $showInstrument) {
                InstrumentSheet(controller: controller)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
        }
        .preferredColorScheme(.dark)
        // A Task sleeps without blocking the 50 Hz main run loop. Cancellation
        // keeps the persisted flag set, which is the one-time promise.
        .task {
            guard !hintShown else { return }
            hintShown = true
            withAnimation(.easeIn(duration: 0.25)) { showHint = true }
            // `try?` so a cancelled sleep (view torn down, app backgrounded)
            // cannot throw out of a SwiftUI modifier.
            try? await Task.sleep(nanoseconds: 4_000_000_000)
            withAnimation(.easeOut(duration: 0.6)) { showHint = false }
        }
        // SwiftUI does not report a cancelled drag. A phone call, a
        // notification pull or leaving the app would otherwise leave the
        // joystick where the finger was and the duck walking by itself.
        .onChange(of: scenePhase) { _, phase in
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
            DirectorOverlay(controller: controller)
                .allowsHitTesting(false)
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

    @ViewBuilder
    private func chrome(in size: CGSize) -> some View {
        VStack(spacing: 0) {
            topBar
            Spacer()
            if let failure = failureMessage {
                failureCard(failure)
            } else {
                // No card: the space between the bars stays transparent to taps.
                Color.clear.frame(height: 0)
            }
            bottomBar
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

    private var topBar: some View {
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
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = .director }
                } label: {
                    Label(Copy.director, systemImage: "viewfinder")
                }
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
                // Held for 0.3 s it is the joy-ride instead: `wheeeGesture`
                // starts it and ends it on the lift, and the gesture's end
                // runs before this action, so this sees the marker it left.
                if wheeeHolding || Date().timeIntervalSince(wheeeEndedAt) < 0.5 {
                    wheeeHolding = false
                    wheeeEndedAt = .distantPast
                } else if let core = controller.core {
                    controller.voice.play(core: core, tag: "chirp")
                }
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
        ZStack {
            Color.black.opacity(0.45)
                .ignoresSafeArea()
                .onTapGesture(perform: onClose)
            picker
                .transition(.scale.combined(with: .opacity))
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
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 28, height: 28)
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
            .frame(maxHeight: 360)
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
                            .frame(maxWidth: .infinity)
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
                        .fill(Color.red)
                        .frame(width: 7, height: 7)
                    Text(Copy.viewfinder)
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
        }
    }
}
