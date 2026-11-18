import SwiftUI

/// The route table's destinations in one place (B7).
///
/// It lives outside ContentView so the shell file keeps shrinking instead
/// of growing a second switch, and so the drawer and the instrument hand
/// their rows to the same table instead of owning nested `.sheet` trees.
/// Every case is a `DuckRoute` case; the shell presents this view as the one
/// sheet it owns.
struct DuckRouteSheets: View {
    let route: DuckRoute
    let portrait: Bool
    @ObservedObject var controller: DuckController
    @Binding var stickMode: StickMode
    @Binding var languageTick: Int
    let photo: DuckController.CapturedPhoto?
    let onDanceStep: (DanceStep) -> Void
    /// A row inside the drawer or the instrument opens its page on top of the
    /// current sheet. Nesting is the behaviour the old UI tests pin (AI closes
    /// back to the instrument), so one vocabulary keeps the presentation
    /// hierarchical instead of flattening it.
    @State private var child: DuckRoute?

    var body: some View {
        content
            .sheet(item: $child) { next in
                DuckRouteSheets(
                    route: next,
                    portrait: portrait,
                    controller: controller,
                    stickMode: $stickMode,
                    languageTick: $languageTick,
                    photo: photo,
                    onDanceStep: onDanceStep
                )
            }
    }

    @ViewBuilder
    private var content: some View {
        switch route {
        case .instrument:
            InstrumentSheet(
                controller: controller,
                onOpen: { child = $0 }
            )
            .presentationDetents(portrait ? [.medium, .large] : [.large])
            .presentationDragIndicator(.visible)
        case .photo:
            if let photo {
                PhotoPreviewSheet(
                    photo: photo,
                    shelf: controller.postcards
                )
            }
        case .rules:
            RulesSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        case .peers:
            PeersSheet(controller: controller)
                .presentationDetents(portrait ? [.medium, .large] : [.large])
                .presentationDragIndicator(.visible)
        case .ai:
            AISheet(controller: controller)
                .presentationDetents(portrait ? [.medium, .large] : [.large])
                .presentationDragIndicator(.visible)
        case .dance:
            DanceSheet(
                studio: controller.dance,
                execute: onDanceStep,
                tryStep: { kind, value in
                    let label = kind == "sound"
                        ? Copy.danceStepSound
                        : SkillLabel.of(value)
                    controller.dance.record(
                        kind,
                        value: value,
                        label: label
                    )
                    onDanceStep(DanceStep(
                        kind: kind,
                        value: value,
                        label: label
                    ))
                }
            )
            .presentationDetents([.large])
            .presentationDragIndicator(.visible)
        case .progress:
            ProgressSheet(controller: controller)
                .presentationDetents(portrait ? [.medium, .large] : [.large])
                .presentationDragIndicator(.visible)
        case .more:
            MoreSheet(
                controller: controller,
                stickMode: $stickMode,
                languageTick: $languageTick,
                onDanceStep: onDanceStep,
                onOpen: { child = $0 }
            )
            .presentationDetents([.large])
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
            CollectionSheet(controller: controller)
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
        case .playMore:
            PlayMoreSheet(
                controller: controller,
                stickMode: $stickMode,
                onOpen: { child = $0 }
            )
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
        case .duckPanel:
            DuckPanelSheet(
                controller: controller,
                languageTick: $languageTick,
                onOpen: { child = $0 }
            )
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
        case .tools:
            ToolsSheet(controller: controller, onOpen: { child = $0 })
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        case .backup:
            BackupSheet(controller: controller)
                .presentationDetents([.medium])
                .presentationDragIndicator(.visible)
        case .ab:
            ABSheet(controller: controller)
                .presentationDetents([.medium])
                .presentationDragIndicator(.visible)
        }
    }
}
