import SwiftUI
import UIKit

/// The chat input's pure state machine: `take()` hands the trimmed question
/// to the model and clears the field, so one sentence is not sent twice.
struct ChatComposer: Equatable {
    var draft = ""

    mutating func take() -> String? {
        let trimmed = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        draft = ""
        return trimmed
    }
}

/// One plain-text exchange with the user's own model. The store lives beside
/// the sheet instead of on DuckController: a conversation is page state, and
/// the controller's published surface stays what the stage observes.
final class ChatStore: ObservableObject {
    @Published private(set) var reply = ""
    @Published private(set) var error: String?
    @Published private(set) var busy = false
    private let brain: ChatBrain

    init(speak: @escaping (String) -> Void) {
        brain = ChatBrain(speak: speak)
    }

    /// Fetch the duck's real state, let the model answer in plain words,
    /// then let ChatBrain read the reply aloud. Nothing here reaches the
    /// core beyond the same /state numbers the companion already sends.
    func ask(config: AIConfig, question: String, language: Copy.Language) {
        guard !busy else { return }
        let trimmed = question.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        guard ChatRules.canAsk(enabled: config.enabled, endpoint: config.endpoint) else {
            error = Copy.chatNoModel
            reply = ""
            return
        }
        busy = true
        error = nil
        reply = ""
        guard let url = URL(string: "http://127.0.0.1:8765/state") else {
            busy = false
            return
        }
        var request = URLRequest(url: url)
        request.timeoutInterval = 2.0
        URLSession.shared.dataTask(with: request) { [weak self] data, _, _ in
            guard let self else { return }
            let state = data.flatMap {
                try? JSONSerialization.jsonObject(with: $0) as? [String: Any]
            } ?? [:]
            DispatchQueue.main.async {
                self.brain.ask(
                    config: config,
                    question: trimmed,
                    state: state,
                    language: language
                ) { [weak self] reply, error in
                    guard let self else { return }
                    self.busy = false
                    if let error {
                        self.error = error
                        return
                    }
                    if let reply { self.reply = reply }
                }
            }
        }.resume()
    }
}

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
    /// The question being typed. Local so a keystroke does not republish the
    /// whole controller (and the stage) 50 times while someone types.
    @State private var composer = ChatComposer()
    @StateObject private var chat: ChatStore

    init(controller: DuckController) {
        _controller = ObservedObject(wrappedValue: controller)
        _chat = StateObject(wrappedValue: ChatStore(speak: { [weak controller] text in
            guard let controller else { return }
            controller.speakChat(text)
        }))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack {
                Text(Copy.ai)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("ai-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(DuckIcon.xs)
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
                VStack(alignment: .leading, spacing: DuckSpace.md) {
                    Toggle(Copy.ai, isOn: $enabled)
                        .font(.duckLabel.weight(.semibold))
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
                    .font(.duckLabel)
                    .accessibilityIdentifier("ai-interval")

                    HStack(spacing: DuckSpace.sm) {
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
                            .font(.duckCaption)
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("ai-status")
                    }
                    if let error = controller.aiError {
                        Text(error)
                            .font(.duckCaption)
                            .foregroundStyle(.orange)
                    }
                    if !controller.aiDecision.isEmpty {
                        Text(controller.aiDecision)
                            .font(.duckCaption.weight(.medium))
                            .foregroundStyle(.cyan)
                            .accessibilityIdentifier("ai-decision")
                    }

                    Text(Copy.aiHint)
                        .font(.duckCaption)
                        .foregroundStyle(.secondary)
                    Text(Copy.aiNote)
                        .font(.duckCaption)
                        .foregroundStyle(.tertiary)
                    Divider().overlay(Color.white.opacity(0.12))
                    chatSection
                }
            }
        }
        .padding(DuckSpace.xl)
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

    /// The chat half of the sheet: one question, one plain-text answer, read
    /// aloud on this phone. Send stays disabled until an endpoint is entered,
    /// and the hint says why instead of failing silently.
    private var chatSection: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            Text(Copy.chatTitle)
                .font(.duckBody.weight(.semibold))
                .accessibilityIdentifier("chat-title")

            HStack(spacing: DuckSpace.sm) {
                TextField(Copy.chatHint, text: $composer.draft)
                    .textFieldStyle(.roundedBorder)
                    .autocorrectionDisabled()
                    .submitLabel(.send)
                    .onSubmit { sendChat() }
                    .accessibilityIdentifier("chat-input")
                Button(Copy.chatSend) {
                    sendChat()
                }
                .buttonStyle(.borderedProminent)
                .frame(minHeight: 44)
                .disabled(!canSend)
                .accessibilityIdentifier("chat-send")
            }

            if !controller.heardText.isEmpty {
                Button(controller.heardText) {
                    composer.draft = controller.heardText
                }
                .font(.duckCaption)
                .buttonStyle(.bordered)
                .lineLimit(1)
                .accessibilityIdentifier("chat-heard")
            }

            if !configured {
                Text(Copy.chatNoModel)
                    .font(.duckCaption)
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("chat-no-model")
            }
            if chat.busy {
                Text(Copy.chatThinking)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("chat-thinking")
            }
            if !chat.reply.isEmpty {
                Text(chat.reply)
                    .font(.duckLabel)
                    .foregroundStyle(.white)
                    .padding(DuckSpace.sm)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(
                        Color.white.opacity(0.10),
                        in: RoundedRectangle(cornerRadius: DuckRadius.thumb)
                    )
                    .accessibilityIdentifier("chat-reply")
            }
            if let error = chat.error, !error.isEmpty {
                Text(error)
                    .font(.duckCaption)
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("chat-error")
            }
            Text(Copy.chatLocalVoice)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
        }
    }

    private var configured: Bool {
        ChatRules.canAsk(enabled: enabled, endpoint: endpoint)
    }

    private var canSend: Bool {
        configured
            && !chat.busy
            && !composer.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Save what is on screen, take the draft (which clears the field), then
    /// ask. Saving first means a typed endpoint works without a separate tap
    /// on Save, exactly like "Think now".
    private func sendChat() {
        guard canSend, let question = composer.take() else { return }
        controller.saveAIConfig(config())
        chat.ask(
            config: controller.aiConfig,
            question: question,
            language: Copy.language
        )
    }

    private func field(
        _ label: String,
        text: Binding<String>,
        id: String,
        url: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.xs) {
            Text(label)
                .font(.duckCaption.weight(.medium))
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
