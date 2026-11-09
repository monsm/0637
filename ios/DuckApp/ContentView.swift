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

    enum Surface: String, CaseIterable {
        case live = "Live"
        case director = "Director"
    }

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                scene
                if controller.ready && controller.renderError == nil {
                    // The scene is the interface: taps on it drive the duck.
                    Color.clear
                        .contentShape(Rectangle())
                        .gesture(driveGesture(in: geometry.size))
                        .onLongPressGesture(minimumDuration: 0.35) {
                            showRing = true
                        }
                }
                chrome(in: geometry.size)
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
    private func driveGesture(in size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                guard controller.ready else { return }
                let center = CGPoint(x: size.width * 0.5, y: size.height * 0.52)
                let dx = (gesture.location.x - center.x) / (size.width * 0.5)
                let dy = (gesture.location.y - center.y) / (size.height * 0.5)
                // Forward is up the screen: the duck walks away from you.
                let forward = max(0, -dy)
                let turn = dx
                controller.joystick = CGPoint(
                    x: abs(turn) > 0.08 ? turn : 0,
                    y: forward > 0.08 ? -forward : 0
                )
            }
            .onEnded { _ in
                controller.joystick = .zero
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
            }
            bottomBar
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
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
                Text("MicroDuck")
                    .font(.system(size: 17, weight: .semibold, design: .rounded))
                Text(controller.ready ? controller.label : "waking up…")
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("duck-label")
                if controller.fallen {
                    Text("it fell")
                        .font(.system(size: 11, weight: .semibold, design: .rounded))
                        .foregroundStyle(.orange)
                }
            }
            Spacer()
            surfaceSwitch
        }
        .padding(12)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
    }

    private var surfaceSwitch: some View {
        HStack(spacing: 4) {
            ForEach(Surface.allCases, id: \.self) { option in
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = option }
                } label: {
                    Text(option.rawValue)
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
                Label("Time", systemImage: "clock.arrow.circlepath")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 18))
                    .frame(width: 46, height: 46)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Time")
            .accessibilityIdentifier("duck-time")

            Button {
                showRing = true
            } label: {
                Label("Skills", systemImage: "sparkles")
                    .labelStyle(.iconOnly)
                    .font(.system(size: 18))
                    .frame(width: 46, height: 46)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Skills")

            Spacer()

            Text("tap ahead to walk · long-press the duck")
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.secondary)
                .opacity(controller.ready ? 1 : 0)
        }
    }

    private func failureCard(_ message: String) -> some View {
        VStack(spacing: 10) {
            Text("MicroDuck could not start")
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

/// The ring of skills: the duck's own one-shots, arranged around a hold.
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
                        Text(name)
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
                VStack(spacing: 6) {
                    Text("sit")
                        .accessibilityIdentifier("ring-sit")
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(.ultraThinMaterial, in: Capsule())
                        .onTapGesture { onPick("sit") }
                    Text("pick")
                        .accessibilityIdentifier("ring-ground_pick")
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(.ultraThinMaterial, in: Capsule())
                        .onTapGesture { onPick("ground_pick") }
                }
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
                    Text("VIEWFINDER")
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
