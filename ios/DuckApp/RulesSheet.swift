import SwiftUI

/// The block program: "when I clap, roll over". Two rows of big chips, one
/// list of what is already running. Every change goes straight to the core,
/// which executes it; this screen is an editor, not a second engine.
struct RulesSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss
    @State private var trigger = "clap"
    @State private var action = "skill"
    @State private var value = "roulade"
    /// Ordered follow-ups staged before Add. Empty means a classic one-action
    /// rule; up to four keep the program comprehensible.
    @State private var chainDraft: [(String, String)] = []
    /// The last import's verdict, shown under the share row.
    @State private var importNote = ""

    private let triggers = ["clap", "whistle", "pet"]
    private let kinds = ["skill", "emotion", "sound"]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(Copy.blocks)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("rules-title")
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
                .accessibilityIdentifier("rules-close")
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if controller.rules.isEmpty {
                        Text(Copy.rulesEmpty)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(Array(controller.rules.enumerated()), id: \.offset) { index, rule in
                            HStack(spacing: 8) {
                                Text("\(Copy.ruleWhen) \(Copy.ruleTrigger(rule.trigger))")
                                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                                Image(systemName: "arrow.right")
                                    .font(.system(size: 11, weight: .bold))
                                    .foregroundStyle(.secondary)
                                Text(label(for: rule.value, kind: rule.action))
                                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                                Spacer()
                                Button {
                                    var next = controller.rules
                                    next.remove(at: index)
                                    controller.saveRules(next)
                                } label: {
                                    Image(systemName: "trash")
                                        .font(.system(size: 13))
                                        .frame(width: 44, height: 44)
                                        .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .foregroundStyle(.red.opacity(0.85))
                                .accessibilityLabel(Copy.deleteRule)
                            }
                            .padding(.horizontal, 12)
                            .frame(minHeight: 48)
                            .background(
                                LinearGradient(
                                    colors: [
                                        Color(red: 0.20, green: 0.32, blue: 0.45),
                                        Color(red: 0.16, green: 0.42, blue: 0.44),
                                    ],
                                    startPoint: .leading,
                                    endPoint: .trailing
                                ),
                                in: RoundedRectangle(cornerRadius: 12)
                            )
                            .accessibilityIdentifier("rule-\(index)")
                        }
                    }

                    Divider()

                    // A program travels between phones as text: the share
                    // sheet carries the versioned document, paste reads one
                    // back and merges it into what the child already built.
                    HStack(spacing: 10) {
                        if !controller.rules.isEmpty {
                            ShareLink(item: controller.rulesShareText) {
                                Label(Copy.rulesShare, systemImage: "square.and.arrow.up")
                                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                            }
                            .buttonStyle(.bordered)
                            .accessibilityIdentifier("rules-share")
                        }
                        Button {
                            if let text = UIPasteboard.general.string,
                               let added = controller.importRules(from: text) {
                                importNote = Copy.rulesPasted(added)
                            } else {
                                importNote = Copy.rulesPasteFailed
                            }
                        } label: {
                            Label(Copy.rulesPaste, systemImage: "doc.on.clipboard")
                                .font(.system(size: 12, weight: .semibold, design: .rounded))
                        }
                        .buttonStyle(.bordered)
                        .accessibilityIdentifier("rules-paste")
                        Spacer()
                    }
                    if !importNote.isEmpty {
                        Text(importNote)
                            .font(.system(size: 11, design: .rounded))
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("rules-import-note")
                    }
                    if !controller.rulesError.isEmpty {
                        Text(controller.rulesError)
                            .font(.system(size: 11, design: .rounded))
                            .foregroundStyle(.orange)
                            .accessibilityIdentifier("rules-error")
                    }

                    Divider()

                    Text(Copy.ruleWhen)
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                    chipRow(triggers, selected: trigger, kind: "trigger") { trigger = $0 }

                    Text(Copy.ruleThen)
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                    chipRow(kinds, selected: action, kind: "action") { action = $0 }
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(valueOptions, id: \.self) { option in
                                chip(
                                    label(for: option),
                                    selected: value == option,
                                    id: "rule-value-\(option)"
                                ) { value = option }
                            }
                        }
                        .padding(.vertical, 2)
                    }

                    // Chain: stage follow-up blocks in order. A child can
                    // build "clap -> roll -> laugh -> chirp" without typing.
                    HStack(spacing: 8) {
                        Text(Copy.ruleChain)
                            .font(.system(size: 12, weight: .medium, design: .rounded))
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button {
                            guard chainDraft.count < 4 else { return }
                            chainDraft.append((action, value))
                        } label: {
                            Label(Copy.ruleChainAdd, systemImage: "plus")
                                .font(.system(size: 11, weight: .semibold, design: .rounded))
                        }
                        .buttonStyle(.bordered)
                        .disabled(chainDraft.count >= 4)
                        .accessibilityIdentifier("rule-chain-add")
                    }
                    if !chainDraft.isEmpty {
                        HStack(spacing: 6) {
                            ForEach(Array(chainDraft.enumerated()), id: \.offset) { index, step in
                                Button {
                                    chainDraft.remove(at: index)
                                } label: {
                                    Text(label(for: step.1, kind: step.0))
                                        .font(.system(size: 11, weight: .semibold, design: .rounded))
                                        .padding(.horizontal, 10)
                                        .frame(minHeight: 48)
                                        .background(Color.white.opacity(0.10), in: Capsule())
                                        .contentShape(Capsule())
                                }
                                .buttonStyle(.plain)
                                .accessibilityHint(Copy.deleteRule)
                                .accessibilityIdentifier("rule-chain-\(index)")
                            }
                            Spacer()
                        }
                    }

                    Button {
                        var next = controller.rules
                        next.append(.init(
                            trigger: trigger,
                            action: action,
                            value: value,
                            then: chainDraft.map { DuckController.DuckRuleStep(action: $0.0, value: $0.1) }
                        ))
                        controller.saveRules(next)
                        chainDraft = []
                    } label: {
                        Label(
                            controller.rules.count >= 8 ? Copy.ruleLimit : Copy.addRule,
                            systemImage: controller.rules.count >= 8
                                ? "checkmark.circle"
                                : "plus.circle.fill"
                        )
                        .font(.system(size: 14, weight: .semibold, design: .rounded))
                        .frame(maxWidth: .infinity, minHeight: 50)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(controller.rules.count >= 8)
                    .accessibilityIdentifier("rule-add")
                }
            }
        }
        .padding(20)
        .preferredColorScheme(.dark)
        .onAppear { controller.loadRules() }
        .onChange(of: action) { _, _ in value = valueOptions.first ?? "" }
    }

    private func chipRow(
        _ options: [String],
        selected: String,
        kind: String,
        onPick: @escaping (String) -> Void
    ) -> some View {
        HStack(spacing: 8) {
            ForEach(options, id: \.self) { option in
                chip(
                    kind == "trigger" ? Copy.ruleTrigger(option) : Copy.ruleKind(option),
                    selected: selected == option,
                    id: "rule-\(kind == "trigger" ? "trigger" : "kind")-\(option)"
                ) { onPick(option) }
            }
        }
    }

    private func chip(
        _ label: String,
        selected: Bool,
        id: String,
        onPick: @escaping () -> Void
    ) -> some View {
        Button(action: onPick) {
            Text(label)
                .font(.system(size: 12, weight: .semibold, design: .rounded))
                .foregroundStyle(selected ? .black : .white.opacity(0.85))
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .background(
                    selected ? Color.white.opacity(0.92) : Color.white.opacity(0.10),
                    in: Capsule()
                )
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(id)
    }

    private var valueOptions: [String] {
        switch action {
        case "skill":
            return ["roulade", "kick_left", "kick_right", "sit", "ground_pick"]
        case "emotion":
            return [
                "excited", "laugh", "curious", "yes", "no", "sad", "angry",
                "defiant", "impatient", "mock", "mmh", "yes_fast", "devastated",
            ]
        default:
            return ["chirp", "coo", "greet", "inquire", "peck", "alarm"]
        }
    }

    private func label(for value: String, kind: String? = nil) -> String {
        switch kind ?? action {
        case "skill", "emotion":
            return SkillLabel.of(value)
        default:
            return value
        }
    }
}
