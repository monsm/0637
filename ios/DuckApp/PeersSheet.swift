import SwiftUI

/// The other ducks on this Wi-Fi. Discovery is the core's UDP announce; a
/// greeting is one of the duck's social reactions, and the answer comes back
/// as "X answered!". No address is typed by the user.
struct PeersSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss
    /// The six digits BOTH people typed. Local UI state only: the core
    /// holds its own copy and refuses anything but six ASCII digits.
    @State private var pairCode = ""

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack {
                VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                    Text(Copy.peers)
                        .font(.duckTitle.weight(.semibold))
                        .accessibilityIdentifier("peers-title")
                    if !controller.peerName.isEmpty {
                        Text("\(Copy.name) · \(controller.peerName)")
                            .font(.duckCaption)
                            .foregroundStyle(.secondary)
                    }
                }
                Spacer()
                Button {
                    dismiss()
                } label: {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("peers-close")
            }

            // Pairing: a stranger on the Wi-Fi is visible but cannot send
            // gifts or start a chorus until both sides typed this code.
            VStack(alignment: .leading, spacing: DuckSpace.xs) {
                TextField(Copy.pairCode, text: $pairCode)
                    .keyboardType(.numberPad)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("peers-pair-code")
                Text(Copy.pairNote)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            }

            if controller.peers.isEmpty {
                Text(Copy.peersEmpty)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("peers-empty")
                if controller.peerName.isEmpty {
                    Button {
                        controller.startPeers()
                    } label: {
                        Label(Copy.peerFind, systemImage: "wifi")
                            .font(.duckLabel.weight(.semibold))
                            .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("peers-start")
                }
            } else {
                ForEach(Array(controller.peers.enumerated()), id: \.element.id) { index, peer in
                    VStack(alignment: .leading, spacing: DuckSpace.sm) {
                        HStack(spacing: DuckSpace.sm) {
                            Circle()
                                .fill(peer.replied ? Color.green : Color.cyan)
                                .frame(width: 8, height: 8)
                            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                                Text(peer.name)
                                    .font(.duckLabel.weight(.semibold))
                                Text(peer.replied ? Copy.peerRepliedLabel : Copy.peerNew)
                                    .font(.duckCaption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            chip(
                                peer.paired ? Copy.peerRepliedLabel : Copy.pairCode,
                                id: "peer-pair-\(index)",
                                enabled: pairCode.count == 6 && !peer.paired
                            ) {
                                controller.pairPeer(code: pairCode, id: peer.id)
                            }
                            chip(Copy.peerSayHi, id: "peer-greet-\(index)") {
                                controller.greetPeer(peer.id)
                            }
                        }
                        // Gifts in the greeting's own chip language: a treat
                        // feeds through the real peck, a cheer runs the
                        // official excited timeline on the other duck.
                        HStack(spacing: DuckSpace.sm) {
                            chip(Copy.giftTreat, id: "gift-treat-\(peer.id)") {
                                controller.giftPeer(peer.id, gift: "treat", value: "")
                            }
                            chip(Copy.giftEmotion, id: "gift-emotion-\(peer.id)") {
                                controller.giftPeer(peer.id, gift: "emotion", value: "excited")
                            }
                            // This phone as the official pad, aimed at THIS
                            // paired neighbour. One tap dials, one stops.
                            if peer.paired && !peer.addr.isEmpty {
                                chip(
                                    controller.padTargetName == peer.name ? Copy.padTarget : Copy.padSender,
                                    id: "pad-send-\(peer.id)"
                                ) {
                                    controller.togglePad(peer: peer)
                                }
                            }
                        }
                    }
                    .padding(.horizontal, DuckSpace.md)
                    .padding(.vertical, DuckSpace.sm)
                    .frame(minHeight: 56)
                    .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: DuckRadius.thumb))
                    // No identifier here: one on a container reaches into the
                    // tree and overwrites every child's identifier (the chips
                    // below all vanished under "peer-row-N" in a CI dump).
                    // Android's smoke owns the peer-row-N anchor; iOS asserts
                    // the chips by their own ids.
                }
                Button {
                    controller.greetPeer(nil)
                } label: {
                    Label(Copy.peerSayAll, systemImage: "hand.wave.fill")
                        .font(.duckLabel.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("peer-greet-all")
            }

            HStack(spacing: DuckSpace.sm) {
                chip(Copy.formation, id: "ensemble-formation", enabled: !controller.peers.isEmpty) {
                    controller.startEnsemble(moves: ["laugh", "yes_fast", "excited", "curious"])
                }
                chip(Copy.roleDance, id: "ensemble-roles", enabled: !controller.peers.isEmpty) {
                    controller.startEnsemble(moves: ["laugh", "yes_fast", "excited", "curious", "sad", "inquire"])
                }
                chip(Copy.voiceChoir, id: "ensemble-choir", enabled: !controller.peers.isEmpty) {
                    controller.startEnsemble(
                        moves: ["laugh", "curious"],
                        sounds: ["chirp", "coo", "greet", "inquire"]
                    )
                }
            }

            // One song for the whole room: this duck leads and every known
            // neighbour joins. A lone duck has nobody to sing with, so the
            // chip stays visible but disabled rather than disappearing.
            chip(Copy.chorus, id: "chorus-start", enabled: !controller.peers.isEmpty) {
                controller.startChorus()
            }

            // The official UDP pad, OFF until the pair code gates it.
            Toggle(isOn: Binding(
                get: { controller.netPadOn },
                set: { controller.setNetPad(on: $0) }
            )) {
                Text(Copy.netPad)
                    .font(.duckCaption)
            }
            .accessibilityIdentifier("peers-netpad")
            Text(Copy.netPadNote)
                .font(.duckCaption)
                .foregroundStyle(.secondary)

            Text(Copy.voiceNote)
                .font(.duckCaption)
                .foregroundStyle(.tertiary)
        }
        .padding(DuckSpace.xl)
    }

    /// The sheet's one chip shape: the greeting's capsule, reused by gifts
    /// and the chorus. A disabled chip keeps its label readable, so the
    /// affordance is discovered before there is anyone to use it with.
    private func chip(
        _ title: String,
        id: String,
        enabled: Bool = true,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Text(title)
                .font(.duckCaption.weight(.semibold))
                .padding(.horizontal, DuckSpace.md)
                .frame(minHeight: 44)
                .background(Color.white.opacity(0.12), in: Capsule())
                .contentShape(Capsule())
                .opacity(enabled ? 1 : 0.45)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityLabel(title)
        .accessibilityIdentifier(id)
    }
}
