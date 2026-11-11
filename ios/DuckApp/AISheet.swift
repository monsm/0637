import SwiftUI
import UIKit

/// The user's own model. The sheet edits a config the shell executes; the
/// endpoint is only contacted while the toggle is on, and only the duck's
/// state and last heard words are sent.
struct AISheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss
    @State private var enabled = false
    @State private var endpoint = ""
    @State private var model = ""
    @State private var key = ""
    @State private var interval = 30.0
    @State private var persona = ""
    @State private var loaded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(Copy.ai)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("ai-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("ai-close")
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Toggle(Copy.ai, isOn: $enabled)
                        .font(.system(size: 14, weight: .semibold, design: .rounded))
                        .accessibilityIdentifier("ai-enable")

                    field(Copy.aiEndpoint, text: $endpoint, id: "ai-endpoint", url: true)
                    field(Copy.aiModel, text: $model, id: "ai-model", url: false)
                    SecureField(Copy.aiKey, text: $key)
                        .textFieldStyle(.roundedBorder)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .accessibilityIdentifier("ai-key")
                    field(Copy.aiPersona, text: $persona, id: "ai-persona", url: false)

                    Stepper(
                        "\(Copy.aiInterval): \(Int(interval))",
                        value: $interval,
                        in: 10...300,
                        step: 5
                    )
                    .font(.system(size: 13, design: .rounded))
                    .accessibilityIdentifier("ai-interval")

                    HStack(spacing: 10) {
                        Button(Copy.aiSave) {
                            controller.saveAIConfig(config())
                        }
                        .buttonStyle(.borderedProminent)
                        .frame(minHeight: 48)
                        .accessibilityIdentifier("ai-save")

                        Button(Copy.aiThinkNow) {
                            controller.saveAIConfig(config())
                            controller.aiThinkNow()
                        }
                        .buttonStyle(.bordered)
                        .frame(minHeight: 48)
                        .accessibilityIdentifier("ai-think")
                    }

                    if !controller.aiStatus.isEmpty {
                        Text(controller.aiStatus)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("ai-status")
                    }
                    if let error = controller.aiError {
                        Text(error)
                            .font(.system(size: 11, design: .rounded))
                            .foregroundStyle(.orange)
                    }
                    if !controller.aiDecision.isEmpty {
                        Text(controller.aiDecision)
                            .font(.system(size: 12, weight: .medium, design: .rounded))
                            .foregroundStyle(.cyan)
                            .accessibilityIdentifier("ai-decision")
                    }

                    Text(Copy.aiHint)
                        .font(.system(size: 10, design: .rounded))
                        .foregroundStyle(.secondary)
                    Text(Copy.aiNote)
                        .font(.system(size: 10, design: .rounded))
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .padding(20)
        .preferredColorScheme(.dark)
        .onAppear {
            guard !loaded else { return }
            loaded = true
            let config = controller.aiConfig
            enabled = config.enabled
            endpoint = config.endpoint
            model = config.model
            key = config.key
            interval = max(10, config.interval)
            persona = config.persona
        }
    }

    private func field(
        _ label: String,
        text: Binding<String>,
        id: String,
        url: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.system(size: 11, weight: .medium, design: .rounded))
                .foregroundStyle(.secondary)
            TextField(label, text: text)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .keyboardType(url ? .URL : .default)
                .accessibilityIdentifier(id)
        }
    }

    private func config() -> AIConfig {
        var config = AIConfig()
        config.enabled = enabled
        config.endpoint = endpoint
        config.model = model
        config.key = key
        config.interval = interval
        config.persona = persona
        return config
    }
}
