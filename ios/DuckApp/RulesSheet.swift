import SwiftUI

/// The block program: "when I clap, roll over". Two rows of big chips, one
/// list of what is already running. Every change goes straight to the core,
/// which executes it; this screen is an editor, not a second engine.
struct RulesSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var trigger = "clap"
    /// The timer's period, in whole simulated seconds.
    @State private var timerPeriod = 60
    @State private var action = "skill"
    @State private var value = "roulade"
    /// Ordered follow-ups staged before Add. Empty means a classic one-action
    /// rule; up to four keep the program comprehensible.
    @State private var chainDraft: [(String, String)] = []
    /// The last import's verdict, shown under the share row.
    @State private var importNote = ""

    private let triggers = ["clap", "whistle", "pet", "timer"]
    private let kinds = ["skill", "emotion", "sound"]

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack {
                Text(Copy.blocks)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("rules-title")
                    .duckSheetFirstFocus()
                Spacer()
                Button {
                    dismiss()
                } label: {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("rules-close")
            }

            ScrollView {
                VStack(alignment: .leading, spacing: DuckSpace.lg) {
                    if controller.rules.isEmpty {
                        DuckEmptyState(
                            title: Copy.rulesEmpty,
                            actionTitle: Copy.rulesPaste,
                            action: { pasteFromClipboard() },
                            identifier: "rules-empty-state"
                        )
                    } else {
                        ForEach(Array(controller.rules.enumerated()), id: \.offset) { index, rule in
                            ruleView(rule, index: index)
                        }
                    }

                    Divider()

                    // A program travels between phones as text: the share
                    // sheet carries the versioned document, paste reads one
                    // back and merges it into what the child already built.
                    HStack(spacing: DuckSpace.sm) {
                        if !controller.rules.isEmpty {
                            ShareLink(item: controller.rulesShareText) {
                                Label(Copy.rulesShare, systemImage: "square.and.arrow.up")
                                    .font(.duckCaption.weight(.semibold))
                            }
                            .buttonStyle(.bordered)
                            .frame(minHeight: 44)
                            .accessibilityIdentifier("rules-share")
                        }
                        Button {
                            pasteFromClipboard()
                        } label: {
                            Label(Copy.rulesPaste, systemImage: "doc.on.clipboard")
                                .font(.duckCaption.weight(.semibold))
                        }
                        .buttonStyle(.bordered)
                        .frame(minHeight: 44)
                        .accessibilityIdentifier("rules-paste")
                        Spacer()
                    }
                    if !importNote.isEmpty {
                        Text(importNote)
                            .font(.duckCaption)
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("rules-import-note")
                    }
                    if !controller.rulesError.isEmpty {
                        Text(controller.rulesError)
                            .font(.duckCaption)
                            .foregroundStyle(.orange)
                            .accessibilityIdentifier("rules-error")
                    }

                    Divider()

                    Text(Copy.ruleWhen)
                        .font(.duckLabel.weight(.semibold))
                    chipRow(triggers, selected: trigger, kind: "trigger") { trigger = $0 }
            if trigger == "timer" {
                // The WHEN, in the duck's own clock: four honest beats.
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: DuckSpace.xs) {
                        ForEach([30, 60, 120, 300], id: \.self) { seconds in
                            Button {
                                timerPeriod = seconds
                            } label: {
                                Text("\(seconds)s")
                                    .font(.duckCaption.weight(.semibold))
                                    .padding(.horizontal, DuckSpace.sm)
                                    .frame(minHeight: 44)
                                    .background(
                                        timerPeriod == seconds
                                            ? Color.white.opacity(0.28)
                                            : Color.white.opacity(0.08),
                                        in: Capsule()
                                    )
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("rule-timer-\(seconds)")
                        }
                    }
                }
            }

                    Text(Copy.ruleThen)
                        .font(.duckLabel.weight(.semibold))
                    chipRow(kinds, selected: action, kind: "action") { action = $0 }
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: DuckSpace.sm) {
                            ForEach(valueOptions, id: \.self) { option in
                                chip(
                                    label(for: option),
                                    selected: value == option,
                                    id: "rule-value-\(option)"
                                ) { value = option }
                            }
                        }
                        .padding(.vertical, DuckSpace.xxs)
                    }

                    // Chain: stage follow-up blocks in order. A child can
                    // build "clap -> roll -> laugh -> chirp" without typing.
                    HStack(spacing: DuckSpace.sm) {
                        Text(Copy.ruleChain)
                            .font(.duckCaption.weight(.medium))
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button {
                            guard chainDraft.count < 4 else { return }
                            chainDraft.append((action, value))
                        } label: {
                            Label(Copy.ruleChainAdd, systemImage: "plus")
                                .font(.duckCaption.weight(.semibold))
                        }
                        .buttonStyle(.bordered)
                        .disabled(chainDraft.count >= 4)
                        .accessibilityIdentifier("rule-chain-add")
                    }
                    if !chainDraft.isEmpty {
                        HStack(spacing: DuckSpace.xs) {
                            ForEach(Array(chainDraft.enumerated()), id: \.offset) { index, step in
                                Button {
                                    chainDraft.remove(at: index)
                                } label: {
                                    Text(label(for: step.1, kind: step.0))
                                        .font(.duckCaption.weight(.semibold))
                                        .padding(.horizontal, DuckSpace.sm)
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
                            then: chainDraft.map { DuckController.DuckRuleStep(action: $0.0, value: $0.1) },
                            periodS: trigger == "timer" ? timerPeriod : nil
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
                        .font(.duckLabel.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 50)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(controller.rules.count >= 8)
                    .accessibilityIdentifier("rule-add")
                }
            }
        }
        .padding(DuckSpace.xl)
        .onAppear { controller.loadRules() }
        .onChange(of: action) { _, _ in value = valueOptions.first ?? "" }
    }

    private func chipRow(
        _ options: [String],
        selected: String,
        kind: String,
        onPick: @escaping (String) -> Void
    ) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: DuckSpace.sm) {
                ForEach(options, id: \.self) { option in
                    chip(
                        kind == "trigger" ? Copy.ruleTrigger(option) : Copy.ruleKind(option),
                        selected: selected == option,
                        id: "rule-\(kind == "trigger" ? "trigger" : "kind")-\(option)"
                    ) { onPick(option) }
                }
            }
            .padding(.vertical, DuckSpace.xxs)
        }
    }

    /// One existing rule. The row becomes a column at accessibility sizes so
    /// "when" and "then" never squeeze each other into an ellipsis.
    @ViewBuilder private func ruleView(_ rule: DuckController.DuckRule, index: Int) -> some View {
        if typeSize.duckIsAccessibility {
            VStack(alignment: .leading, spacing: DuckSpace.xs) {
                Text("\(Copy.ruleWhen) \(Copy.ruleTrigger(rule.trigger))")
                    .font(.duckLabel.weight(.semibold))
                Text(label(for: rule.value, kind: rule.action))
                    .font(.duckLabel.weight(.semibold))
                HStack {
                    Spacer()
                    deleteRuleButton(index: index)
                }
            }
            .padding(.horizontal, DuckSpace.md)
            .padding(.vertical, DuckSpace.sm)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                Color.duckBgSurface,
                in: RoundedRectangle(cornerRadius: DuckRadius.thumb)
            )
            .accessibilityIdentifier("rule-\(index)")
        } else {
            HStack(spacing: DuckSpace.sm) {
                Text("\(Copy.ruleWhen) \(Copy.ruleTrigger(rule.trigger))")
                    .font(.duckLabel.weight(.semibold))
                Image(systemName: "arrow.right")
                    .font(DuckIcon.xs)
                    .foregroundStyle(.secondary)
                Text(label(for: rule.value, kind: rule.action))
                    .font(.duckLabel.weight(.semibold))
                Spacer()
                deleteRuleButton(index: index)
            }
            .padding(.horizontal, DuckSpace.md)
            .frame(minHeight: 48)
            .background(
                Color.duckBgSurface,
                in: RoundedRectangle(cornerRadius: DuckRadius.thumb)
            )
            .accessibilityIdentifier("rule-\(index)")
        }
    }

    private func deleteRuleButton(index: Int) -> some View {
        Button {
            var next = controller.rules
            next.remove(at: index)
            controller.saveRules(next)
        } label: {
            Image(systemName: "trash")
                .font(DuckIcon.xs)
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(.red.opacity(0.85))
        .accessibilityLabel(Copy.deleteRule)
    }

    private func pasteFromClipboard() {
        if let text = UIPasteboard.general.string,
           let result = controller.importRules(from: text) {
            if result.parsed == 0 && result.skippedTimers == 0 {
                importNote = Copy.rulesPasteFailed
            } else {
                // A timer the core would refuse is named, not silently absent
                // from the imported set (the same rule Android follows):
                // saving it would make the core reject the whole program.
                var notes: [String] = []
                if result.skippedTimers > 0 {
                    notes.append(Copy.rulesTimerPeriodBad)
                }
                if result.parsed > 0 {
                    notes.append(Copy.rulesPasted(result.added))
                }
                importNote = notes.joined(separator: "\n")
            }
        } else {
            importNote = Copy.rulesPasteFailed
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
                .font(.duckCaption.weight(.semibold))
                .foregroundStyle(selected ? .black : .white.opacity(0.85))
                .padding(.horizontal, DuckSpace.md)
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
