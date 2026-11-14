import SwiftUI

/// The stick modes the stage understands. It lives outside ContentView so
/// the drawer and the stage share one vocabulary.
enum StickMode: String, CaseIterable {
    case drive, head, body, path
}

/// The one "More" drawer (B4 §2): four groups, at most four rows each.
///
/// Everything the first screen used to hide behind a menu lives here — one
/// tap from Live, and nothing inside is a second layer of menus. The drawer
/// is the same component on both platforms; only the icons are translated.
///
/// Decisions already taken: the "Together" group is one row — 邻居 — because
/// the chorus and gifts are actions inside the neighbours page, not pages of
/// their own; and tilt-to-look is a drive mode, so it stays in the "This
/// duck" panel instead of the drawer.
struct MoreSheet: View {
    @ObservedObject var controller: DuckController
    @Binding var stickMode: StickMode
    /// The language tick: the chip flips Copy.language, and this sheet has to
    /// read the new words.
    @Binding var languageTick: Int
    /// The dance sheet replays through the shell's own step runner.
    let onDanceStep: (DanceStep) -> Void
    @Environment(\.dismiss) private var dismiss

    /// Where a drawer row leads. One sheet, one destination: a view with
    /// several `.sheet` modifiers is unreliable, and this codebase says so
    /// about itself in ContentView.
    private enum Destination: Identifiable {
        case rules, dance, peers, quests, diary, postcards, dex, ai, modes, big, backup

        var id: String {
            switch self {
            case .rules: return "rules"
            case .dance: return "dance"
            case .peers: return "peers"
            case .quests: return "quests"
            case .diary: return "diary"
            case .postcards: return "postcards"
            case .dex: return "dex"
            case .ai: return "ai"
            case .modes: return "modes"
            case .big: return "big"
            case .backup: return "backup"
            }
        }
    }
    @State private var destination: Destination?

    var body: some View {
        let _ = languageTick
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                group(Copy.groupPlay, id: "play") {
                    row(
                        Icons.path, Copy.pathMode,
                        note: Copy.pathRowNote,
                        check: stickMode == .path,
                        id: "more-path"
                    ) {
                        stickMode = .path
                        controller.setHead(yaw: 0, pitch: 0)
                        controller.releaseBody()
                        dismiss()
                    }
                    row(Icons.blocks, Copy.blocks, note: Copy.blocksRowNote, id: "more-blocks") {
                        destination = .rules
                    }
                    row(Icons.dance, Copy.dance, note: Copy.danceRowNote, id: "more-dance") {
                        destination = .dance
                    }
                    row(
                        Icons.controls, Copy.stickModes,
                        note: Copy.stick(stickMode.rawValue),
                        id: "more-controls"
                    ) {
                        destination = .modes
                    }
                }
                group(Copy.groupTogether, id: "together") {
                    row(
                        Icons.peers, Copy.peers,
                        note: Copy.peersRowNote,
                        badge: "\(controller.peers.count)/3",
                        id: "more-peers"
                    ) {
                        destination = .peers
                    }
                }
                group(Copy.groupRecord, id: "record") {
                    row(
                        Icons.quests, Copy.dailyQuests,
                        note: Copy.questsRowNote,
                        badge: questsLeft,
                        id: "more-quests"
                    ) {
                        destination = .quests
                    }
                    row(Icons.diary, Copy.diary, id: "more-diary") {
                        destination = .diary
                    }
                    row(
                        Icons.postcards, Copy.postcardShelf,
                        badge: "\(controller.postcards.cards.count)",
                        id: "more-postcards"
                    ) {
                        destination = .postcards
                    }
                    row(Icons.dex, Copy.dex, note: Copy.dexRowNote, id: "more-dex") {
                        destination = .dex
                    }
                }
                group(Copy.thisDuck, id: "duck") {
                    row(
                        Icons.ai, Copy.ai,
                        note: aiNote,
                        dot: !aiConfigured,
                        id: "more-ai"
                    ) {
                        destination = .ai
                    }
                    row(
                        controller.nightActive ? Icons.morning : Icons.night,
                        controller.nightActive ? Copy.goodMorning : Copy.goodNight,
                        note: controller.nightActive ? Copy.morningRowNote : Copy.nightRowNote,
                        showsChevron: false,
                        id: "more-night"
                    ) {
                        controller.postNight(on: !controller.nightActive)
                    }
                    row(
                        Icons.flashlight, Copy.flashlight,
                        note: controller.flashlightColor == nil ? Copy.flashlightOff : Copy.flashlightOn,
                        check: controller.flashlightColor != nil,
                        showsChevron: false,
                        id: "more-flashlight"
                    ) {
                        controller.toggleFlashlight()
                    }
                    row(
                        "externaldrive", Copy.backup,
                        note: Copy.backupRowNote,
                        id: "more-backup"
                    ) {
                        destination = .backup
                    }
                    row(
                        "location.north", Copy.compass,
                        note: Copy.compassNote,
                        check: controller.compassOn,
                        showsChevron: false,
                        id: "more-compass"
                    ) {
                        controller.compassOn.toggle()
                    }
                    row(
                        "lightbulb", Copy.roomLamp,
                        note: Copy.roomLampNote,
                        badge: String(format: "%.0f%%", controller.ambientScale * 100),
                        showsChevron: false,
                        id: "more-lamp"
                    ) {
                        // Five honest stops across the readable range.
                        let stops: [Double] = [0.55, 0.7, 0.85, 1.0, 1.15]
                        let index = stops.firstIndex(of: controller.ambientScale) ?? 3
                        controller.ambientScale = stops[(index + 1) % stops.count]
                    }
                    row(
                        "hourglass", Copy.parentTime,
                        note: controller.parentMinutes.map { minutes in
                            String(format: Copy.parentLeft, minutes, Int(controller.parentLeftS / 60.0))
                        } ?? Copy.parentOff,
                        showsChevron: false,
                        id: "more-parent"
                    ) {
                        // Off -> 15 -> 30 -> 60 -> off: one tap, one honest
                        // choice, no second-layer menu.
                        let next = (controller.parentMinutes ?? 0) == 0 ? 15
                            : (controller.parentMinutes == 15 ? 30
                            : (controller.parentMinutes == 30 ? 60 : 0))
                        controller.setParent(minutes: next)
                    }
                    row(
                        "hand.tap", Copy.bigButtons,
                        note: Copy.bigButtonsNote,
                        id: "more-big"
                    ) {
                        destination = .big
                    }
                    row(
                        Icons.heart, Copy.heart,
                        note: controller.heartOn ? String(format: "%.0f bpm", controller.heartBpm) : Copy.heartRowNote,
                        check: controller.heartOn,
                        showsChevron: false,
                        id: "more-heart"
                    ) {
                        controller.heartOn.toggle()
                    }
                    row(
                        "globe", Copy.languageRow,
                        note: Copy.language.rawValue,
                        showsChevron: false,
                        id: "more-language"
                    ) {
                        Copy.language = Copy.language == .english ? .chinese : .english
                        languageTick &+= 1
                    }
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 14)
            .padding(.bottom, 28)
        }
        .background(Color.sheetSurface)
        .preferredColorScheme(.dark)
        .sheet(item: $destination) { destination in
            sheet(for: destination)
        }
    }

    /// The screen behind one drawer row. It lives in its own function so the
    /// drawer's body stays small enough for the type checker.
    @ViewBuilder
    private func sheet(for destination: Destination) -> some View {
        switch destination {
        case .rules:
            RulesSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        case .dance:
            DanceSheet(
                studio: controller.dance,
                execute: { step in onDanceStep(step) },
                tryStep: { kind, value in
                    let label = kind == "sound"
                        ? Copy.danceStepSound
                        : SkillLabel.of(value)
                    controller.dance.record(kind, value: value, label: label)
                    onDanceStep(DanceStep(kind: kind, value: value, label: label))
                }
            )
            .presentationDetents([.large])
            .presentationDragIndicator(.visible)
        case .peers:
            PeersSheet(controller: controller)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        case .quests:
            ProgressSheet()
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        case .diary:
            DiarySheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        case .postcards:
            PostcardShelfView(shelf: controller.postcards)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        case .dex:
            DexSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        case .ai:
            AISheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        case .modes:
            ModeSheet(controller: controller, stickMode: $stickMode)
                .presentationDetents([.height(320)])
                .presentationDragIndicator(.visible)
        case .big:
            BigButtonsSheet(controller: controller)
                .presentationDetents([.height(400)])
                .presentationDragIndicator(.visible)
        case .backup:
            BackupSheet(controller: controller)
                .presentationDetents([.medium])
                .presentationDragIndicator(.visible)
        }
    }

    // MARK: - Rows

    private var header: some View {
        HStack {
            Text(Copy.more)
                .font(.system(size: 20, weight: .semibold, design: .rounded))
                .foregroundStyle(.white)
                .accessibilityIdentifier("more-title")
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
            .accessibilityIdentifier("more-close")
        }
    }

    private func group<Content: View>(
        _ title: String,
        id: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.system(size: 13, weight: .semibold, design: .rounded))
                .foregroundStyle(.secondary)
                .padding(.leading, 4)
                .padding(.bottom, 2)
            VStack(spacing: 2) { content() }
        }
    }

    /// One row: 48 pt tall, a 20 pt icon, a 15 pt title, an optional 12 pt
    /// note, and a badge on the right. Text capsules are for chips, not for
    /// actions (B4 §3).
    private func row(
        _ icon: String,
        _ title: String,
        note: String? = nil,
        badge: String? = nil,
        dot: Bool = false,
        check: Bool = false,
        showsChevron: Bool = true,
        id: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.system(size: 20))
                    .foregroundStyle(.white.opacity(0.85))
                    .frame(width: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 15, weight: .medium, design: .rounded))
                        .foregroundStyle(.white)
                    if let note {
                        Text(note)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                if check {
                    Image(systemName: "checkmark")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color.actionAccent)
                }
                if dot {
                    Circle()
                        .fill(Color.actionAccent)
                        .frame(width: 8, height: 8)
                }
                if let badge {
                    Text(badge)
                        .font(.system(size: 12, weight: .medium, design: .rounded))
                        .foregroundStyle(.secondary)
                }
                if showsChevron {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.secondary)
                }
            }
            .frame(minHeight: 48)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(id)
    }

    // MARK: - Badges

    /// How many of today's goals are still open — the board's own counters.
    private var questsLeft: String? {
        let left = controller.quests.filter { !$0.done }.count
        return controller.quests.isEmpty ? nil : "\(left)"
    }

    private var aiConfigured: Bool {
        controller.aiConfig.enabled
            && !controller.aiConfig.endpoint.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private var aiNote: String {
        aiConfigured ? Copy.aiOn : Copy.aiNeedEndpoint
    }
}

/// The three stick modes, one tap each. Path has its own row in the drawer;
/// this panel is the rest of the pad. Tilt-to-look is also a drive mode, but
/// it lives in the "This duck" panel, not here.
struct ModeSheet: View {
    @ObservedObject var controller: DuckController
    @Binding var stickMode: StickMode
    @Environment(\.dismiss) private var dismiss

    private let modes: [StickMode] = [.drive, .head, .body]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(Copy.stickModes)
                .font(.system(size: 18, weight: .semibold, design: .rounded))
                .foregroundStyle(.white)
            ForEach(modes, id: \.self) { mode in
                Button {
                    stickMode = mode
                    controller.setHead(yaw: 0, pitch: 0)
                    controller.releaseBody()
                    dismiss()
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: Icons.stick(mode))
                            .font(.system(size: 18))
                            .foregroundStyle(.white.opacity(0.85))
                            .frame(width: 26)
                        Text(Copy.stick(mode.rawValue))
                            .font(.system(size: 15, weight: .medium, design: .rounded))
                            .foregroundStyle(.white)
                        Spacer()
                        if stickMode == mode {
                            Image(systemName: "checkmark")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(Color.actionAccent)
                        }
                    }
                    .frame(minHeight: 48)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("mode-option-\(mode.rawValue)")
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.sheetSurface)
        .preferredColorScheme(.dark)
    }
}
/// UI_TOKENS v1 literals until B2 grows `DuckTheme.swift`: a new component
/// must not invent another surface or accent (design review 2026-11-13).
private extension Color {
    /// bg.surface #221F1D — the opaque sheet surface, warm like the room
    /// (UI_TOKENS §4).
    static let sheetSurface = Color(red: 0.133, green: 0.122, blue: 0.114)
    /// accent.action #FF8A3D — selection, checks and attention dots.
    static let actionAccent = Color(red: 1.0, green: 0.541, blue: 0.239)
}
