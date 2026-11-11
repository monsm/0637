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

                    Text(Copy.ruleWhen)
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                    chipRow(triggers, selected: trigger, prefix: "rule-trigger") { trigger = $0 }

                    Text(Copy.ruleThen)
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                    chipRow(kinds, selected: action, prefix: "rule-kind") { action = $0 }
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

                    Button {
                        var next = controller.rules
                        next.append(.init(trigger: trigger, action: action, value: value))
                        controller.saveRules(next)
                    } label: {
                        Label(Copy.addRule, systemImage: "plus.circle.fill")
                            .font(.system(size: 14, weight: .semibold, design: .rounded))
                            .frame(maxWidth: .infinity, minHeight: 50)
                    }
                    .buttonStyle(.borderedProminent)
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
        prefix: String,
        onPick: @escaping (String) -> Void
    ) -> some View {
        HStack(spacing: 8) {
            ForEach(options, id: \.self) { option in
                chip(
                    option == trigger && prefix == "rule-trigger"
                        ? Copy.ruleTrigger(option)
                        : (prefix == "rule-kind" ? Copy.ruleKind(option) : option),
                    selected: selected == option,
                    id: "\(prefix)-\(option)"
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
