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
    @State private var showInstrument = false
    @State private var tapTarget: CGPoint?
    @State private var lastTap = Date.distantPast
    /// Flipping this re-renders every `Copy` string.
    @State private var languageTick = 0

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
            }
            .ignoresSafeArea()
            .sheet(isPresented: $showInstrument) {
                InstrumentSheet(controller: controller)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - Scene

    @ViewBuilder
    private var scene: some View {
        if controller.ready && controller.renderError == nil {
            MetalView(controller: controller)
                .ignoresSafeArea()
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
                // Forward is up the screen: the duck walks away from you. Drag
                // below the centre to walk backwards, the pad's stick does.
                let forward = -dy
                controller.joystick = CGPoint(
                    x: abs(dx) > 0.08 ? dx : 0,
                    y: abs(forward) > 0.08 ? -forward : 0
                )
            }
            .onEnded { _ in
                // A held duck stays in the hand where the drag left it; the
                // walk command is what must stop with the finger.
                if !controller.carrying {
                    controller.joystick = .zero
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
            HStack(spacing: 6) {
                languageSwitch
                surfaceSwitch
            }
        }
        .padding(12)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
    }

    private var languageSwitch: some View {
        Button {
            Copy.language = Copy.language == .english ? .chinese : .english
            languageTick &+= 1
        } label: {
            Text(Copy.language.rawValue)
                .font(.system(size: 11, weight: .medium, design: .rounded))
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(Color.white.opacity(0.10), in: Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Copy.languageLabel)
        .accessibilityIdentifier("duck-language")
    }

    private var surfaceSwitch: some View {
        HStack(spacing: 4) {
            ForEach(Surface.allCases, id: \.self) { option in
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = option }
                } label: {
                    Text(option == .live ? Copy.live : Copy.director)
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(
                            surface == option ? Color.white.opacity(0.18) : Color.clear,
                            in: Capsule()
                        )
                }
                .buttonStyle(.plain)
            }
        }
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
                    .frame(width: 46, height: 46)
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
                    .frame(width: 46, height: 46)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.skills)
            .accessibilityIdentifier("duck-skills")

            // The duck's own voice: `robotctl quack` is a chirp.
            Button {
                controller.voice.play("chirp")
            } label: {
                Label(Copy.quack, systemImage: "waveform")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 18))
                    .frame(width: 46, height: 46)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.quack)
            .accessibilityIdentifier("duck-quack")

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
                .frame(width: 46, height: 46)
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

/// The ring of skills: every verb `/skills` publishes — the manifest's
/// one-shots and the loop's own `sit` / `stand` / `ground_pick` — arranged
/// around a hold.
struct SkillRing: View {
    let names: [String]
    let onPick: (String) -> Void
    let onClose: () -> Void

    var body: some View {
        ZStack {
            Color.black.opacity(0.45)
                .ignoresSafeArea()
                .onTapGesture(perform: onClose)
            ZStack {
                ForEach(Array(names.enumerated()), id: \.offset) { index, name in
                    let angle = Double(index) / Double(max(names.count, 1)) * 2 * .pi
                    let radius: CGFloat = 120
                    Button {
                        onPick(name)
                    } label: {
                        Text(SkillLabel.of(name))
                            .font(.system(size: 12, weight: .medium, design: .rounded))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 9)
                            .background(.ultraThinMaterial, in: Capsule())
                    }
                    .buttonStyle(.plain)
                    // The ring's entries share their names with nothing else,
                    // but an accessibility id keeps the query unambiguous.
                    .accessibilityIdentifier("ring-\(name)")
                    .offset(
                        x: radius * cos(angle - .pi / 2),
                        y: radius * sin(angle - .pi / 2)
                    )
                }
                Circle()
                    .fill(.ultraThinMaterial)
                    .frame(width: 54, height: 54)
                    .overlay(
                        Image(systemName: "sparkles")
                            .font(.system(size: 18))
                            .foregroundStyle(.secondary)
                    )
                    .onTapGesture(perform: onClose)
            }
            .transition(.scale.combined(with: .opacity))
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
