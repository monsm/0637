import SwiftUI

/// The other ducks on this Wi-Fi. Discovery is the core's UDP announce; a
/// greeting is one of the duck's social reactions, and the answer comes back
/// as "X answered!". No address is typed by the user.
struct PeersSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss

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

            Text(Copy.voiceNote)
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.tertiary)
        }
        .padding(20)
        .preferredColorScheme(.dark)
    }

    /// The sheet's one chip shape: the greeting's capsule, reused by gifts.
    private func chip(
        _ title: String,
        id: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 12, weight: .semibold, design: .rounded))
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .background(Color.white.opacity(0.12), in: Capsule())
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .accessibilityIdentifier(id)
    }
}
