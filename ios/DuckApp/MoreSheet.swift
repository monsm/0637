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

    @State private var showRules = false
    @State private var showDance = false
    @State private var showPeers = false
    @State private var showQuests = false
    @State private var showDiary = false
    @State private var showPostcards = false
    @State private var showDex = false
    @State private var showAI = false
    @State private var showModes = false

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
                        showRules = true
                    }
                    row(Icons.dance, Copy.dance, note: Copy.danceRowNote, id: "more-dance") {
                        showDance = true
                    }
                    row(
                        Icons.controls, Copy.stickModes,
                        note: Copy.stick(stickMode.rawValue),
                        id: "more-controls"
                    ) {
                        showModes = true
                    }
                }
                group(Copy.groupTogether, id: "together") {
                    row(
                        Icons.peers, Copy.peers,
                        note: Copy.peersRowNote,
                        badge: "\(controller.peers.count)/3",
                        id: "more-peers"
                    ) {
                        showPeers = true
                    }
                }
                group(Copy.groupRecord, id: "record") {
                    row(
                        Icons.quests, Copy.dailyQuests,
                        note: Copy.questsRowNote,
                        badge: questsLeft,
                        id: "more-quests"
                    ) {
                        showQuests = true
                    }
                    row(Icons.diary, Copy.diary, id: "more-diary") {
                        showDiary = true
                    }
                    row(
                        Icons.postcards, Copy.postcardShelf,
                        badge: "\(controller.postcards.cards.count)",
                        id: "more-postcards"
                    ) {
                        showPostcards = true
                    }
                    row(Icons.dex, Copy.dex, note: Copy.dexRowNote, id: "more-dex") {
                        showDex = true
                    }
                }
                group(Copy.thisDuck, id: "duck") {
                    row(
                        Icons.ai, Copy.ai,
                        note: aiNote,
                        dot: !aiConfigured,
                        id: "more-ai"
                    ) {
                        showAI = true
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
        .background(Color(red: 0.055, green: 0.075, blue: 0.10))
        .preferredColorScheme(.dark)
        .sheet(isPresented: $showRules) {
            RulesSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showDance) {
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
        }
        .sheet(isPresented: $showPeers) {
            PeersSheet(controller: controller)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showQuests) {
            ProgressSheet()
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showDiary) {
            DiarySheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showPostcards) {
            PostcardShelfView(shelf: controller.postcards)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showDex) {
            DexSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showAI) {
            AISheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showModes) {
            ModeSheet(controller: controller, stickMode: $stickMode)
                .presentationDetents([.height(320)])
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
        .accessibilityIdentifier("more-group-\(id)")
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
                        .foregroundStyle(Color.cyan)
                }
                if dot {
                    Circle()
                        .fill(Color.orange)
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
                        .foregroundStyle(.tertiary)
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
                                .foregroundStyle(Color.cyan)
                        }
                    }
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("mode-option-\(mode.rawValue)")
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(red: 0.055, green: 0.075, blue: 0.10))
        .preferredColorScheme(.dark)
    }
}