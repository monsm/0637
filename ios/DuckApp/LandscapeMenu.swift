import SwiftUI

/// The landscape top bar's menu (boss ruling 2026-11-16): four groups in
/// exactly the portrait drawer's order, consuming the one DuckRoute table
/// instead of keeping a second menu of its own. It lives beside the drawer
/// rather than inside ContentView so the shell file does not keep growing.
struct LandscapeMenu: View {
    @ObservedObject var controller: DuckController
    @Binding var stickMode: StickMode
    @Binding var surface: ContentView.Surface
    let onOpen: (DuckRoute) -> Void
    /// The words page (G2). The route table is pinned by
    /// scripts/check-routes.py, so this presents AliasSheet locally rather
    /// than adding a DuckRoute case. Not named `show*`: that prefix is the
    /// route gate's loose-state scent.
    @State private var aliasOpen = false

    var body: some View {
        Menu {
                // Same four groups, same order as the portrait drawer
                // (boss ruling 2026-11-16): landscape consumes the one route
                // table instead of keeping a second menu of its own.
                Section(header: Text(Copy.groupPlay)) {
                    ForEach(StickMode.allCases, id: \.self) { option in
                        Button {
                            stickMode = option
                            controller.setHead(yaw: 0, pitch: 0)
                            controller.releaseBody()
                    } label: {
                            Label(Copy.stick(option.rawValue), systemImage: stickIcon(option))
                        }
                        // One id per mode, path included: the UI test opens
                        // the Path Painter through this menu entry.
                        .accessibilityIdentifier("mode-\(option.rawValue)")
                    }
                    Button { onOpen(.rules) } label: {
                        Label(Copy.blocks, systemImage: "square.grid.2x2.fill")
                    }
                    .accessibilityIdentifier("open-rules")
                    Button { onOpen(.dance) } label: {
                        Label(Copy.dance, systemImage: "music.note.list")
                    }
                    .accessibilityIdentifier("open-dance")
                    Button { onOpen(.playMore) } label: {
                        Label(Copy.playMore, systemImage: Icons.controls)
                    }
                    .accessibilityIdentifier("open-playmore")
                }
                Section(header: Text(Copy.groupTogether)) {
                    Button { onOpen(.peers) } label: {
                        Label(Copy.peers, systemImage: "dot.radiowaves.left.and.right")
                    }
                    .accessibilityIdentifier("open-peers")
                    Button { onOpen(.peers) } label: {
                        Label(Copy.chorus, systemImage: Icons.chorus)
                    }
                    .accessibilityIdentifier("open-chorus")
                    Button { onOpen(.peers) } label: {
                        Label(Copy.gifts, systemImage: Icons.gift)
                    }
                    .accessibilityIdentifier("open-gifts")
                    Button { controller.p2On.toggle() } label: {
                        Label(Copy.twoPlayer, systemImage: "person.2")
                    }
                    .accessibilityIdentifier("open-p2")
                }
                Section(header: Text(Copy.groupRecord)) {
                    Button { onOpen(.progress) } label: {
                        Label(Copy.dailyQuests, systemImage: Icons.quests)
                    }
                    .accessibilityIdentifier("open-progress")
                    Button { onOpen(.diary) } label: {
                        Label(Copy.diary, systemImage: Icons.diary)
                    }
                    .accessibilityIdentifier("open-diary")
                    Button { onOpen(.postcards) } label: {
                        Label(Copy.postcardShelf, systemImage: Icons.postcards)
                    }
                    .accessibilityIdentifier("open-postcards")
                    Button { onOpen(.dex) } label: {
                        Label(Copy.collectionTitle, systemImage: Icons.dex)
                    }
                    .accessibilityIdentifier("open-dex")
                }
                Section(header: Text(Copy.thisDuck)) {
                    Button { onOpen(.ai) } label: {
                        Label(Copy.ai, systemImage: "brain.head.profile")
                    }
                    .accessibilityIdentifier("open-ai")
                    Button { aliasOpen = true } label: {
                        Label(Copy.aliasTitle, systemImage: "text.bubble.fill")
                    }
                    .accessibilityIdentifier("alias-open")
                    Button { controller.postNight(on: !controller.nightActive) } label: {
                        Label(
                            controller.nightActive ? Copy.goodMorning : Copy.goodNight,
                            systemImage: controller.nightActive ? Icons.morning : Icons.night
                        )
                    }
                    .accessibilityIdentifier("open-night")
                    Button { onOpen(.duckPanel) } label: {
                        Label(Copy.duckFace, systemImage: Icons.preferences)
                    }
                    .accessibilityIdentifier("open-duckpanel")
                    Button { onOpen(.tools) } label: {
                        Label(Copy.tools, systemImage: "wrench.and.screwdriver")
                    }
                    .accessibilityIdentifier("open-tools")
                }
                Divider()
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = .live }
            } label: {
                    Label(Copy.live, systemImage: "circle.dashed")
                }
                .accessibilityIdentifier("surface-live")
                Button {
                    withAnimation(.snappy(duration: 0.2)) { surface = .director }
            } label: {
                    Label(Copy.director, systemImage: "viewfinder")
                }
                .accessibilityIdentifier("surface-director")
        } label: {
                Image(systemName: "ellipsis")
                    .font(.body.weight(.semibold))
                    .frame(width: 48, height: 48)
                    .background(.ultraThinMaterial, in: Circle())
                    .contentShape(Circle())
            }
        .accessibilityLabel(Copy.menu)
        .accessibilityIdentifier("duck-menu")
        .sheet(isPresented: $aliasOpen) {
            AliasSheet(controller: controller)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
        }
    }

    /// One icon per stick mode, so the menu reads at a glance instead of
    /// being a row of identical words.
    private func stickIcon(_ mode: StickMode) -> String {
        switch mode {
        case .drive: return "figure.walk"
        case .head: return "eye"
        case .body: return "arrow.up.and.down"
        case .path: return "scribble"
        }
    }
}
