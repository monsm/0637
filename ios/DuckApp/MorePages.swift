import SwiftUI

/// The three destination pages the IA reorder added (boss ruling 2026-11-16):
/// the drawer keeps four groups x four rows, and the rows that used to cram
/// the duck group move into these pages. Each page carries at most six
/// controls - the boss's cap - and a control that needs a full sheet (A/B,
/// big buttons, backup) nests it through the same route opener the drawer
/// uses.
///
/// The grouping is the single source of truth in contracts/features.yaml;
/// this file renders the three pages it defines.

private struct PageChrome<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    init(title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: DuckSpace.sm) {
                HStack {
                    Text(title)
                        .font(.duckTitle)
                        .foregroundStyle(.white)
                    Spacer()
                    SheetCloseButton()
                }
                content
            }
            .padding(.horizontal, DuckSpace.xl)
            .padding(.top, DuckSpace.md)
            .padding(.bottom, DuckSpace.xxl)
        }
        .background(Color.duckBgSurface)
    }
}

private struct PageRow: View {
    let title: String
    var note: String? = nil
    var selected: Bool = false
    let id: String
    let action: () -> Void

    var body: some View {
        // The page row IS the shared list row now (D2): one implementation,
        // the trailing on/off word and its colour included.
        DuckListRow(
            title: title,
            note: note,
            value: selected ? Copy.stateOn : Copy.stateOff,
            selected: selected,
            identifier: id,
            action: action
        )
    }
}

/// 玩一玩: the four play controls that left the drawer (B4 keeps the door).
struct PlayMoreSheet: View {
    @ObservedObject var controller: DuckController
    @Binding var stickMode: StickMode
    let onOpen: (DuckRoute) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        PageChrome(title: Copy.playMore) {
            // The four play controls share one segmented control (D2).
            DuckSegmented(
                options: StickMode.allCases.map { Copy.stick($0.rawValue) },
                selectedIndex: StickMode.allCases.firstIndex(of: stickMode) ?? 0,
                onSelect: { index in
                    let mode = StickMode.allCases[index]
                    stickMode = mode
                    controller.setHead(yaw: 0, pitch: 0)
                    controller.releaseBody()
                },
                identifier: "play-more-mode-stick"
            )
            PageRow(
                title: Copy.abTry,
                note: Copy.abNote,
                selected: false,
                id: "play-more-ab"
            ) { onOpen(.ab) }
            PageRow(
                title: Copy.pushCompare,
                selected: false,
                id: "play-more-push"
            ) { controller.pushCompare() }
            PageRow(
                title: Copy.hud,
                selected: controller.hudOn,
                id: "play-more-hud"
            ) { controller.hudOn.toggle() }
        }
    }
}

/// 这只鸭子: the duck group's own toggles (heart, flashlight, language,
/// compass, teach-words, tilt) - six controls, under the cap. Big buttons
/// moved to the tools page together with Android (2026-11-19 H2: the panel
/// was at the seven-control reality; the contract reordered it to six).
struct DuckPanelSheet: View {
    @ObservedObject var controller: DuckController
    @Binding var languageTick: Int
    /// The words page (G2). A local presentation, the same pattern the
    /// instrument used: the route table is pinned by scripts/check-routes.py,
    /// so a sheet from inside a page is a local `.sheet`, not a route case.
    @State private var aliasOpen = false

    var body: some View {
        PageChrome(title: Copy.thisDuck) {
            PageRow(
                title: Copy.heart,
                note: controller.heartOn
                    ? String(format: "%.0f bpm", controller.heartBpm)
                    : Copy.heartRowNote,
                selected: controller.heartOn,
                id: "duck-panel-heart"
            ) { controller.heartOn.toggle() }
            PageRow(
                title: Copy.flashlight,
                note: controller.flashlightColor == nil ? Copy.flashlightOff : Copy.flashlightOn,
                selected: controller.flashlightColor != nil,
                id: "duck-panel-flashlight"
            ) { controller.toggleFlashlight() }
            PageRow(
                title: Copy.languageRow,
                note: Copy.language.rawValue,
                selected: false,
                id: "duck-panel-language"
            ) {
                Copy.language = Copy.language == .english ? .chinese : .english
                languageTick &+= 1
            }
            PageRow(
                title: Copy.compass,
                note: Copy.compassNote,
                selected: controller.compassOn,
                id: "duck-panel-compass"
            ) { controller.compassOn.toggle() }
            PageRow(
                title: controller.tiltLook ? Copy.tiltLookOn : Copy.tiltLook,
                selected: controller.tiltLook,
                id: "duck-panel-tilt"
            ) { controller.tiltLook.toggle() }
            PageRow(
                title: Copy.aliasTitle,
                note: Copy.aliasLocalOnly,
                selected: false,
                id: "alias-open"
            ) { aliasOpen = true }
        }
        .sheet(isPresented: $aliasOpen) {
            AliasSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
    }
}

/// 工具: the parent's tools, plus big buttons relocated here so the "This
/// duck" panel fits under the six-control cap (2026-11-19 H2, both phones
/// moved together; the row keeps its duck-panel-big id). Days (2026-11-19
/// K2) is the sixth: a local sheet — the route table is pinned by
/// scripts/check-routes.py — showing the core's own day record.
struct ToolsSheet: View {
    @ObservedObject var controller: DuckController
    let onOpen: (DuckRoute) -> Void
    /// The days board. A local presentation, the same pattern the duck
    /// panel's words page uses (see DuckPanelSheet.aliasOpen).
    @State private var daysOpen = false

    private var parentNote: String {
        controller.parentMinutes.map { minutes in
            Copy.parentLeft(minutes, Int(controller.parentLeftS / 60.0))
        } ?? Copy.parentOff
    }

    private var lampNote: String {
        "\(Copy.roomLampNote) \(String(format: "%.0f%%", controller.ambientScale * 100))"
    }

    var body: some View {
        PageChrome(title: Copy.tools) {
            PageRow(
                title: Copy.parentTime,
                note: parentNote,
                selected: controller.parentMinutes != nil,
                id: "tools-parent"
            ) {
                let next = (controller.parentMinutes ?? 0) == 0 ? 15
                    : (controller.parentMinutes == 15 ? 30
                    : (controller.parentMinutes == 30 ? 60 : 0))
                controller.setParent(minutes: next)
            }
            PageRow(
                title: Copy.roomLamp,
                note: lampNote,
                selected: false,
                id: "tools-lamp"
            ) {
                let stops: [Double] = [0.55, 0.7, 0.85, 1.0, 1.15]
                let index = stops.firstIndex(of: controller.ambientScale) ?? 3
                controller.ambientScale = stops[(index + 1) % stops.count]
            }
            PageRow(
                title: Copy.backup,
                note: Copy.backupRowNote,
                selected: false,
                id: "tools-backup"
            ) { onOpen(.backup) }
            PageRow(
                title: Copy.tof,
                note: controller.tofStrip,
                selected: false,
                id: "tools-tof"
            ) { controller.refreshTofStrip() }
            PageRow(
                title: Copy.bigButtons,
                note: Copy.bigButtonsNote,
                selected: false,
                id: "duck-panel-big"
            ) { onOpen(.big) }
            PageRow(
                title: Copy.daysTitle,
                selected: false,
                id: "days-open"
            ) { daysOpen = true }
        }
        .sheet(isPresented: $daysOpen) {
            DaysSheet()
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
    }
}
