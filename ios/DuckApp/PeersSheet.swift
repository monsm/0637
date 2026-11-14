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
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(Copy.peers)
                        .font(.system(size: 20, weight: .semibold, design: .rounded))
                        .accessibilityIdentifier("peers-title")
                    if !controller.peerName.isEmpty {
                        Text("\(Copy.name) · \(controller.peerName)")
                            .font(.system(size: 11, design: .rounded))
                            .foregroundStyle(.secondary)
                    }
                }
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
                .accessibilityIdentifier("peers-close")
            }

            // Pairing: a stranger on the Wi-Fi is visible but cannot send
            // gifts or start a chorus until both sides typed this code.
            VStack(alignment: .leading, spacing: 6) {
                TextField(Copy.pairCode, text: $pairCode)
                    .keyboardType(.numberPad)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("peers-pair-code")
                Text(Copy.pairNote)
                    .font(.system(size: 10, design: .rounded))
                    .foregroundStyle(.secondary)
            }

            if controller.peers.isEmpty {
                Text(Copy.peersEmpty)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("peers-empty")
                if controller.peerName.isEmpty {
                    Button {
                        controller.startPeers()
                    } label: {
                        Label(Copy.peerFind, systemImage: "wifi")
                            .font(.system(size: 13, weight: .semibold, design: .rounded))
                            .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("peers-start")
                }
            } else {
                ForEach(Array(controller.peers.enumerated()), id: \.element.id) { index, peer in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 10) {
                            Circle()
                                .fill(peer.replied ? Color.green : Color.cyan)
                                .frame(width: 8, height: 8)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(peer.name)
                                    .font(.system(size: 14, weight: .semibold, design: .rounded))
                                Text(peer.replied ? Copy.peerRepliedLabel : Copy.peerNew)
                                    .font(.system(size: 10, design: .rounded))
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
                        HStack(spacing: 8) {
                            chip(Copy.giftTreat, id: "gift-treat-\(peer.id)") {
                                controller.giftPeer(peer.id, gift: "treat", value: "")
                            }
                            chip(Copy.giftEmotion, id: "gift-emotion-\(peer.id)") {
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
                                controller.giftPeer(peer.id, gift: "emotion", value: "excited")
                            }
                        }
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .frame(minHeight: 56)
                    .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 12))
                    .accessibilityIdentifier("peer-row-\(index)")
                }
                Button {
                    controller.greetPeer(nil)
                } label: {
                    Label(Copy.peerSayAll, systemImage: "hand.wave.fill")
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("peer-greet-all")
            }

            HStack(spacing: 8) {
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
                    .font(.system(size: 12, design: .rounded))
            }
            .accessibilityIdentifier("peers-netpad")
            Text(Copy.netPadNote)
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.secondary)

            Text(Copy.voiceNote)
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.tertiary)
        }
        .padding(20)
        .preferredColorScheme(.dark)
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
                .font(.system(size: 12, weight: .semibold, design: .rounded))
                .padding(.horizontal, 14)
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
