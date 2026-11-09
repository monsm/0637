import Foundation
import SwiftUI

struct ContentView: View {
    @StateObject private var controller = DuckController()
    @State private var joystick = CGPoint.zero

    var body: some View {
        ZStack {
            if controller.ready && controller.renderError == nil {
                MetalView(controller: controller)
                    .ignoresSafeArea()
            } else {
                // The renderer reads the core's meshes when it is created,
                // so it only exists once the core is ready.
                Color(red: 0.055, green: 0.075, blue: 0.10)
                    .ignoresSafeArea()
            }

            if let failure = failureMessage {
                // Never leave the user looking at a black screen: say what
                // went wrong.
                failureCard(failure)
            }

            VStack {
                HStack(alignment: .top) {
                    statusPanel
                    Spacer()
                }
                Spacer()
                HStack(alignment: .bottom) {
                    JoystickView(value: $joystick)
                        .frame(width: 150, height: 150)
                    Spacer()
                    controls
                }
                .padding(20)
            }
            .padding(16)
        }
        .onChange(of: joystick) { _, value in
            controller.joystick = value
        }
        .preferredColorScheme(.dark)
    }

    /// The first thing that went wrong: the core, then the renderer.
    private var failureMessage: String? {
        if let error = controller.errorMessage { return error }
        if let error = controller.renderError { return error }
        return nil
    }

    private func failureCard(_ message: String) -> some View {
        VStack(spacing: 12) {
            Text("MicroDuck could not start")
                .font(.headline)
            Text(message)
                .font(.caption)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
        }
        .padding(24)
        .frame(maxWidth: 420)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
        .padding(32)
    }

    private var statusPanel: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("MicroDuck")
                .font(.headline)
            if let error = controller.errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            } else if !controller.ready {
                Text("Loading MuJoCo + policies…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Text(controller.label)
                    .font(.caption)
                    .accessibilityIdentifier("duck-label")
                Text("\(controller.bringupText) · gain \(controller.gain)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("AI API :8765 · shake to push")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            if controller.fallen {
                Text("FALLEN")
                    .font(.caption.bold())
                    .foregroundStyle(.red)
            }
        }
        .padding(12)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12))
    }

    private var controls: some View {
        VStack(alignment: .trailing, spacing: 8) {
            ForEach(controller.skillNames(), id: \.self) { name in
                Button(name) {
                    controller.trigger(name)
                }
            }
            Button("Sit / Stand") {
                controller.sit()
            }
            Button("Ground pick") {
                controller.groundPick()
            }
            Button("Reload AI") {
                controller.reloadPolicies()
            }
        }
        .buttonStyle(.borderedProminent)
        .font(.caption)
    }
}

struct JoystickView: View {
    @Binding var value: CGPoint
    @State private var offset: CGSize = .zero

    var body: some View {
        let radius: CGFloat = 58
        ZStack {
            Circle()
                .fill(.white.opacity(0.06))
                .overlay(Circle().stroke(.white.opacity(0.15), lineWidth: 1))
            Circle()
                .fill(.white.opacity(0.22))
                .frame(width: 54, height: 54)
                .offset(offset)
        }
        .contentShape(Circle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { gesture in
                    var translation = gesture.translation
                    let length = hypot(translation.width, translation.height)
                    if length > radius {
                        translation = CGSize(
                            width: translation.width / length * radius,
                            height: translation.height / length * radius
                        )
                    }
                    offset = translation
                    value = CGPoint(
                        x: translation.width / radius,
                        y: translation.height / radius
                    )
                }
                .onEnded { _ in
                    offset = .zero
                    value = .zero
                }
        )
    }
}
